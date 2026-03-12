//! AP2 payment receipt types.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::payment_request::PaymentCurrencyAmount;

/// Details about a successful payment.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Success {
    /// Transaction confirmation ID at the merchant.
    pub merchant_confirmation_id: String,
    /// Transaction confirmation ID at the PSP.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub psp_confirmation_id: Option<String>,
    /// Transaction confirmation ID at the network.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network_confirmation_id: Option<String>,
}

/// Details about an errored payment.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Error {
    /// Human-readable message explaining the error.
    pub error_message: String,
}

/// Details about a failed payment.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Failure {
    /// Human-readable message explaining the failure.
    pub failure_message: String,
}

/// The status of a completed payment.
///
/// Each variant has a distinct set of fields, so untagged deserialization
/// can identify the correct variant from the JSON shape.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PaymentStatus {
    Success(Success),
    Error(Error),
    Failure(Failure),
}

/// Information about the final state of a payment.
///
/// Exchanged via a data part with key
/// [`PAYMENT_RECEIPT_DATA_KEY`](super::roles::PAYMENT_RECEIPT_DATA_KEY).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PaymentReceipt {
    /// Reference to the processed payment mandate.
    pub payment_mandate_id: String,
    /// When the receipt was created, in ISO 8601 format.
    pub timestamp: String,
    /// Unique identifier for the payment.
    pub payment_id: String,
    /// Monetary amount of the payment.
    pub amount: PaymentCurrencyAmount,
    /// Outcome of the payment.
    pub payment_status: PaymentStatus,
    /// Payment-method-specific details.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payment_method_details: Option<HashMap<String, serde_json::Value>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn success_receipt_roundtrip() {
        let receipt = PaymentReceipt {
            payment_mandate_id: "pm_123".into(),
            timestamp: "2025-09-16T12:00:00Z".into(),
            payment_id: "pay_456".into(),
            amount: PaymentCurrencyAmount {
                currency: "USD".into(),
                value: 120.0,
            },
            payment_status: PaymentStatus::Success(Success {
                merchant_confirmation_id: "mc_789".into(),
                psp_confirmation_id: Some("psp_101".into()),
                network_confirmation_id: None,
            }),
            payment_method_details: None,
        };
        let json = serde_json::to_string(&receipt).unwrap();
        let back: PaymentReceipt = serde_json::from_str(&json).unwrap();
        assert_eq!(receipt, back);
    }

    #[test]
    fn error_receipt() {
        let receipt = PaymentReceipt {
            payment_mandate_id: "pm_123".into(),
            timestamp: "2025-09-16T12:00:00Z".into(),
            payment_id: "pay_456".into(),
            amount: PaymentCurrencyAmount {
                currency: "USD".into(),
                value: 120.0,
            },
            payment_status: PaymentStatus::Error(Error {
                error_message: "Card declined".into(),
            }),
            payment_method_details: None,
        };
        let json = serde_json::to_string(&receipt).unwrap();
        assert!(json.contains("error_message"));
        let back: PaymentReceipt = serde_json::from_str(&json).unwrap();
        assert_eq!(receipt, back);
    }

    #[test]
    fn failure_receipt() {
        let receipt = PaymentReceipt {
            payment_mandate_id: "pm_123".into(),
            timestamp: "2025-09-16T12:00:00Z".into(),
            payment_id: "pay_456".into(),
            amount: PaymentCurrencyAmount {
                currency: "USD".into(),
                value: 120.0,
            },
            payment_status: PaymentStatus::Failure(Failure {
                failure_message: "Insufficient funds".into(),
            }),
            payment_method_details: None,
        };
        let json = serde_json::to_string(&receipt).unwrap();
        assert!(json.contains("failure_message"));
        let back: PaymentReceipt = serde_json::from_str(&json).unwrap();
        assert_eq!(receipt, back);
    }
}
