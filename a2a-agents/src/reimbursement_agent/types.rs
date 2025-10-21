use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Standard reimbursement request structure
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ReimbursementRequest {
    /// Initial request with partial data
    Initial {
        #[serde(skip_serializing_if = "Option::is_none")]
        date: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        amount: Option<Money>,
        #[serde(skip_serializing_if = "Option::is_none")]
        purpose: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        category: Option<ExpenseCategory>,
        #[serde(skip_serializing_if = "Option::is_none")]
        receipt_files: Option<Vec<String>>,
    },
    /// Complete form submission
    FormSubmission {
        request_id: String,
        date: String,
        amount: Money,
        purpose: String,
        category: ExpenseCategory,
        #[serde(skip_serializing_if = "Option::is_none")]
        receipt_files: Option<Vec<String>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        notes: Option<String>,
    },
    /// Status query
    StatusQuery { request_id: String },
}

/// Money type with proper validation
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum Money {
    /// String format like "$100.00" or "USD 100.00"
    String(String),
    /// Numeric format
    Number {
        amount: f64,
        #[serde(default = "default_currency")]
        currency: String,
    },
}

fn default_currency() -> String {
    "USD".to_string()
}

impl Money {
    pub fn to_formatted_string(&self) -> String {
        match self {
            Money::String(s) => s.clone(),
            Money::Number { amount, currency } => {
                if currency == "USD" {
                    format!("${:.2}", amount)
                } else {
                    format!("{} {:.2}", currency, amount)
                }
            }
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        match self {
            Money::String(s) => {
                // Basic validation for string format
                if s.is_empty() {
                    return Err("Amount cannot be empty".to_string());
                }
                Ok(())
            }
            Money::Number { amount, .. } => {
                if *amount <= 0.0 {
                    return Err("Amount must be positive".to_string());
                }
                if amount.is_nan() || amount.is_infinite() {
                    return Err("Amount must be a valid number".to_string());
                }
                Ok(())
            }
        }
    }
}

/// Expense categories
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ExpenseCategory {
    Travel,
    Meals,
    Supplies,
    Equipment,
    Training,
    Other,
}

impl ExpenseCategory {
    pub fn all() -> Vec<Self> {
        vec![
            Self::Travel,
            Self::Meals,
            Self::Supplies,
            Self::Equipment,
            Self::Training,
            Self::Other,
        ]
    }
}

/// Response types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ReimbursementResponse {
    /// Form response for initial requests
    #[serde(rename = "form")]
    Form {
        form: FormSchema,
        form_data: FormData,
        #[serde(skip_serializing_if = "Option::is_none")]
        instructions: Option<String>,
    },
    /// Result of processing
    #[serde(rename = "result")]
    Result {
        request_id: String,
        status: ProcessingStatus,
        #[serde(skip_serializing_if = "Option::is_none")]
        message: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        details: Option<ProcessingDetails>,
    },
    /// Error response
    #[serde(rename = "error")]
    Error {
        code: String,
        message: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        field: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        suggestions: Option<Vec<String>>,
    },
}

/// Processing status
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ProcessingStatus {
    Pending,
    UnderReview,
    Approved,
    Rejected,
    RequiresAdditionalInfo,
}

/// Processing details
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessingDetails {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub approved_amount: Option<Money>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub approval_date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub approver: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rejection_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required_documents: Option<Vec<String>>,
}

/// Form schema for dynamic form generation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FormSchema {
    #[serde(rename = "type")]
    pub schema_type: String,
    pub properties: Map<String, Value>,
    pub required: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dependencies: Option<Map<String, Value>>,
}

/// Form data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FormData {
    pub request_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub amount: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub purpose: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub receipt_files: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

impl FormData {
    pub fn new(request_id: String) -> Self {
        Self {
            request_id,
            date: None,
            amount: None,
            purpose: None,
            category: None,
            receipt_files: None,
            notes: None,
        }
    }

    pub fn with_partial_data(
        request_id: String,
        date: Option<String>,
        amount: Option<String>,
        purpose: Option<String>,
    ) -> Self {
        Self {
            request_id,
            date,
            amount,
            purpose,
            category: None,
            receipt_files: None,
            notes: None,
        }
    }
}

/// Receipt metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReceiptMetadata {
    pub file_id: String,
    pub file_name: String,
    pub mime_type: String,
    pub size_bytes: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upload_timestamp: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extracted_data: Option<ExtractedReceiptData>,
}

/// Data extracted from receipts (future OCR integration)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedReceiptData {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vendor_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_amount: Option<Money>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub items: Option<Vec<LineItem>>,
    pub confidence_score: f32,
}

/// Line item from receipt
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LineItem {
    pub description: String,
    pub amount: Money,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quantity: Option<f32>,
}

/// Validation rules for reimbursement requests
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationRules {
    pub max_amount: Money,
    pub allowed_categories: Vec<ExpenseCategory>,
    pub requires_receipt_above: Money,
    pub date_range_days: u32,
}

impl Default for ValidationRules {
    fn default() -> Self {
        Self {
            max_amount: Money::Number {
                amount: 5000.0,
                currency: "USD".to_string(),
            },
            allowed_categories: ExpenseCategory::all(),
            requires_receipt_above: Money::Number {
                amount: 25.0,
                currency: "USD".to_string(),
            },
            date_range_days: 90,
        }
    }
}
