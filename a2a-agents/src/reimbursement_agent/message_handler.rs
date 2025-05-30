use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::HashSet;
use std::sync::Mutex;
use uuid::Uuid;

use a2a_rs::domain::{A2AError, Artifact, Message, Part, Role, Task, TaskState, TaskStatus};
use a2a_rs::port::message_handler::AsyncMessageHandler;

// In a real implementation, this would be a proper database
lazy_static::lazy_static! {
    static ref REQUEST_IDS: Mutex<HashSet<String>> = Mutex::new(HashSet::new());
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestForm {
    pub request_id: String,
    pub date: String,
    pub amount: String,
    pub purpose: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReimbursementResult {
    pub request_id: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FormResponse {
    #[serde(rename = "type")]
    pub form_type: String,
    pub form: FormSchema,
    pub form_data: RequestForm,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FormSchema {
    #[serde(rename = "type")]
    pub schema_type: String,
    pub properties: FormProperties,
    pub required: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FormProperties {
    pub date: FormField,
    pub amount: FormField,
    pub purpose: FormField,
    pub request_id: FormField,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FormField {
    #[serde(rename = "type")]
    pub field_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    pub description: String,
    pub title: String,
}

#[derive(Debug, Clone)]
struct MessageContent {
    text: Option<String>,
    data: Option<Map<String, Value>>,
    files: Vec<FileInfo>,
}

#[derive(Debug, Clone)]
struct FileInfo {
    name: Option<String>,
    mime_type: Option<String>,
    has_content: bool,
    metadata: Option<Map<String, Value>>,
}

/// Modern message handler for reimbursement processing
/// 
/// Supports multiple content types:
/// - Text: Natural language reimbursement requests
/// - Data: Structured JSON form submissions
/// - File: Receipt attachments (future: OCR processing)
#[derive(Clone)]
pub struct ReimbursementMessageHandler {
    // Future: Add database connection, LLM client, OCR service, etc.
}

impl ReimbursementMessageHandler {
    pub fn new() -> Self {
        Self {}
    }

    /// Extract and process content from message parts
    fn extract_message_content(&self, message: &Message) -> Result<MessageContent, A2AError> {
        if message.parts.is_empty() {
            return Err(A2AError::ValidationError {
                field: "message.parts".to_string(),
                message: "Message must contain at least one part".to_string(),
            });
        }

        let mut text_content = String::new();
        let mut data_content: Option<Map<String, Value>> = None;
        let mut files = Vec::new();

        for part in &message.parts {
            match part {
                Part::Text { text, .. } => {
                    if !text_content.is_empty() {
                        text_content.push(' ');
                    }
                    text_content.push_str(text);
                }
                Part::Data { data, .. } => {
                    // Merge data parts if multiple exist
                    if let Some(ref mut existing_data) = data_content {
                        for (key, value) in data {
                            existing_data.insert(key.clone(), value.clone());
                        }
                    } else {
                        data_content = Some(data.clone());
                    }
                }
                Part::File { file, metadata } => {
                    files.push(FileInfo {
                        name: file.name.clone(),
                        mime_type: file.mime_type.clone(),
                        has_content: file.bytes.is_some() || file.uri.is_some(),
                        metadata: metadata.clone(),
                    });
                    
                    // Log receipt handling (future implementation)
                    if let Some(ref mime) = file.mime_type {
                        if mime.starts_with("image/") || mime == "application/pdf" {
                            tracing::info!("Receipt file detected: {:?}", file.name);
                        }
                    }
                }
            }
        }

        Ok(MessageContent {
            text: if text_content.is_empty() { None } else { Some(text_content) },
            data: data_content,
            files,
        })
    }

    /// Create a request form with generated or extracted parameters
    fn create_request_form(
        &self,
        date: Option<String>,
        amount: Option<String>,
        purpose: Option<String>,
    ) -> RequestForm {
        let request_id = format!("req_{}", Uuid::new_v4().simple());

        // Add to the set of known request IDs
        if let Ok(mut ids) = REQUEST_IDS.lock() {
            ids.insert(request_id.clone());
        } else {
            tracing::error!("Failed to acquire lock on request store");
        }

        // Validate and clean input values
        let clean_date = date
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| "<transaction date>".to_string());
        
        let clean_amount = amount
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| "<transaction dollar amount>".to_string());
        
        let clean_purpose = purpose
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| "<business justification/purpose of the transaction>".to_string());

        RequestForm {
            request_id,
            date: clean_date,
            amount: clean_amount,
            purpose: clean_purpose,
        }
    }

    /// Generate a form response for the client
    fn create_form_response(&self, form_request: RequestForm) -> FormResponse {
        FormResponse {
            form_type: "form".to_string(),
            form: FormSchema {
                schema_type: "object".to_string(),
                properties: FormProperties {
                    date: FormField {
                        field_type: "string".to_string(),
                        format: Some("date".to_string()),
                        description: "Date of expense".to_string(),
                        title: "Date".to_string(),
                    },
                    amount: FormField {
                        field_type: "string".to_string(),
                        format: Some("number".to_string()),
                        description: "Amount of expense".to_string(),
                        title: "Amount".to_string(),
                    },
                    purpose: FormField {
                        field_type: "string".to_string(),
                        format: None,
                        description: "Purpose of expense".to_string(),
                        title: "Purpose".to_string(),
                    },
                    request_id: FormField {
                        field_type: "string".to_string(),
                        format: None,
                        description: "Request ID".to_string(),
                        title: "Request ID".to_string(),
                    },
                },
                required: vec![
                    "request_id".to_string(),
                    "date".to_string(),
                    "amount".to_string(),
                    "purpose".to_string(),
                ],
            },
            form_data: form_request,
            instructions: Some("Please fill out all required fields for your reimbursement request.".to_string()),
        }
    }

    /// Process a reimbursement request
    fn process_reimbursement(&self, request_id: &str) -> Result<ReimbursementResult, A2AError> {
        // Validate request ID format
        if !request_id.starts_with("req_") {
            return Err(A2AError::ValidationError {
                field: "request_id".to_string(),
                message: "Request ID must start with 'req_'".to_string(),
            });
        }

        // Check if request ID exists
        let valid = REQUEST_IDS.lock()
            .map_err(|_| A2AError::Internal("Failed to acquire lock on request store".to_string()))?
            .contains(request_id);

        if !valid {
            return Err(A2AError::ValidationError {
                field: "request_id".to_string(),
                message: format!("Request ID '{}' not found or already processed", request_id),
            });
        }

        // In a real implementation, this would:
        // - Validate amounts against policy limits
        // - Check receipt attachments
        // - Verify manager approval if needed
        // - Update database status
        
        Ok(ReimbursementResult {
            request_id: request_id.to_string(),
            status: "approved".to_string(),
        })
    }

    /// Extract reimbursement data from message content
    fn extract_reimbursement_data(&self, content: &MessageContent) -> Result<RequestForm, A2AError> {
        // First check if we have structured data (e.g., from a form submission)
        if let Some(ref data) = content.data {
            // Try to parse as RequestForm directly
            if let Ok(form) = serde_json::from_value::<RequestForm>(Value::Object(data.clone())) {
                // Validate and store the request ID
                if !form.request_id.is_empty() {
                    REQUEST_IDS.lock()
                        .map_err(|_| A2AError::Internal("Failed to acquire lock on request store".to_string()))?
                        .insert(form.request_id.clone());
                }
                return Ok(form);
            }

            // Otherwise, extract fields from the data
            let request_id = data.get("request_id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| format!("req_{}", Uuid::new_v4().simple()));

            let date = data.get("date")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            let amount = data.get("amount")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .or_else(|| {
                    data.get("amount")
                        .and_then(|v| v.as_f64())
                        .map(|f| {
                            // Validate amount is positive
                            if f <= 0.0 {
                                tracing::warn!("Invalid amount: {} (must be positive)", f);
                                return format!("<invalid amount: ${:.2}>", f);
                            }
                            format!("${:.2}", f)
                        })
                });

            let purpose = data.get("purpose")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            if date.is_some() && amount.is_some() && purpose.is_some() {
                let form = RequestForm {
                    request_id: request_id.clone(),
                    date: date.unwrap(),
                    amount: amount.unwrap(),
                    purpose: purpose.unwrap(),
                };
                REQUEST_IDS.lock()
                    .map_err(|_| A2AError::Internal("Failed to acquire lock on request store".to_string()))?
                    .insert(request_id);
                return Ok(form);
            }
        }

        // Fall back to text parsing if no structured data
        if let Some(ref text) = content.text {
            let (date, amount, purpose) = self.parse_text_for_reimbursement(text);
            return Ok(self.create_request_form(date, amount, purpose));
        }

        // If no content to parse, return empty form
        Ok(self.create_request_form(None, None, None))
    }

    /// Parse text content for reimbursement information
    fn parse_text_for_reimbursement(&self, text: &str) -> (Option<String>, Option<String>, Option<String>) {
        let mut amount = None;
        let mut purpose = None;
        let date = None; // For now, we don't extract dates

        // Simple amount extraction
        if text.contains('$') {
            let parts: Vec<&str> = text.split('$').collect();
            if parts.len() > 1 {
                let amount_parts: Vec<&str> = parts[1].split_whitespace().collect();
                if !amount_parts.is_empty() {
                    // Remove any non-numeric characters except decimal point
                    let clean_amount: String = amount_parts[0]
                        .chars()
                        .filter(|c| c.is_ascii_digit() || *c == '.')
                        .collect();
                    if !clean_amount.is_empty() {
                        amount = Some(format!("${}", clean_amount));
                    }
                }
            }
        }

        // Simple purpose extraction
        if text.contains("for ") {
            let parts: Vec<&str> = text.split("for ").collect();
            if parts.len() > 1 {
                purpose = Some(parts[1].trim().to_string());
            }
        }

        (date, amount, purpose)
    }

    /// Process reimbursement request based on content type
    fn process_reimbursement_request(&self, content: &MessageContent) -> Result<(String, bool), A2AError> {
        // Check if this is a form submission (has structured data)
        if let Some(ref data) = content.data {
            // Check if it has request_id field (form submission)
            if data.contains_key("request_id") {
                // Try to extract and validate the form data
                match self.extract_reimbursement_data(content) {
                    Ok(form) => {
                        // Check if all required fields are filled
                        if form.date.contains("transaction date") || 
                           form.amount.contains("dollar amount") || 
                           form.purpose.contains("justification") {
                            // Form is incomplete
                            return Ok((
                                "The form is incomplete. Please provide all required information.".to_string(),
                                false
                            ));
                        }

                        // Process the complete form
                        match self.process_reimbursement(&form.request_id) {
                            Ok(result) => {
                                match serde_json::to_string(&result) {
                                    Ok(json) => Ok((json, false)),
                                    Err(e) => {
                                        tracing::error!("Failed to serialize reimbursement result: {}", e);
                                        Err(A2AError::Internal(
                                            "Failed to serialize reimbursement result".to_string()
                                        ))
                                    }
                                }
                            }
                            Err(e) => {
                                tracing::warn!("Reimbursement processing failed: {}", e);
                                Err(e)
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Failed to extract reimbursement data: {}", e);
                        Err(A2AError::InvalidParams(
                            "Invalid form data: missing or malformed required fields".to_string()
                        ))
                    }
                }
            } else {
                // Data without request_id, treat as initial request
                self.handle_initial_request(content)
            }
        } else if let Some(ref text) = content.text {
            // Text-based request
            if text.to_lowercase().contains("reimburse") || 
               text.to_lowercase().contains("expense") ||
               text.to_lowercase().contains("receipt") {
                // Initial reimbursement request
                self.handle_initial_request(content)
            } else if text.contains("request_id") || text.contains("req_") {
                // Text containing request ID - might be a follow-up
                Ok(("Please use the form to submit your reimbursement details.".to_string(), false))
            } else {
                // General query
                Ok((
                    "I'm a reimbursement agent. I can help you with:\n\
                    • Processing expense reimbursements\n\
                    • Generating reimbursement forms\n\
                    • Tracking reimbursement status\n\
                    \nJust tell me about your expense and I'll help you get reimbursed!".to_string(),
                    false
                ))
            }
        } else {
            // No content to process
            Ok((
                "I didn't receive any content to process. Please describe your reimbursement request.".to_string(),
                false
            ))
        }
    }

    /// Handle initial reimbursement request
    fn handle_initial_request(&self, content: &MessageContent) -> Result<(String, bool), A2AError> {
        // Extract reimbursement data from content
        let form = self.extract_reimbursement_data(content)?;
        let form_response = self.create_form_response(form);
        
        match serde_json::to_string(&form_response) {
            Ok(json) => Ok((json, true)),
            Err(e) => {
                tracing::error!("Failed to serialize form response: {}", e);
                Err(A2AError::Internal(
                    "Failed to generate reimbursement form".to_string()
                ))
            }
        }
    }

    /// Create a response message with proper task state
    fn create_response_message(
        &self,
        content: String,
        is_form: bool,
        _task_id: &str,
    ) -> Result<(Message, TaskState), A2AError> {
        // Determine task state based on response type
        let task_state = if is_form {
            TaskState::InputRequired
        } else {
            TaskState::Completed
        };

        // Create message parts based on content type
        let parts = if is_form {
            // For forms, we could send both text and data parts
            // The text part contains the JSON for compatibility
            // The data part contains the structured form (future enhancement)
            vec![Part::text(content)]
        } else {
            // For regular responses, just send text
            vec![Part::text(content)]
        };

        // Create the response message
        let message = Message::builder()
            .role(Role::Agent)
            .parts(parts)
            .message_id(Uuid::new_v4().to_string())
            .build();

        Ok((message, task_state))
    }
}

impl Default for ReimbursementMessageHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl AsyncMessageHandler for ReimbursementMessageHandler {
    async fn process_message<'a>(
        &self,
        task_id: &'a str,
        message: &'a Message,
        _session_id: Option<&'a str>,
    ) -> Result<Task, A2AError> {
        // Extract all content from the message
        let content = self.extract_message_content(message)?;

        tracing::info!("Processing reimbursement message with {} text parts, {} data parts, {} files",
            if content.text.is_some() { 1 } else { 0 },
            if content.data.is_some() { 1 } else { 0 },
            content.files.len()
        );

        // Log file information if present
        for file in &content.files {
            tracing::info!("Received file: {:?} (type: {:?})", file.name, file.mime_type);
        }

        // Determine the type of request based on content
        let (response_content, is_form) = self.process_reimbursement_request(&content)?;

        // Create the response message and determine task state
        let (response_message, task_state) = self.create_response_message(response_content.clone(), is_form, task_id)?;

        // Create task status
        let status = TaskStatus {
            state: task_state,
            message: Some(response_message.clone()),
            timestamp: Some(chrono::Utc::now()),
        };

        // Create artifact with the response (for future use)
        let _artifact = Artifact {
            artifact_id: Uuid::new_v4().to_string(),
            name: Some("Reimbursement Response".to_string()),
            description: Some("Response from reimbursement agent".to_string()),
            parts: vec![Part::text(response_content)],
            metadata: None,
        };

        // Create and return the task
        let task = Task::builder()
            .id(task_id.to_string())
            .context_id(message.context_id.clone().unwrap_or_else(|| Uuid::new_v4().to_string()))
            .status(status)
            .build();

        tracing::info!("Reimbursement task processed successfully");
        Ok(task)
    }

    async fn validate_message<'a>(&self, message: &'a Message) -> Result<(), A2AError> {
        // Call the default validation first
        if message.parts.is_empty() {
            return Err(A2AError::ValidationError {
                field: "message.parts".to_string(),
                message: "Message must contain at least one part".to_string(),
            });
        }

        // Additional reimbursement-specific validation
        for part in &message.parts {
            match part {
                Part::Text { text, .. } => {
                    if text.is_empty() {
                        return Err(A2AError::ValidationError {
                            field: "text".to_string(),
                            message: "Text parts cannot be empty".to_string(),
                        });
                    }
                }
                Part::Data { data, .. } => {
                    if data.is_empty() {
                        return Err(A2AError::ValidationError {
                            field: "data".to_string(),
                            message: "Data parts cannot be empty".to_string(),
                        });
                    }
                }
                Part::File { file, .. } => {
                    // Validate file has content
                    if file.bytes.is_none() && file.uri.is_none() {
                        return Err(A2AError::ValidationError {
                            field: "file".to_string(),
                            message: "File must have either bytes or URI".to_string(),
                        });
                    }
                    
                    // Validate supported file types for receipts
                    if let Some(ref mime_type) = file.mime_type {
                        let supported_types = ["image/jpeg", "image/png", "image/gif", "application/pdf"];
                        if !supported_types.contains(&mime_type.as_str()) {
                            return Err(A2AError::ContentTypeNotSupported(
                                format!("File type '{}' not supported for receipts. Supported types: {}", 
                                    mime_type, 
                                    supported_types.join(", ")
                                )
                            ));
                        }
                    }
                }
            }
        }

        Ok(())
    }
}