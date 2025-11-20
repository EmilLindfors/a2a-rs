use async_trait::async_trait;
use chrono::Utc;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tracing::{debug, error, info, instrument, warn};
use uuid::Uuid;

use a2a_rs::domain::{A2AError, Message, Part, Role, Task, TaskState};
use a2a_rs::port::message_handler::AsyncMessageHandler;

use super::ai_client::{AiClient, ChatMessage};
use super::types::*;

// NOTE: Task storage is handled by DefaultRequestProcessor + SQLx/InMemory storage
// This handler is stateless and only processes messages

/// Metrics for tracking handler performance
#[derive(Debug, Default, Clone)]
pub struct HandlerMetrics {
    pub total_requests: u64,
    pub successful_requests: u64,
    pub validation_errors: u64,
    pub processing_errors: u64,
    pub forms_generated: u64,
    pub approvals_processed: u64,
    pub auto_approvals: u64,
}

impl HandlerMetrics {
    fn increment_requests(&mut self) {
        self.total_requests += 1;
    }

    fn increment_success(&mut self) {
        self.successful_requests += 1;
    }

    fn increment_validation_errors(&mut self) {
        self.validation_errors += 1;
    }

    fn increment_processing_errors(&mut self) {
        self.processing_errors += 1;
    }

    fn increment_forms(&mut self) {
        self.forms_generated += 1;
    }

    fn increment_approvals(&mut self) {
        self.approvals_processed += 1;
    }

    fn increment_auto_approvals(&mut self) {
        self.auto_approvals += 1;
    }

    fn log_metrics(&self) {
        info!(
            total_requests = self.total_requests,
            successful_requests = self.successful_requests,
            validation_errors = self.validation_errors,
            processing_errors = self.processing_errors,
            forms_generated = self.forms_generated,
            approvals_processed = self.approvals_processed,
            auto_approvals = self.auto_approvals,
            success_rate = if self.total_requests > 0 {
                (self.successful_requests as f64 / self.total_requests as f64) * 100.0
            } else {
                0.0
            },
            "Handler metrics"
        );
    }
}

/// Reimbursement message handler with proper JSON parsing and validation
/// Reimbursement handler that manages task history through a task manager
#[derive(Clone)]
pub struct ReimbursementHandler<T>
where
    T: a2a_rs::port::AsyncTaskManager + Clone + Send + Sync + 'static,
{
    task_manager: T,
    validation_rules: ValidationRules,
    #[allow(dead_code)]
    file_metadata_store: Arc<Mutex<HashMap<String, Map<String, Value>>>>,
    metrics: Arc<Mutex<HandlerMetrics>>,
    ai_client: Option<AiClient>,
}

impl<T> ReimbursementHandler<T>
where
    T: a2a_rs::port::AsyncTaskManager + Clone + Send + Sync + 'static,
{
    pub fn new(task_manager: T) -> Self {
        // Try to initialize AI client from environment
        let ai_client = match AiClient::from_env() {
            Ok(client) => {
                info!("AI client initialized successfully");
                Some(client)
            }
            Err(e) => {
                warn!(
                    "Failed to initialize AI client: {}. Conversational features will be disabled.",
                    e
                );
                None
            }
        };

        Self {
            task_manager,
            validation_rules: ValidationRules::default(),
            file_metadata_store: Arc::new(Mutex::new(HashMap::new())),
            metrics: Arc::new(Mutex::new(HandlerMetrics::default())),
            ai_client,
        }
    }

    pub fn with_validation_rules(mut self, rules: ValidationRules) -> Self {
        self.validation_rules = rules;
        self
    }

    /// Merge metadata from parts into a combined metadata map
    fn merge_metadata(&self, target: &mut Map<String, Value>, source: &Map<String, Value>) {
        for (key, value) in source {
            // Handle special metadata keys that might influence processing
            match key.as_str() {
                "expense_type" | "category_hint" | "date_format" | "currency" => {
                    target.insert(key.clone(), value.clone());
                }
                "auto_approve" if value.is_boolean() => {
                    target.insert(key.clone(), value.clone());
                }
                "priority" if value.is_string() => {
                    target.insert(key.clone(), value.clone());
                }
                _ => {
                    // Store other metadata for reference
                    if !key.starts_with("_") {
                        // Skip internal metadata
                        target.insert(key.clone(), value.clone());
                    }
                }
            }
        }
    }

    /// Store file metadata for later retrieval
    fn store_file_metadata(&self, file_id: &str, metadata: Map<String, Value>) {
        if let Ok(mut store) = self.file_metadata_store.lock() {
            store.insert(file_id.to_string(), metadata);
        }
    }

    /// Retrieve file metadata
    fn get_file_metadata(&self, file_id: &str) -> Option<Map<String, Value>> {
        self.file_metadata_store
            .lock()
            .ok()
            .and_then(|store| store.get(file_id).cloned())
    }

    /// Get current metrics
    pub fn get_metrics(&self) -> HandlerMetrics {
        self.metrics.lock().map(|m| m.clone()).unwrap_or_default()
    }

    /// Log current metrics
    pub fn log_metrics(&self) {
        if let Ok(metrics) = self.metrics.lock() {
            metrics.log_metrics();
        }
    }

    /// Update metrics based on processing results
    fn update_metrics(&self, response: &ReimbursementResponse, auto_approved: bool) {
        if let Ok(mut metrics) = self.metrics.lock() {
            metrics.increment_requests();

            match response {
                ReimbursementResponse::Form { .. } => {
                    metrics.increment_forms();
                    metrics.increment_success();
                }
                ReimbursementResponse::Result { status, .. } => {
                    metrics.increment_approvals();
                    if auto_approved {
                        metrics.increment_auto_approvals();
                    }
                    match status {
                        super::types::ProcessingStatus::Approved
                        | super::types::ProcessingStatus::Pending => {
                            metrics.increment_success();
                        }
                        _ => {}
                    }
                }
                ReimbursementResponse::Error { code, .. } => {
                    if code == "VALIDATION_ERROR" {
                        metrics.increment_validation_errors();
                    } else {
                        metrics.increment_processing_errors();
                    }
                }
            }
        }
    }

    /// System prompt for the AI reimbursement assistant
    fn get_system_prompt(&self) -> String {
        r#"You are an AI-powered reimbursement assistant. You MUST respond with valid JSON only.

Company Policy:
- Expenses under $100: auto-approved (if all info provided + receipt if amount > $25)
- Expenses $100+: requires manager approval (mark as "pending")
- Categories: travel, meals, supplies, equipment, training, other
- Required info: date, amount, purpose, category
- Receipt rules: ONLY required when amount is GREATER than $25 (>$25)
  * $25 or less: NO receipt needed
  * Over $25 (e.g. $25.01, $26, $30): Receipt REQUIRED

CRITICAL: You MUST respond with ONLY valid JSON in this exact format:

{
  "action": "approved",
  "message": "Your conversational message here",
  "extracted_data": {
    "date": "2025-10-24",
    "amount": 25.00,
    "currency": "USD",
    "purpose": "Office supplies",
    "category": "supplies",
    "has_receipt": true
  },
  "needs_clarification": [],
  "approval_reason": "Expense under $100 with receipt"
}

Action values (choose ONE):
- "approved": All info present + amount<$100 + (receipt uploaded if amount>$25)
- "pending": All info present + amount>=$100
- "question": Missing required info OR (amount>$25 AND no receipt uploaded)
- "error": Cannot process (system error, not user error)

Rules:
1. ALWAYS return valid JSON (no extra text before or after)
2. Use "question" action when info is missing
3. Set extracted_data fields to null if unknown
4. When user uploads file, set has_receipt=true and acknowledge in message
5. Be conversational in the "message" field only
6. IMPORTANT: Check math carefully - $23 is NOT over $25, $26 IS over $25

Example response when asking for info:
{
  "action": "question",
  "message": "What was the amount of the expense?",
  "extracted_data": {
    "date": null,
    "amount": null,
    "currency": "USD",
    "purpose": null,
    "category": null,
    "has_receipt": false
  },
  "needs_clarification": ["amount"],
  "approval_reason": null
}"#
        .to_string()
    }

    /// Extract text content from message parts
    fn extract_text_from_message(&self, message: &Message) -> String {
        let mut text_parts = Vec::new();
        for part in &message.parts {
            if let Part::Text { text, .. } = part {
                text_parts.push(text.clone());
            }
        }
        text_parts.join(" ")
    }

    /// Check if a message has file attachments
    fn has_file_attachments(&self, message: &Message) -> (bool, Vec<String>) {
        let mut has_files = false;
        let mut file_names = Vec::new();

        for part in &message.parts {
            if let Part::File { file, .. } = part {
                has_files = true;
                if let Some(ref name) = file.name {
                    file_names.push(name.clone());
                } else {
                    file_names.push("unnamed file".to_string());
                }
            }
        }

        (has_files, file_names)
    }

    /// Process a reimbursement request using AI
    async fn process_with_ai(
        &self,
        user_message: &str,
        task: Option<&Task>,
        current_message: &Message,
    ) -> Result<ReimbursementResponse, String> {
        let ai_client = self
            .ai_client
            .as_ref()
            .ok_or_else(|| "AI client not available - cannot process request".to_string())?;

        let system_prompt = self.get_system_prompt();

        // Build conversation history
        let mut history = Vec::new();

        if let Some(task) = task {
            if let Some(ref messages) = task.history {
                for msg in messages {
                    match msg.role {
                        Role::User => {
                            let text = self.extract_text_from_message(msg);
                            let (has_files, file_names) = self.has_file_attachments(msg);

                            let message_with_context = if has_files {
                                format!(
                                    "{}\n[User uploaded file(s): {}]",
                                    text,
                                    file_names.join(", ")
                                )
                            } else {
                                text
                            };

                            if !message_with_context.is_empty() {
                                history.push(ChatMessage::user(message_with_context));
                            }
                        }
                        Role::Agent => {
                            let text = self.extract_text_from_message(msg);
                            if !text.is_empty() {
                                history.push(ChatMessage::assistant(text));
                            }
                        }
                    }
                }
            }
        }

        // Check if current message has files
        let (has_files, file_names) = self.has_file_attachments(current_message);
        let current_message_text = if has_files {
            format!(
                "{}\n[User uploaded file(s): {}]",
                user_message,
                file_names.join(", ")
            )
        } else {
            user_message.to_string()
        };

        // Add current user message
        history.push(ChatMessage::user(current_message_text));

        // Get AI response with JSON mode enforced
        let ai_response = ai_client
            .ask_with_history_json(&system_prompt, history)
            .await?;

        info!(response_length = ai_response.len(), "Received AI response");

        // Parse AI response as JSON
        self.parse_ai_response(&ai_response)
    }

    /// Parse the AI's JSON response into a ReimbursementResponse
    fn parse_ai_response(&self, ai_response: &str) -> Result<ReimbursementResponse, String> {
        // Try to extract JSON from the response (AI might wrap it in markdown)
        let json_str = if let Some(start) = ai_response.find('{') {
            if let Some(end) = ai_response.rfind('}') {
                &ai_response[start..=end]
            } else {
                ai_response
            }
        } else {
            ai_response
        };

        #[derive(Debug, Deserialize)]
        struct AiResponse {
            action: String,
            message: String,
            #[serde(default)]
            extracted_data: Option<ExtractedData>,
            #[serde(default)]
            needs_clarification: Vec<String>,
            #[serde(default)]
            #[allow(dead_code)]
            approval_reason: Option<String>,
        }

        #[derive(Debug, Deserialize)]
        struct ExtractedData {
            #[allow(dead_code)]
            date: Option<String>,
            amount: Option<f64>,
            currency: Option<String>,
            #[allow(dead_code)]
            purpose: Option<String>,
            #[allow(dead_code)]
            category: Option<String>,
            #[allow(dead_code)]
            has_receipt: Option<bool>,
        }

        let parsed: AiResponse = serde_json::from_str(json_str).map_err(|e| {
            format!(
                "Failed to parse AI response as JSON: {}. Response was: {}",
                e, ai_response
            )
        })?;

        match parsed.action.as_str() {
            "approved" | "pending" => {
                let extracted = parsed
                    .extracted_data
                    .ok_or("Missing extracted_data for approval")?;

                let amount = Money::Number {
                    amount: extracted.amount.ok_or("Missing amount")?,
                    currency: extracted.currency.unwrap_or_else(|| "USD".to_string()),
                };

                let status = if parsed.action == "approved" {
                    ProcessingStatus::Approved
                } else {
                    ProcessingStatus::Pending
                };

                let details = ProcessingDetails {
                    approved_amount: Some(amount.clone()),
                    approval_date: Some(Utc::now().to_rfc3339()),
                    approver: Some("AI Assistant".to_string()),
                    rejection_reason: None,
                    required_documents: if extracted.amount.unwrap_or(0.0) > 25.0 {
                        Some(vec!["receipt".to_string()])
                    } else {
                        None
                    },
                };

                Ok(ReimbursementResponse::Result {
                    request_id: format!("req_{}", Uuid::new_v4().simple()),
                    status,
                    message: Some(parsed.message),
                    details: Some(details),
                })
            }
            "question" => {
                // AI needs more information - return as conversational response
                Ok(ReimbursementResponse::Error {
                    code: "NEEDS_MORE_INFO".to_string(),
                    message: parsed.message,
                    field: None,
                    suggestions: if !parsed.needs_clarification.is_empty() {
                        Some(parsed.needs_clarification)
                    } else {
                        None
                    },
                })
            }
            "error" => Ok(ReimbursementResponse::Error {
                code: "PROCESSING_ERROR".to_string(),
                message: parsed.message,
                field: None,
                suggestions: None,
            }),
            _ => Err(format!("Unknown action from AI: {}", parsed.action)),
        }
    }

    /// Parse message content into a reimbursement request
    #[instrument(skip(self, message), fields(message_id = %message.message_id, parts_count = message.parts.len()))]
    fn parse_request(&self, message: &Message) -> Result<ReimbursementRequest, A2AError> {
        // Extract all content from message parts
        let mut text_content = Vec::new();
        let mut data_content: Option<Map<String, Value>> = None;
        let mut file_ids = Vec::new();
        let mut metadata_hints: Map<String, Value> = Map::new();

        for (idx, part) in message.parts.iter().enumerate() {
            match part {
                Part::Text { text, metadata } => {
                    debug!(
                        part_index = idx,
                        text_length = text.len(),
                        "Processing text part"
                    );
                    text_content.push(text.clone());
                    // Extract any metadata hints for processing
                    if let Some(meta) = metadata {
                        debug!(metadata_keys = ?meta.keys().collect::<Vec<_>>(), "Found text metadata");
                        self.merge_metadata(&mut metadata_hints, meta);
                    }
                }
                Part::Data { data, metadata } => {
                    debug!(part_index = idx, data_keys = ?data.keys().collect::<Vec<_>>(), "Processing data part");
                    // Merge multiple data parts
                    if let Some(ref mut existing) = data_content {
                        for (k, v) in data {
                            existing.insert(k.clone(), v.clone());
                        }
                    } else {
                        data_content = Some(data.clone());
                    }
                    // Extract metadata
                    if let Some(meta) = metadata {
                        debug!(metadata_keys = ?meta.keys().collect::<Vec<_>>(), "Found data metadata");
                        self.merge_metadata(&mut metadata_hints, meta);
                    }
                }
                Part::File { file, metadata } => {
                    // Store file references with metadata
                    let file_id = if let Some(ref name) = file.name {
                        name.clone()
                    } else {
                        format!("file_{}", Uuid::new_v4().simple())
                    };
                    info!(part_index = idx, file_id = %file_id, mime_type = ?file.mime_type, "Processing file part");
                    file_ids.push(file_id.clone());

                    // Store file metadata for later processing
                    if let Some(meta) = metadata {
                        let mut file_meta = meta.clone();
                        file_meta.insert("file_id".to_string(), Value::String(file_id.clone()));
                        if let Some(mime) = &file.mime_type {
                            file_meta.insert("mime_type".to_string(), Value::String(mime.clone()));
                        }
                        debug!(file_id = %file_id, metadata_keys = ?file_meta.keys().collect::<Vec<_>>(), "Storing file metadata");
                        self.store_file_metadata(&file_id, file_meta);
                    }
                }
            }
        }

        // Apply metadata hints to data if available
        if let Some(mut data) = data_content {
            // Merge metadata hints into data for enhanced processing
            for (key, value) in metadata_hints {
                if !data.contains_key(&key) {
                    data.insert(key, value);
                }
            }
            data_content = Some(data);
        }

        // Try to parse structured data first
        if let Some(data) = data_content {
            // Try parsing as complete request types
            if let Ok(request) =
                serde_json::from_value::<ReimbursementRequest>(Value::Object(data.clone()))
            {
                return Ok(request);
            }

            // Check if it's a status query
            if let Some(request_id) = data.get("request_id").and_then(|v| v.as_str()) {
                if data.len() == 1 || data.get("action").and_then(|v| v.as_str()) == Some("status")
                {
                    return Ok(ReimbursementRequest::StatusQuery {
                        request_id: request_id.to_string(),
                    });
                }
            }

            // Try to build an initial or form submission request
            let request_id = data.get("request_id").and_then(|v| v.as_str());
            let date = data
                .get("date")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let amount = self.parse_money_from_value(data.get("amount"));
            let purpose = data
                .get("purpose")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            // Check for category from both direct field and metadata hints
            let category = data
                .get("category")
                .and_then(|v| serde_json::from_value::<ExpenseCategory>(v.clone()).ok())
                .or_else(|| {
                    // Try category_hint from metadata
                    let hint = data.get("category_hint")?.as_str()?;
                    serde_json::from_value::<ExpenseCategory>(Value::String(hint.to_string())).ok()
                })
                .or_else(|| {
                    // Try expense_type from metadata
                    let expense_type = data.get("expense_type")?.as_str()?;
                    Some(match expense_type.to_lowercase().as_str() {
                        "travel" => ExpenseCategory::Travel,
                        "meals" | "meal" => ExpenseCategory::Meals,
                        "supplies" | "supply" => ExpenseCategory::Supplies,
                        "equipment" => ExpenseCategory::Equipment,
                        "training" => ExpenseCategory::Training,
                        _ => ExpenseCategory::Other,
                    })
                });

            let notes = data
                .get("notes")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            let receipt_files = if !file_ids.is_empty() {
                Some(file_ids.clone())
            } else {
                data.get("receipt_files")
                    .and_then(|v| serde_json::from_value::<Vec<String>>(v.clone()).ok())
            };

            if let Some(request_id) = request_id
                && let (Some(date), Some(amount), Some(purpose), Some(category)) = (
                    date.clone(),
                    amount.clone(),
                    purpose.clone(),
                    category.clone(),
                )
            {
                // Form submission with request_id
                return Ok(ReimbursementRequest::FormSubmission {
                    request_id: request_id.to_string(),
                    date,
                    amount,
                    purpose,
                    category,
                    receipt_files,
                    notes,
                });
            } else {
                // Initial request without request_id
                return Ok(ReimbursementRequest::Initial {
                    date,
                    amount,
                    purpose,
                    category,
                    receipt_files,
                });
            }
        }

        // Fall back to text parsing
        if !text_content.is_empty() {
            let combined_text = text_content.join(" ");
            return self.parse_text_request(&combined_text, file_ids);
        }

        Err(A2AError::InvalidParams(
            "No valid request data found in message".to_string(),
        ))
    }

    /// Parse money from various JSON value formats with metadata awareness
    fn parse_money_from_value_with_metadata(
        &self,
        value: Option<&Value>,
        metadata: &Map<String, Value>,
    ) -> Option<Money> {
        // Check for currency hint in metadata
        let default_currency = metadata
            .get("currency")
            .and_then(|v| v.as_str())
            .unwrap_or("USD")
            .to_string();

        match value? {
            Value::String(s) => Some(Money::String(s.clone())),
            Value::Number(n) => n.as_f64().map(|amount| Money::Number {
                amount,
                currency: default_currency,
            }),
            Value::Object(obj) => {
                let amount = obj.get("amount")?.as_f64()?;
                let currency = obj
                    .get("currency")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&default_currency)
                    .to_string();
                Some(Money::Number { amount, currency })
            }
            _ => None,
        }
    }

    /// Parse money from various JSON value formats (legacy method for compatibility)
    fn parse_money_from_value(&self, value: Option<&Value>) -> Option<Money> {
        self.parse_money_from_value_with_metadata(value, &Map::new())
    }

    /// Parse text content for reimbursement information
    fn parse_text_request(
        &self,
        text: &str,
        file_ids: Vec<String>,
    ) -> Result<ReimbursementRequest, A2AError> {
        let lower_text = text.to_lowercase();

        // Check if it's a status query
        if lower_text.contains("status")
            && text.contains("req_")
            && let Some(start) = text.find("req_")
        {
            let request_id: String = text[start..]
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            return Ok(ReimbursementRequest::StatusQuery { request_id });
        }

        // Parse as initial request
        let mut date = None;
        let mut amount = None;
        let mut purpose = None;
        let mut category = None;

        // Extract amount
        if let Some(dollar_pos) = text.find('$') {
            let amount_str: String = text[dollar_pos + 1..]
                .chars()
                .take_while(|c| c.is_numeric() || *c == '.')
                .collect();
            if let Ok(parsed_amount) = amount_str.parse::<f64>() {
                amount = Some(Money::Number {
                    amount: parsed_amount,
                    currency: "USD".to_string(),
                });
            }
        }

        // Extract date patterns (MM/DD/YYYY, YYYY-MM-DD, etc.)
        let date_patterns = [
            r"\d{1,2}/\d{1,2}/\d{4}",
            r"\d{4}-\d{2}-\d{2}",
            r"\d{1,2}-\d{1,2}-\d{4}",
        ];
        for pattern in &date_patterns {
            if let Ok(regex) = regex::Regex::new(pattern) {
                if let Some(mat) = regex.find(text) {
                    date = Some(mat.as_str().to_string());
                    break;
                }
            }
        }

        // Extract purpose (text after "for" or "Description:")
        if let Some(desc_pos) = text.find("Description:") {
            // Extract text from "Description:" until next line or end
            let desc_start = desc_pos + "Description:".len();
            let purpose_text = text[desc_start..].lines().next().unwrap_or("").trim();
            if !purpose_text.is_empty() {
                purpose = Some(purpose_text.to_string());
            }
        } else if let Some(for_pos) = lower_text.find(" for ") {
            let purpose_text = text[for_pos + 5..].trim();
            if !purpose_text.is_empty() {
                purpose = Some(purpose_text.to_string());
            }
        }

        // Detect category from keywords
        if lower_text.contains("travel")
            || lower_text.contains("flight")
            || lower_text.contains("hotel")
        {
            category = Some(ExpenseCategory::Travel);
        } else if lower_text.contains("meal")
            || lower_text.contains("lunch")
            || lower_text.contains("dinner")
        {
            category = Some(ExpenseCategory::Meals);
        } else if lower_text.contains("office")
            || lower_text.contains("supply")
            || lower_text.contains("supplies")
        {
            category = Some(ExpenseCategory::Supplies);
        } else if lower_text.contains("equipment") || lower_text.contains("hardware") {
            category = Some(ExpenseCategory::Equipment);
        } else if lower_text.contains("training") || lower_text.contains("course") {
            category = Some(ExpenseCategory::Training);
        } else if lower_text.contains("other") {
            category = Some(ExpenseCategory::Other);
        }

        let receipt_files = if !file_ids.is_empty() {
            Some(file_ids)
        } else {
            None
        };

        Ok(ReimbursementRequest::Initial {
            date,
            amount,
            purpose,
            category,
            receipt_files,
        })
    }

    /// Validate a reimbursement request
    #[instrument(skip(self, request), fields(request_type = ?std::mem::discriminant(request)))]
    fn validate_request(&self, request: &ReimbursementRequest) -> Result<(), A2AError> {
        match request {
            ReimbursementRequest::Initial { amount, .. } => {
                // Validate amount if provided
                if let Some(money) = amount {
                    if let Err(e) = money.validate() {
                        warn!(error = %e, "Amount validation failed");
                        return Err(A2AError::ValidationError {
                            field: "amount".to_string(),
                            message: e,
                        });
                    }
                }
                Ok(())
            }
            ReimbursementRequest::FormSubmission {
                date,
                amount,
                purpose,
                category,
                ..
            } => {
                // Validate required fields
                if date.trim().is_empty() {
                    return Err(A2AError::ValidationError {
                        field: "date".to_string(),
                        message: "Date is required".to_string(),
                    });
                }

                if purpose.trim().is_empty() {
                    return Err(A2AError::ValidationError {
                        field: "purpose".to_string(),
                        message: "Purpose is required".to_string(),
                    });
                }

                // Validate amount
                if let Err(e) = amount.validate() {
                    return Err(A2AError::ValidationError {
                        field: "amount".to_string(),
                        message: e,
                    });
                }

                // Validate against rules
                if !self.validation_rules.allowed_categories.contains(category) {
                    return Err(A2AError::ValidationError {
                        field: "category".to_string(),
                        message: format!("Category {:?} is not allowed", category),
                    });
                }

                // TODO: Add more validation (date range, amount limits, etc.)

                Ok(())
            }
            ReimbursementRequest::StatusQuery { request_id } => {
                if !request_id.starts_with("req_") {
                    return Err(A2AError::ValidationError {
                        field: "request_id".to_string(),
                        message: "Invalid request ID format".to_string(),
                    });
                }
                Ok(())
            }
        }
    }

    /// Process a reimbursement request and generate appropriate response
    #[instrument(skip(self, request), fields(request_type = ?std::mem::discriminant(&request)))]
    fn process_request(
        &self,
        request: ReimbursementRequest,
    ) -> Result<ReimbursementResponse, A2AError> {
        match &request {
            ReimbursementRequest::Initial {
                date,
                amount,
                purpose,
                category,
                receipt_files,
            } => {
                // Generate a new request ID
                let request_id = format!("req_{}", Uuid::new_v4().simple());

                // Check if we have all required fields for auto-processing
                let has_all_fields = date.is_some()
                    && amount.is_some()
                    && purpose.is_some()
                    && !purpose.as_ref().unwrap_or(&String::new()).trim().is_empty()
                    && category.is_some();

                // Check for auto-approval based on amount (if we have all fields)
                if has_all_fields {
                    let auto_approve = match amount.as_ref().unwrap() {
                        Money::Number { amount, .. } if *amount < 100.0 => true,
                        Money::Number { .. } => false, // >= $100
                        Money::String(s) => {
                            // Try to parse string amount
                            let amount_str = s.trim_start_matches('$').replace(",", "");
                            if let Ok(amt) = amount_str.parse::<f64>() {
                                amt < 100.0
                            } else {
                                false
                            }
                        }
                    };

                    if auto_approve {
                        // Auto-approve small expenses with all required fields
                        let details = ProcessingDetails {
                            approved_amount: amount.clone(),
                            approval_date: Some(Utc::now().to_rfc3339()),
                            approver: Some("System Auto-Approval".to_string()),
                            rejection_reason: None,
                            required_documents: None,
                        };

                        return Ok(ReimbursementResponse::Result {
                            request_id: request_id.clone(),
                            status: ProcessingStatus::Approved,
                            message: Some(format!(
                                "✅ Your expense of {} has been automatically approved! No further action needed.",
                                amount.as_ref().unwrap().to_formatted_string()
                            )),
                            details: Some(details),
                        });
                    }
                }

                // Otherwise, return a form for completion/review
                let form_data = FormData {
                    request_id: request_id.clone(),
                    date: date.clone().or_else(|| Some(String::new())),
                    amount: amount
                        .as_ref()
                        .map(|m| m.to_formatted_string())
                        .or_else(|| Some(String::new())),
                    purpose: purpose.clone().or_else(|| Some(String::new())),
                    category: category
                        .as_ref()
                        .map(|c| format!("{:?}", c).to_lowercase())
                        .or_else(|| Some("other".to_string())),
                    receipt_files: receipt_files.clone(),
                    notes: None,
                };

                // Create form schema
                let form_schema = self.create_form_schema();

                Ok(ReimbursementResponse::Form {
                    form: form_schema,
                    form_data,
                    instructions: Some(
                        "Please complete all required fields for your reimbursement request. \
                        Receipts are required for expenses over $25."
                            .to_string(),
                    ),
                })
            }
            ReimbursementRequest::FormSubmission {
                request_id,
                date: _,
                amount,
                purpose: _,
                category: _,
                receipt_files,
                notes: _,
            } => {
                // Store the request
                // Process receipt files with metadata
                let mut receipts = vec![];
                if let Some(files) = receipt_files {
                    for file_id in files {
                        if let Some(metadata) = self.get_file_metadata(file_id) {
                            receipts.push(ReceiptMetadata {
                                file_id: file_id.clone(),
                                file_name: metadata
                                    .get("file_name")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or(file_id)
                                    .to_string(),
                                mime_type: metadata
                                    .get("mime_type")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("application/octet-stream")
                                    .to_string(),
                                size_bytes: metadata
                                    .get("size_bytes")
                                    .and_then(|v| v.as_u64())
                                    .unwrap_or(0)
                                    as usize,
                                upload_timestamp: Some(Utc::now().to_rfc3339()),
                                extracted_data: None, // TODO: OCR integration
                            });
                        }
                    }
                }

                // Check for auto-approval based on amount
                // In a real system, we'd get this from the message metadata or business rules
                // For now, we'll auto-approve small amounts
                let auto_approve =
                    matches!(amount, Money::Number { amount, .. } if *amount < 100.0);

                let status = if auto_approve {
                    ProcessingStatus::Approved
                } else {
                    ProcessingStatus::Pending
                };

                // NOTE: Task storage is handled by DefaultRequestProcessor
                // We just process the message and return a response
                // (receipts metadata is still tracked in file_metadata_store if needed)

                // In a real implementation, this would trigger workflow processing
                let details = ProcessingDetails {
                    approved_amount: Some(amount.clone()),
                    approval_date: Some(Utc::now().to_rfc3339()),
                    approver: Some("System Auto-Approval".to_string()),
                    rejection_reason: None,
                    required_documents: None,
                };

                Ok(ReimbursementResponse::Result {
                    request_id: request_id.clone(),
                    status,
                    message: Some(if auto_approve {
                        "Your reimbursement request has been auto-approved for amounts under $100."
                            .to_string()
                    } else {
                        "Your reimbursement request has been submitted for review.".to_string()
                    }),
                    details: if auto_approve { Some(details) } else { None },
                })
            }
            ReimbursementRequest::StatusQuery { request_id } => {
                // NOTE: Actual task status is managed by DefaultRequestProcessor/SQLx
                // This handler doesn't have access to task storage directly
                // The client should use tasks/get instead for real status
                {
                    Err(A2AError::ValidationError {
                        field: "request_id".to_string(),
                        message: format!("Request {} not found", request_id),
                    })
                }
            }
        }
    }

    /// Create form schema for reimbursement requests
    fn create_form_schema(&self) -> FormSchema {
        let mut properties = Map::new();

        // Date field
        properties.insert(
            "date".to_string(),
            json!({
                "type": "string",
                "format": "date",
                "title": "Expense Date",
                "description": "Date when the expense was incurred"
            }),
        );

        // Amount field
        properties.insert(
            "amount".to_string(),
            json!({
                "type": "string",
                "format": "money",
                "title": "Amount",
                "description": "Total amount to be reimbursed",
                "pattern": r"^\$?\d+(\.\d{2})?$"
            }),
        );

        // Purpose field
        properties.insert(
            "purpose".to_string(),
            json!({
                "type": "string",
                "title": "Business Purpose",
                "description": "Business justification for the expense",
                "minLength": 10
            }),
        );

        // Category field
        properties.insert(
            "category".to_string(),
            json!({
                "type": "string",
                "title": "Expense Category",
                "enum": ["travel", "meals", "supplies", "equipment", "training", "other"],
                "description": "Category of the expense"
            }),
        );

        // Notes field (optional)
        properties.insert(
            "notes".to_string(),
            json!({
                "type": "string",
                "title": "Additional Notes",
                "description": "Any additional information (optional)"
            }),
        );

        // Request ID (hidden/readonly)
        properties.insert(
            "request_id".to_string(),
            json!({
                "type": "string",
                "title": "Request ID",
                "readOnly": true
            }),
        );

        FormSchema {
            schema_type: "object".to_string(),
            properties,
            required: vec![
                "request_id".to_string(),
                "date".to_string(),
                "amount".to_string(),
                "purpose".to_string(),
                "category".to_string(),
            ],
            dependencies: None,
        }
    }

    /// Convert response to message parts
    fn response_to_parts(&self, response: ReimbursementResponse) -> Vec<Part> {
        match response {
            ReimbursementResponse::Form { ref form, .. } => {
                // Serialize as JSON for both text and data parts
                if let Ok(json_str) = serde_json::to_string_pretty(&response) {
                    let mut metadata = Map::new();
                    metadata.insert(
                        "response_type".to_string(),
                        Value::String("form".to_string()),
                    );
                    metadata.insert(
                        "form_id".to_string(),
                        Value::String(
                            form.properties
                                .get("request_id")
                                .and_then(|v| v.get("default"))
                                .and_then(|v| v.as_str())
                                .unwrap_or("unknown")
                                .to_string(),
                        ),
                    );

                    vec![
                        Part::text_with_metadata(json_str.clone(), metadata.clone()),
                        Part::Data {
                            data: serde_json::from_str::<Map<String, Value>>(&json_str)
                                .unwrap_or_default(),
                            metadata: Some(metadata),
                        },
                    ]
                } else {
                    vec![Part::text("Failed to serialize response".to_string())]
                }
            }
            ReimbursementResponse::Result {
                ref request_id,
                ref status,
                ..
            } => {
                if let Ok(json_str) = serde_json::to_string_pretty(&response) {
                    let mut metadata = Map::new();
                    metadata.insert(
                        "response_type".to_string(),
                        Value::String("result".to_string()),
                    );
                    metadata.insert("request_id".to_string(), Value::String(request_id.clone()));
                    metadata.insert("status".to_string(), Value::String(format!("{:?}", status)));

                    vec![
                        Part::text_with_metadata(json_str.clone(), metadata.clone()),
                        Part::Data {
                            data: serde_json::from_str::<Map<String, Value>>(&json_str)
                                .unwrap_or_default(),
                            metadata: Some(metadata),
                        },
                    ]
                } else {
                    vec![Part::text("Failed to serialize response".to_string())]
                }
            }
            ReimbursementResponse::Error {
                ref message,
                ref code,
                ..
            } => {
                let mut metadata = Map::new();
                metadata.insert(
                    "response_type".to_string(),
                    Value::String("error".to_string()),
                );
                metadata.insert("error_code".to_string(), Value::String(code.clone()));

                vec![Part::text_with_metadata(message.clone(), metadata)]
            }
        }
    }
}

#[async_trait]
impl<T> AsyncMessageHandler for ReimbursementHandler<T>
where
    T: a2a_rs::port::AsyncTaskManager + Clone + Send + Sync + 'static,
{
    #[instrument(skip(self, message), fields(
        task_id = %task_id,
        message_id = %message.message_id,
        session_id = ?_session_id,
        parts_count = message.parts.len()
    ))]
    async fn process_message<'a>(
        &self,
        task_id: &'a str,
        message: &'a Message,
        _session_id: Option<&'a str>,
    ) -> Result<Task, A2AError> {
        error!(
            "🚨 HANDLER CALLED: Processing reimbursement request for task_id={}",
            task_id
        );
        info!("Processing reimbursement request");

        // Check if task exists and get its current state
        let existing_task = if self.task_manager.task_exists(task_id).await? {
            Some(self.task_manager.get_task(task_id, Some(50)).await?)
        } else {
            None
        };

        // If task doesn't exist, create it
        if existing_task.is_none() {
            let context_id = message
                .context_id
                .as_ref()
                .map(|s| s.to_string())
                .unwrap_or_else(|| Uuid::new_v4().to_string());
            self.task_manager.create_task(task_id, &context_id).await?;
        }

        // Check if this task already has a completed/approved expense
        // If so, treat this as a follow-up conversation message
        if let Some(ref task) = existing_task {
            if task.status.state == TaskState::Completed {
                // This is a follow-up to a completed task
                // Add user message to history
                self.task_manager
                    .update_task_status(task_id, TaskState::Working, Some(message.clone()))
                    .await?;

                // Send a simple acknowledgment
                let response_message = Message::builder()
                    .role(Role::Agent)
                    .parts(vec![Part::text("Your expense has already been processed. Is there anything else I can help you with?".to_string())])
                    .message_id(Uuid::new_v4().to_string())
                    .context_id(message.context_id.clone().unwrap_or_default())
                    .build();

                // Add agent response and return
                let final_task = self
                    .task_manager
                    .update_task_status(task_id, TaskState::Completed, Some(response_message))
                    .await?;

                return Ok(final_task);
            }
        }

        // Add the user's message to history first
        self.task_manager
            .update_task_status(task_id, TaskState::Working, Some(message.clone()))
            .await?;

        // Send immediate acknowledgment
        let ack_message = Message::builder()
            .role(Role::Agent)
            .parts(vec![Part::text("Processing your request...".to_string())])
            .message_id(Uuid::new_v4().to_string())
            .context_id(message.context_id.clone().unwrap_or_default())
            .build();

        self.task_manager
            .update_task_status(task_id, TaskState::Working, Some(ack_message))
            .await?;

        // Clone what we need for the background task
        let handler = self.clone();
        let task_id_owned = task_id.to_string();
        let message_owned = message.clone();
        let context_id = message.context_id.clone().unwrap_or_default();

        // Log before spawning
        info!(task_id = %task_id, "About to spawn background worker for AI processing");

        // Spawn background task to process with AI
        let spawn_result = tokio::task::spawn(async move {
            info!(task_id = %task_id_owned, "🚀 Background worker STARTED - beginning AI processing");

            // Extract text content from message
            let text_content = handler.extract_text_from_message(&message_owned);

            // Get current task for context
            let current_task = match handler
                .task_manager
                .get_task(&task_id_owned, Some(50))
                .await
            {
                Ok(task) => {
                    info!(task_id = %task_id_owned, history_count = task.history.as_ref().map(|h| h.len()).unwrap_or(0), "Retrieved task for AI processing");
                    Some(task)
                }
                Err(e) => {
                    error!(task_id = %task_id_owned, error = %e, "Failed to get task for AI processing");
                    None
                }
            };

            // Process with AI
            info!(task_id = %task_id_owned, "Calling AI for processing");
            let (response, auto_approved) = match handler
                .process_with_ai(&text_content, current_task.as_ref(), &message_owned)
                .await
            {
                Ok(resp) => {
                    info!(task_id = %task_id_owned, response_type = ?std::mem::discriminant(&resp), "AI processed request successfully");
                    let auto_approved = matches!(&resp, ReimbursementResponse::Result { status, .. } if matches!(status, super::types::ProcessingStatus::Approved));
                    (resp, auto_approved)
                }
                Err(e) => {
                    error!(task_id = %task_id_owned, error = %e, "AI processing failed");
                    let resp = ReimbursementResponse::Error {
                        code: "AI_PROCESSING_ERROR".to_string(),
                        message: format!("I'm having trouble processing your request. Please try again or provide more details. Error: {}", e),
                        field: None,
                        suggestions: Some(vec![
                            "Provide the date of the expense".to_string(),
                            "Specify the amount".to_string(),
                            "Describe the business purpose".to_string(),
                            "Select a category (travel, meals, supplies, equipment, training, or other)".to_string(),
                        ]),
                    };
                    (resp, false)
                }
            };

            // Update metrics
            handler.update_metrics(&response, auto_approved);

            // Determine task state based on response type
            let task_state = match &response {
                ReimbursementResponse::Form { .. } => TaskState::InputRequired,
                ReimbursementResponse::Result { status, .. } => match status {
                    ProcessingStatus::RequiresAdditionalInfo => TaskState::InputRequired,
                    _ => TaskState::Completed,
                },
                ReimbursementResponse::Error { code, .. } => {
                    if code == "NEEDS_MORE_INFO" {
                        TaskState::InputRequired
                    } else {
                        TaskState::Failed
                    }
                }
            };

            // Create response message
            let response_parts = handler.response_to_parts(response);
            let response_message = Message::builder()
                .role(Role::Agent)
                .parts(response_parts)
                .message_id(Uuid::new_v4().to_string())
                .context_id(context_id)
                .build();

            // Update task with AI response
            info!(task_id = %task_id_owned, new_state = ?task_state, "Updating task with AI response");
            match handler
                .task_manager
                .update_task_status(&task_id_owned, task_state.clone(), Some(response_message))
                .await
            {
                Ok(updated_task) => {
                    info!(
                        task_id = %task_id_owned,
                        task_state = ?task_state,
                        history_count = updated_task.history.as_ref().map(|h| h.len()).unwrap_or(0),
                        "Background worker: Successfully updated task with AI response (push notification should have been sent)"
                    );
                }
                Err(e) => {
                    error!(task_id = %task_id_owned, error = %e, "Background worker: Failed to update task with AI response");
                }
            }

            info!(task_id = %task_id_owned, "✅ Background worker COMPLETED successfully");
        });

        // Log that the task was spawned
        info!(task_id = %task_id, background_task_id = ?spawn_result.id(), "Background worker spawned successfully");

        // Return immediately - client will get updates via polling or push notifications
        info!(task_id = %task_id, "Returning immediate acknowledgment, AI processing in background");

        // Get the updated task with the acknowledgment message
        let final_task = self.task_manager.get_task(task_id, Some(50)).await?;
        Ok(final_task)
    }

    async fn validate_message<'a>(&self, message: &'a Message) -> Result<(), A2AError> {
        if message.parts.is_empty() {
            return Err(A2AError::ValidationError {
                field: "message.parts".to_string(),
                message: "Message must contain at least one part".to_string(),
            });
        }

        // Validate individual parts
        for (idx, part) in message.parts.iter().enumerate() {
            match part {
                Part::Text { text, .. } => {
                    if text.trim().is_empty() {
                        return Err(A2AError::ValidationError {
                            field: format!("parts[{}].text", idx),
                            message: "Text content cannot be empty".to_string(),
                        });
                    }
                }
                Part::Data { data, .. } => {
                    if data.is_empty() {
                        return Err(A2AError::ValidationError {
                            field: format!("parts[{}].data", idx),
                            message: "Data content cannot be empty".to_string(),
                        });
                    }
                }
                Part::File { file, .. } => {
                    if file.bytes.is_none() && file.uri.is_none() {
                        return Err(A2AError::ValidationError {
                            field: format!("parts[{}].file", idx),
                            message: "File must have either bytes or URI".to_string(),
                        });
                    }

                    // Validate mime type for receipts
                    if let Some(ref mime) = file.mime_type {
                        let supported_types = [
                            "image/jpeg",
                            "image/png",
                            "image/gif",
                            "image/webp",
                            "application/pdf",
                            "image/heic",
                            "image/heif",
                        ];
                        if !supported_types.contains(&mime.as_str()) {
                            return Err(A2AError::ContentTypeNotSupported(format!(
                                "Unsupported file type '{}'. Supported types: {}",
                                mime,
                                supported_types.join(", ")
                            )));
                        }
                    }
                }
            }
        }

        Ok(())
    }
}
