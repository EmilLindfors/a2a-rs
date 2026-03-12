//! W3C Payment Request API types used by AP2.
//!
//! These types mirror the [W3C Payment Request API](https://www.w3.org/TR/payment-request/)
//! as adopted by the official AP2 Python SDK.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::contact::ContactAddress;

/// A monetary amount with currency code.
///
/// Follows the W3C `PaymentCurrencyAmount` dictionary.
///
/// See: <https://www.w3.org/TR/payment-request/#dom-paymentcurrencyamount>
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PaymentCurrencyAmount {
    /// Three-letter ISO 4217 currency code (e.g. `"USD"`).
    pub currency: String,
    /// Monetary value.
    pub value: f64,
}

/// An item for purchase and the value asked for it.
///
/// See: <https://www.w3.org/TR/payment-request/#dom-paymentitem>
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PaymentItem {
    /// Human-readable description of the item.
    pub label: String,
    /// The monetary amount of the item.
    pub amount: PaymentCurrencyAmount,
    /// If `true`, indicates the amount is not yet final.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending: Option<bool>,
    /// Refund duration for this item, in days. Defaults to 30.
    #[serde(default = "default_refund_period")]
    pub refund_period: i32,
}

fn default_refund_period() -> i32 {
    30
}

/// A shipping option with its cost.
///
/// See: <https://www.w3.org/TR/payment-request/#dom-paymentshippingoption>
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PaymentShippingOption {
    /// Unique identifier for the shipping option.
    pub id: String,
    /// Human-readable description.
    pub label: String,
    /// Cost of this shipping option.
    pub amount: PaymentCurrencyAmount,
    /// Whether this is the default selection.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected: Option<bool>,
}

/// Options controlling what payer information to collect.
///
/// See: <https://www.w3.org/TR/payment-request/#dom-paymentoptions>
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PaymentOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_payer_name: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_payer_email: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_payer_phone: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_shipping: Option<bool>,
    /// One of `"shipping"`, `"delivery"`, or `"pickup"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shipping_type: Option<String>,
}

impl Default for PaymentOptions {
    fn default() -> Self {
        Self {
            request_payer_name: Some(false),
            request_payer_email: Some(false),
            request_payer_phone: Some(false),
            request_shipping: Some(true),
            shipping_type: None,
        }
    }
}

/// A supported payment method and its associated data.
///
/// See: <https://www.w3.org/TR/payment-request/#dom-paymentmethoddata>
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PaymentMethodData {
    /// Payment method identifier (e.g. `"CARD"`, `"google-pay"`).
    pub supported_methods: String,
    /// Payment-method-specific data.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<HashMap<String, serde_json::Value>>,
}

/// A price modifier that applies when a specific payment method is selected.
///
/// See: <https://www.w3.org/TR/payment-request/#dom-paymentdetailsmodifier>
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PaymentDetailsModifier {
    /// The payment method ID this modifier applies to.
    pub supported_methods: String,
    /// Overrides the original item total for this method.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<PaymentItem>,
    /// Additional line items for this payment method.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub additional_display_items: Option<Vec<PaymentItem>>,
    /// Payment-method-specific data for the modifier.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<HashMap<String, serde_json::Value>>,
}

/// Details of the payment being requested.
///
/// See: <https://www.w3.org/TR/payment-request/#dom-paymentdetailsinit>
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PaymentDetailsInit {
    /// Unique identifier for this payment request.
    pub id: String,
    /// Line items to display to the user.
    pub display_items: Vec<PaymentItem>,
    /// Available shipping options.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shipping_options: Option<Vec<PaymentShippingOption>>,
    /// Price modifiers for particular payment methods.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modifiers: Option<Vec<PaymentDetailsModifier>>,
    /// Total payment amount.
    pub total: PaymentItem,
}

/// A request for payment.
///
/// See: <https://www.w3.org/TR/payment-request/#paymentrequest-interface>
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PaymentRequest {
    /// Supported payment methods.
    pub method_data: Vec<PaymentMethodData>,
    /// Financial details of the transaction.
    pub details: PaymentDetailsInit,
    /// Options for collecting payer information.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<PaymentOptions>,
    /// The user's provided shipping address.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shipping_address: Option<ContactAddress>,
}

/// Indicates the user has chosen a payment method and approved a payment request.
///
/// See: <https://www.w3.org/TR/payment-request/#paymentresponse-interface>
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PaymentResponse {
    /// Unique ID from the original `PaymentRequest`.
    pub request_id: String,
    /// Payment method chosen by the user.
    pub method_name: String,
    /// Payment-method-specific transaction data.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<HashMap<String, serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shipping_address: Option<ContactAddress>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shipping_option: Option<PaymentShippingOption>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payer_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payer_email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payer_phone: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payment_currency_amount_roundtrip() {
        let amt = PaymentCurrencyAmount {
            currency: "USD".into(),
            value: 120.0,
        };
        let json = serde_json::to_string(&amt).unwrap();
        let back: PaymentCurrencyAmount = serde_json::from_str(&json).unwrap();
        assert_eq!(amt, back);
    }

    #[test]
    fn payment_item_default_refund_period() {
        let json = r#"{"label":"Shoes","amount":{"currency":"USD","value":99.99}}"#;
        let item: PaymentItem = serde_json::from_str(json).unwrap();
        assert_eq!(item.refund_period, 30);
    }

    #[test]
    fn payment_request_full_roundtrip() {
        let req = PaymentRequest {
            method_data: vec![PaymentMethodData {
                supported_methods: "CARD".into(),
                data: Some(HashMap::from([(
                    "payment_processor_url".into(),
                    serde_json::Value::String("http://example.com/pay".into()),
                )])),
            }],
            details: PaymentDetailsInit {
                id: "order_123".into(),
                display_items: vec![PaymentItem {
                    label: "Cool Shoes".into(),
                    amount: PaymentCurrencyAmount {
                        currency: "USD".into(),
                        value: 120.0,
                    },
                    pending: None,
                    refund_period: 30,
                }],
                shipping_options: None,
                modifiers: None,
                total: PaymentItem {
                    label: "Total".into(),
                    amount: PaymentCurrencyAmount {
                        currency: "USD".into(),
                        value: 120.0,
                    },
                    pending: None,
                    refund_period: 30,
                },
            },
            options: Some(PaymentOptions {
                request_payer_name: Some(false),
                request_payer_email: Some(false),
                request_payer_phone: Some(false),
                request_shipping: Some(true),
                shipping_type: None,
            }),
            shipping_address: None,
        };
        let json = serde_json::to_string(&req).unwrap();
        let back: PaymentRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(req, back);
    }

    #[test]
    fn payment_response_minimal() {
        let resp = PaymentResponse {
            request_id: "order_123".into(),
            method_name: "CARD".into(),
            details: None,
            shipping_address: None,
            shipping_option: None,
            payer_name: None,
            payer_email: None,
            payer_phone: None,
        };
        let json = serde_json::to_string(&resp).unwrap();
        let back: PaymentResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(resp, back);
    }

    #[test]
    fn payment_method_data_single_string() {
        // The official SDK uses a single string, NOT an array
        let json = r#"{"supported_methods":"CARD"}"#;
        let pmd: PaymentMethodData = serde_json::from_str(json).unwrap();
        assert_eq!(pmd.supported_methods, "CARD");
    }
}
