use std::collections::HashSet;
use std::sync::Mutex;
use rand::Rng;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use std::fmt;
use futures::stream::{Stream, StreamExt};
use std::pin::Pin;
use std::task::{Context, Poll};

// In a real implementation, we'd use a proper database or storage service
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
pub struct StreamItem {
    pub is_task_complete: bool,
    pub content: StreamContent,
}

#[derive(Debug, Clone)]
pub enum StreamContent {
    Text(String),
    Data(serde_json::Value),
}

impl fmt::Display for StreamContent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StreamContent::Text(text) => write!(f, "{}", text),
            StreamContent::Data(data) => write!(f, "{}", serde_json::to_string(data).unwrap_or_default()),
        }
    }
}

/// Simulated stream implementation for agent responses
pub struct AgentStream {
    receiver: mpsc::Receiver<StreamItem>,
}

impl Stream for AgentStream {
    type Item = StreamItem;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Pin::new(&mut self.receiver).poll_recv(cx)
    }
}

pub struct ReimbursementAgent {
    // In a real implementation, this would contain LLM client, etc.
}

impl ReimbursementAgent {
    pub const SUPPORTED_CONTENT_TYPES: [&'static str; 2] = ["text", "text/plain"];
    
    pub fn new() -> Self {
        Self {}
    }

    /// Create a request form with the given parameters
    pub fn create_request_form(&self, date: Option<String>, amount: Option<String>, purpose: Option<String>) -> RequestForm {
        let mut rng = rand::thread_rng();
        let request_id = format!("request_id_{}", rng.gen_range(1000000..9999999));
        
        // Add to the set of known request IDs
        REQUEST_IDS.lock().unwrap().insert(request_id.clone());
        
        RequestForm {
            request_id,
            date: date.unwrap_or_else(|| "<transaction date>".to_string()),
            amount: amount.unwrap_or_else(|| "<transaction dollar amount>".to_string()),
            purpose: purpose.unwrap_or_else(|| "<business justification/purpose of the transaction>".to_string()),
        }
    }

    /// Generate a form response for the client
    pub fn return_form(&self, form_request: RequestForm, instructions: Option<String>) -> FormResponse {
        let required_fields = vec![
            "request_id".to_string(),
            "date".to_string(), 
            "amount".to_string(), 
            "purpose".to_string()
        ];
        
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
                        description: "Request id".to_string(),
                        title: "Request ID".to_string(),
                    },
                },
                required: required_fields,
            },
            form_data: form_request,
            instructions,
        }
    }

    /// Process a reimbursement request
    pub fn reimburse(&self, request_id: &str) -> ReimbursementResult {
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

    /// Simulate the agent's synchronous response processing
    pub fn invoke(&self, query: &str, _session_id: &str) -> String {
        // This is a simplified implementation - in the real world, 
        // this would call an LLM or other AI agent
        
        if query.contains("reimburse") {
            // Parse basic request details
            let date = None;
            let mut amount = None;
            let mut purpose = None;
            
            if query.contains("$") {
                // Simple extraction logic - in a real implementation,
                // this would be handled by the LLM
                let parts: Vec<&str> = query.split("$").collect();
                if parts.len() > 1 {
                    let amount_parts: Vec<&str> = parts[1].split_whitespace().collect();
                    if !amount_parts.is_empty() {
                        amount = Some(amount_parts[0].to_string());
                    }
                }
            }
            
            if query.contains("for") {
                let parts: Vec<&str> = query.split("for").collect();
                if parts.len() > 1 {
                    purpose = Some(parts[1].trim().to_string());
                }
            }
            
            // Create a request form
            let form = self.create_request_form(date, amount, purpose);
            
            // Return the form as JSON
            let form_response = self.return_form(form, None);
            serde_json::to_string(&form_response).unwrap_or_else(|_| "Error creating form".to_string())
        } 
        else if query.contains("request_id") {
            // This would be a follow-up with a filled form
            // In a real implementation, this would parse the JSON and extract fields
            
            // For demo purposes:
            if query.contains("MISSING_INFO") {
                "The form is incomplete. Please provide all required information.".to_string()
            } else {
                let reimburse_result = self.reimburse("request_id_1234567");
                format!("Your reimbursement request has been {}. Request ID: {}", 
                    reimburse_result.status, reimburse_result.request_id)
            }
        } 
        else {
            "I'm a reimbursement agent. How can I help you with your reimbursement request?".to_string()
        }
    }

    /// Simulate the agent's streaming response
    pub fn stream(&self, query: &str, _session_id: &str) -> AgentStream {
        let (sender, receiver) = mpsc::channel(10);
        let query = query.to_string();
        
        // Spawn a task to simulate processing
        tokio::spawn(async move {
            // Send a processing message
            let _ = sender.send(StreamItem {
                is_task_complete: false,
                content: StreamContent::Text("Processing the reimbursement request...".to_string()),
            }).await;
            
            // Simulate some processing delay
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
            
            // Now send the final response
            let response = if query.contains("reimburse") {
                // Parse basic request details (simplified)
                let date = None;
                let mut amount = None;
                let mut purpose = None;
                
                if query.contains("$") {
                    let parts: Vec<&str> = query.split("$").collect();
                    if parts.len() > 1 {
                        let amount_parts: Vec<&str> = parts[1].split_whitespace().collect();
                        if !amount_parts.is_empty() {
                            amount = Some(amount_parts[0].to_string());
                        }
                    }
                }
                
                if query.contains("for") {
                    let parts: Vec<&str> = query.split("for").collect();
                    if parts.len() > 1 {
                        purpose = Some(parts[1].trim().to_string());
                    }
                }
                
                // Create a request form
                let form = RequestForm {
                    request_id: "request_id_1234567".to_string(),
                    date: date.unwrap_or_else(|| "<transaction date>".to_string()),
                    amount: amount.unwrap_or_else(|| "<transaction dollar amount>".to_string()),
                    purpose: purpose.unwrap_or_else(|| "<business justification/purpose of the transaction>".to_string()),
                };
                
                // Generate a form response
                let form_response = FormResponse {
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
                                description: "Request id".to_string(),
                                title: "Request ID".to_string(),
                            },
                        },
                        required: vec!["request_id".to_string(), "date".to_string(), "amount".to_string(), "purpose".to_string()],
                    },
                    form_data: form,
                    instructions: None,
                };
                
                StreamContent::Data(serde_json::to_value(form_response).unwrap())
            } else if query.contains("request_id") {
                // This would be a follow-up with a filled form
                if query.contains("MISSING_INFO") {
                    StreamContent::Text("The form is incomplete. Please provide all required information.".to_string())
                } else {
                    let result = ReimbursementResult {
                        request_id: "request_id_1234567".to_string(),
                        status: "approved".to_string(),
                    };
                    StreamContent::Data(serde_json::to_value(result).unwrap())
                }
            } else {
                StreamContent::Text("I'm a reimbursement agent. How can I help you with your reimbursement request?".to_string())
            };
            
            // Send the final response
            let _ = sender.send(StreamItem {
                is_task_complete: true,
                content: response,
            }).await;
        });
        
        AgentStream { receiver }
    }
}