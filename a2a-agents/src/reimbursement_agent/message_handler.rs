use async_trait::async_trait;
use serde::{Deserialize, Serialize};
// use serde_json::{Map, Value}; // Unused for now
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

/// Modern message handler for reimbursement processing
pub struct ReimbursementMessageHandler {
    // Future: Add database connection, LLM client, etc.
}

impl ReimbursementMessageHandler {
    pub fn new() -> Self {
        Self {}
    }

    /// Extract text content from message parts
    fn extract_text_content(&self, message: &Message) -> Result<String, A2AError> {
        if message.parts.is_empty() {
            return Err(A2AError::ValidationError {
                field: "message.parts".to_string(),
                message: "Message must contain at least one part".to_string(),
            });
        }

        // Combine all text parts
        let mut text_content = String::new();
        for part in &message.parts {
            match part {
                Part::Text { text, .. } => {
                    if !text_content.is_empty() {
                        text_content.push(' ');
                    }
                    text_content.push_str(text);
                }
                Part::Data { data, .. } => {
                    // Handle structured data (e.g., form submissions)
                    if let Ok(text) = serde_json::to_string(data) {
                        if !text_content.is_empty() {
                            text_content.push(' ');
                        }
                        text_content.push_str(&text);
                    }
                }
                Part::File { .. } => {
                    // Future: Handle file uploads (receipts, etc.)
                    tracing::info!("File part received but not yet implemented");
                }
            }
        }

        Ok(text_content)
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
        REQUEST_IDS.lock().unwrap().insert(request_id.clone());

        RequestForm {
            request_id,
            date: date.unwrap_or_else(|| "<transaction date>".to_string()),
            amount: amount.unwrap_or_else(|| "<transaction dollar amount>".to_string()),
            purpose: purpose.unwrap_or_else(|| {
                "<business justification/purpose of the transaction>".to_string()
            }),
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
    fn process_reimbursement(&self, request_id: &str) -> ReimbursementResult {
        let valid = REQUEST_IDS.lock().unwrap().contains(request_id);

        if !valid {
            return ReimbursementResult {
                request_id: request_id.to_string(),
                status: "Error: Invalid request_id.".to_string(),
            };
        }

        ReimbursementResult {
            request_id: request_id.to_string(),
            status: "approved".to_string(),
        }
    }

    /// Extract structured data from text using simple parsing
    /// In a real implementation, this would use an LLM or NLP
    fn extract_reimbursement_data(&self, text: &str) -> (Option<String>, Option<String>, Option<String>) {
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

        // Create the response message
        let message = Message::builder()
            .role(Role::Agent)
            .parts(vec![Part::text(content)])
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
        // Extract text content from the message
        let text_content = self.extract_text_content(message)?;

        tracing::info!("Processing reimbursement message: {}", text_content);

        // Process the message based on content
        let (response_content, is_form) = if text_content.contains("reimburse") {
            // Initial reimbursement request
            let (date, amount, purpose) = self.extract_reimbursement_data(&text_content);
            let form = self.create_request_form(date, amount, purpose);
            let form_response = self.create_form_response(form);
            
            match serde_json::to_string(&form_response) {
                Ok(json) => (json, true),
                Err(_) => ("Error creating reimbursement form".to_string(), false),
            }
        } else if text_content.contains("request_id") || text_content.contains("req_") {
            // Form submission or follow-up
            if text_content.contains("MISSING_INFO") {
                ("The form is incomplete. Please provide all required information.".to_string(), false)
            } else {
                // Try to extract request ID for processing
                // In a real implementation, this would parse the JSON properly
                let result = self.process_reimbursement("req_sample");
                match serde_json::to_string(&result) {
                    Ok(json) => (json, false),
                    Err(_) => ("Error processing reimbursement".to_string(), false),
                }
            }
        } else {
            // General query
            ("I'm a reimbursement agent. How can I help you with your reimbursement request?".to_string(), false)
        };

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
                Part::File { .. } => {
                    // Future: Validate file content (receipts, etc.)
                    tracing::info!("File validation not yet implemented");
                }
            }
        }

        Ok(())
    }
}