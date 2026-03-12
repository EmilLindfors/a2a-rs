//! AP2 mandate types: IntentMandate, CartMandate, and PaymentMandate.

use serde::{Deserialize, Serialize};

use super::payment_request::{PaymentItem, PaymentRequest, PaymentResponse};

/// A user's purchase intent with constraints and requirements.
///
/// Created by a shopping agent based on user input and sent to a merchant
/// agent via an A2A `Message` with data key
/// [`INTENT_MANDATE_DATA_KEY`](super::roles::INTENT_MANDATE_DATA_KEY).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IntentMandate {
    /// If `false`, the agent can make purchases without further user approval
    /// once all purchase conditions are satisfied. Must be `true` if the
    /// mandate is not signed by the user.
    #[serde(default = "default_true")]
    pub user_cart_confirmation_required: bool,

    /// Natural-language description of the user's intent, confirmed by the
    /// user. Required.
    pub natural_language_description: String,

    /// Merchants allowed to fulfill the intent. `None` means any merchant.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub merchants: Option<Vec<String>>,

    /// Specific product SKUs. `None` means any SKU.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skus: Option<Vec<String>>,

    /// If `true`, purchased items must be refundable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requires_refundability: Option<bool>,

    /// When this intent expires, in ISO 8601 format.
    pub intent_expiry: String,
}

fn default_true() -> bool {
    true
}

/// The detailed contents of a cart, signed by the merchant.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CartContents {
    /// Unique identifier for this cart.
    pub id: String,

    /// Whether the merchant requires user confirmation before purchase.
    pub user_cart_confirmation_required: bool,

    /// W3C `PaymentRequest` containing items, prices, and accepted methods.
    pub payment_request: PaymentRequest,

    /// When this cart expires, in ISO 8601 format.
    pub cart_expiry: String,

    /// Name of the merchant.
    pub merchant_name: String,
}

/// A cart whose contents have been digitally signed by the merchant.
///
/// Returned by a merchant agent as an A2A `Artifact` with data key
/// [`CART_MANDATE_DATA_KEY`](super::roles::CART_MANDATE_DATA_KEY).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CartMandate {
    /// Cart details and payment information.
    pub contents: CartContents,

    /// Base64url-encoded JWT digitally signing the cart contents with the
    /// merchant's private key.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub merchant_authorization: Option<String>,
}

/// Core payment authorization data inside a [`PaymentMandate`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PaymentMandateContents {
    /// Unique identifier for this payment mandate.
    pub payment_mandate_id: String,

    /// Reference to the payment request identifier.
    pub payment_details_id: String,

    /// Total amount being authorized.
    pub payment_details_total: PaymentItem,

    /// The user's chosen payment method and details.
    pub payment_response: PaymentResponse,

    /// Identifier of the merchant agent.
    pub merchant_agent: String,

    /// Creation timestamp in ISO 8601 format.
    pub timestamp: String,
}

/// User's instructions and authorization for payment.
///
/// Sent via an A2A `Message` with data key
/// [`PAYMENT_MANDATE_DATA_KEY`](super::roles::PAYMENT_MANDATE_DATA_KEY).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PaymentMandate {
    /// Core payment authorization data.
    pub payment_mandate_contents: PaymentMandateContents,

    /// Base64url-encoded verifiable credential presentation binding the user
    /// to the cart and payment mandate hashes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_authorization: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::payment_request::{
        PaymentCurrencyAmount, PaymentDetailsInit, PaymentMethodData,
    };

    fn sample_payment_request() -> PaymentRequest {
        PaymentRequest {
            method_data: vec![PaymentMethodData {
                supported_methods: "CARD".into(),
                data: None,
            }],
            details: PaymentDetailsInit {
                id: "order_123".into(),
                display_items: vec![PaymentItem {
                    label: "Shoes".into(),
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
            options: None,
            shipping_address: None,
        }
    }

    #[test]
    fn intent_mandate_default_confirmation() {
        let json = r#"{
            "natural_language_description": "Buy red shoes",
            "intent_expiry": "2026-12-31T23:59:59Z"
        }"#;
        let im: IntentMandate = serde_json::from_str(json).unwrap();
        assert!(im.user_cart_confirmation_required);
    }

    #[test]
    fn intent_mandate_roundtrip() {
        let im = IntentMandate {
            user_cart_confirmation_required: false,
            natural_language_description: "Cool red shoes".into(),
            merchants: Some(vec!["nike".into()]),
            skus: None,
            requires_refundability: Some(true),
            intent_expiry: "2026-09-16T15:00:00Z".into(),
        };
        let json = serde_json::to_string(&im).unwrap();
        let back: IntentMandate = serde_json::from_str(&json).unwrap();
        assert_eq!(im, back);
    }

    #[test]
    fn cart_mandate_roundtrip() {
        let cm = CartMandate {
            contents: CartContents {
                id: "cart_123".into(),
                user_cart_confirmation_required: false,
                payment_request: sample_payment_request(),
                cart_expiry: "2026-12-31T23:59:59Z".into(),
                merchant_name: "Cool Shoe Store".into(),
            },
            merchant_authorization: Some("eyJhbGci...".into()),
        };
        let json = serde_json::to_string(&cm).unwrap();
        let back: CartMandate = serde_json::from_str(&json).unwrap();
        assert_eq!(cm, back);
    }

    #[test]
    fn payment_mandate_roundtrip() {
        let pm = PaymentMandate {
            payment_mandate_contents: PaymentMandateContents {
                payment_mandate_id: "pm_123".into(),
                payment_details_id: "order_123".into(),
                payment_details_total: PaymentItem {
                    label: "Total".into(),
                    amount: PaymentCurrencyAmount {
                        currency: "USD".into(),
                        value: 120.0,
                    },
                    pending: None,
                    refund_period: 30,
                },
                payment_response: crate::types::payment_request::PaymentResponse {
                    request_id: "order_123".into(),
                    method_name: "CARD".into(),
                    details: None,
                    shipping_address: None,
                    shipping_option: None,
                    payer_name: None,
                    payer_email: None,
                    payer_phone: None,
                },
                merchant_agent: "MerchantAgent".into(),
                timestamp: "2025-08-26T19:36:36Z".into(),
            },
            user_authorization: Some("eyJhbGci...".into()),
        };
        let json = serde_json::to_string(&pm).unwrap();
        let back: PaymentMandate = serde_json::from_str(&json).unwrap();
        assert_eq!(pm, back);
    }

    #[test]
    fn compat_with_python_sdk_intent_message() {
        // JSON matching the official A2A extension doc example
        let json = r#"{
            "user_cart_confirmation_required": false,
            "natural_language_description": "I'd like some cool red shoes in my size",
            "merchants": null,
            "skus": null,
            "required_refundability": true,
            "intent_expiry": "2025-09-16T15:00:00Z"
        }"#;
        // Note: the Python SDK uses `requires_refundability` but the A2A
        // extension doc example uses `required_refundability`. We accept
        // our canonical field name `requires_refundability`; the extra field
        // is silently ignored by serde.
        let _im: IntentMandate = serde_json::from_str(json).unwrap();
    }

    #[test]
    fn compat_with_python_sdk_cart_artifact() {
        let json = r#"{
            "contents": {
                "id": "cart_shoes_123",
                "user_cart_confirmation_required": false,
                "user_signature_required": false,
                "payment_request": {
                    "method_data": [{"supported_methods": "CARD", "data": {"payment_processor_url": "http://example.com/pay"}}],
                    "details": {
                        "id": "order_shoes_123",
                        "display_items": [{"label": "Cool Shoes Max", "amount": {"currency": "USD", "value": 120.0}}],
                        "total": {"label": "Total", "amount": {"currency": "USD", "value": 120.0}}
                    },
                    "options": {
                        "request_payer_name": false,
                        "request_payer_email": false,
                        "request_payer_phone": false,
                        "request_shipping": true
                    }
                },
                "cart_expiry": "2026-12-31T23:59:59Z",
                "merchant_name": "Cool Shoe Store"
            },
            "merchant_authorization": "sig_merchant_shoes_abc1"
        }"#;
        let cm: CartMandate = serde_json::from_str(json).unwrap();
        assert_eq!(cm.contents.id, "cart_shoes_123");
    }
}
