use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tracing::{debug, error, info, instrument, warn};
use uuid::Uuid;

use a2a_rs::domain::{A2AError, Message, Part, Role, Task, TaskState, TaskStatus};
use a2a_rs::port::message_handler::AsyncMessageHandler;

use super::types::*;

/// Storage for reimbursement requests (in production, this would be a database)
#[derive(Debug, Clone)]
struct ReimbursementStore {
    requests: Arc<Mutex<HashMap<String, StoredRequest>>>,
}

#[derive(Debug, Clone)]
struct StoredRequest {
    #[allow(dead_code)]
    request: ReimbursementRequest,
    status: ProcessingStatus,
    #[allow(dead_code)]
    created_at: DateTime<Utc>,
    #[allow(dead_code)]
    updated_at: DateTime<Utc>,
    #[allow(dead_code)]
    receipts: Vec<ReceiptMetadata>,
    #[allow(dead_code)]
    metadata: Option<Map<String, Value>>,
}

impl Default for ReimbursementStore {
    fn default() -> Self {
        Self {
            requests: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

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
#[derive(Clone)]
pub struct ReimbursementHandler {
    store: ReimbursementStore,
    validation_rules: ValidationRules,
    file_metadata_store: Arc<Mutex<HashMap<String, Map<String, Value>>>>,
    metrics: Arc<Mutex<HandlerMetrics>>,
}

impl ReimbursementHandler {
    pub fn new() -> Self {
        Self {
            store: ReimbursementStore::default(),
            validation_rules: ValidationRules::default(),
            file_metadata_store: Arc::new(Mutex::new(HashMap::new())),
            metrics: Arc::new(Mutex::new(HandlerMetrics::default())),
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
                    data.get("category_hint")
                        .and_then(|v| v.as_str())
                        .and_then(|s| {
                            serde_json::from_value::<ExpenseCategory>(Value::String(s.to_string()))
                                .ok()
                        })
                })
                .or_else(|| {
                    // Try expense_type from metadata
                    data.get("expense_type")
                        .and_then(|v| v.as_str())
                        .map(|s| match s.to_lowercase().as_str() {
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

            if let Some(request_id) = request_id {
                // Form submission with request_id
                if let (Some(date), Some(amount), Some(purpose), Some(category)) = (
                    date.clone(),
                    amount.clone(),
                    purpose.clone(),
                    category.clone(),
                ) {
                    return Ok(ReimbursementRequest::FormSubmission {
                        request_id: request_id.to_string(),
                        date,
                        amount,
                        purpose,
                        category,
                        receipt_files,
                        notes,
                    });
                }
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
        if lower_text.contains("status") && text.contains("req_") {
            if let Some(start) = text.find("req_") {
                let request_id: String = text[start..]
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                return Ok(ReimbursementRequest::StatusQuery { request_id });
            }
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

        // Extract purpose (text after "for")
        if let Some(for_pos) = lower_text.find(" for ") {
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
        } else if lower_text.contains("supply") || lower_text.contains("supplies") {
            category = Some(ExpenseCategory::Supplies);
        } else if lower_text.contains("equipment") || lower_text.contains("hardware") {
            category = Some(ExpenseCategory::Equipment);
        } else if lower_text.contains("training") || lower_text.contains("course") {
            category = Some(ExpenseCategory::Training);
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

                // Create form data with any partial information
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
                if let Some(ref files) = receipt_files {
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

                // Check for auto-approval in metadata
                let auto_approve = self
                    .store
                    .requests
                    .lock()
                    .ok()
                    .and({
                        // In a real system, we'd get this from the message metadata
                        // For now, we'll auto-approve small amounts
                        match &amount {
                            Money::Number { amount, .. } if *amount < 100.0 => Some(true),
                            _ => None,
                        }
                    })
                    .unwrap_or(false);

                let status = if auto_approve {
                    ProcessingStatus::Approved
                } else {
                    ProcessingStatus::Pending
                };

                let stored_request = StoredRequest {
                    request: request.clone(),
                    status: status.clone(),
                    created_at: Utc::now(),
                    updated_at: Utc::now(),
                    receipts,
                    metadata: Some({
                        let mut meta = Map::new();
                        meta.insert("auto_approved".to_string(), Value::Bool(auto_approve));
                        meta
                    }),
                };

                self.store
                    .requests
                    .lock()
                    .map_err(|_| A2AError::Internal("Failed to acquire lock".to_string()))?
                    .insert(request_id.clone(), stored_request);

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
                // Look up the request
                let requests = self
                    .store
                    .requests
                    .lock()
                    .map_err(|_| A2AError::Internal("Failed to acquire lock".to_string()))?;

                if let Some(stored) = requests.get(request_id) {
                    Ok(ReimbursementResponse::Result {
                        request_id: request_id.clone(),
                        status: stored.status.clone(),
                        message: Some(format!("Request status: {:?}", stored.status)),
                        details: None,
                    })
                } else {
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

impl Default for ReimbursementHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl AsyncMessageHandler for ReimbursementHandler {
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
        info!("Processing reimbursement request");

        // Parse and validate the request
        let request = match self.parse_request(message) {
            Ok(req) => {
                info!(request_type = ?std::mem::discriminant(&req), "Request parsed successfully");
                req
            }
            Err(e) => {
                error!(error = %e, "Failed to parse request");
                return Err(e);
            }
        };

        if let Err(e) = self.validate_request(&request) {
            warn!(error = %e, "Request validation failed");
            return Err(e);
        }

        // Process the request
        let (response, auto_approved) = match self.process_request(request) {
            Ok(resp) => {
                info!(response_type = ?std::mem::discriminant(&resp), "Request processed successfully");
                let auto_approved = matches!(&resp, ReimbursementResponse::Result { status, .. } if matches!(status, super::types::ProcessingStatus::Approved));
                (resp, auto_approved)
            }
            Err(e) => {
                error!(error = %e, "Failed to process request");
                // Convert error to response
                let resp = match e {
                    A2AError::ValidationError { field, message } => ReimbursementResponse::Error {
                        code: "VALIDATION_ERROR".to_string(),
                        message,
                        field: Some(field),
                        suggestions: None,
                    },
                    _ => ReimbursementResponse::Error {
                        code: "PROCESSING_ERROR".to_string(),
                        message: e.to_string(),
                        field: None,
                        suggestions: None,
                    },
                };
                (resp, false)
            }
        };

        // Update metrics
        self.update_metrics(&response, auto_approved);

        // Determine task state based on response type
        let task_state = match &response {
            ReimbursementResponse::Form { .. } => TaskState::InputRequired,
            ReimbursementResponse::Result { status, .. } => match status {
                ProcessingStatus::RequiresAdditionalInfo => TaskState::InputRequired,
                _ => TaskState::Completed,
            },
            ReimbursementResponse::Error { .. } => TaskState::Failed,
        };

        // Create response message
        let response_parts = self.response_to_parts(response);
        let response_message = if let Some(context_id) = &message.context_id {
            Message::builder()
                .role(Role::Agent)
                .parts(response_parts)
                .message_id(Uuid::new_v4().to_string())
                .context_id(context_id.clone())
                .build()
        } else {
            Message::builder()
                .role(Role::Agent)
                .parts(response_parts)
                .message_id(Uuid::new_v4().to_string())
                .build()
        };

        // Create task status
        let status = TaskStatus {
            state: task_state.clone(),
            message: Some(response_message),
            timestamp: Some(Utc::now()),
        };

        // Build and return task
        let task = Task::builder()
            .id(task_id.to_string())
            .context_id(
                message
                    .context_id
                    .clone()
                    .unwrap_or_else(|| Uuid::new_v4().to_string()),
            )
            .status(status)
            .build();

        info!(task_state = ?task_state, "Successfully processed reimbursement request");
        Ok(task)
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
