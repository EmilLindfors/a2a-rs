//! Helpers for embedding and extracting AP2 types in A2A protocol messages.
//!
//! AP2 mandates are carried inside `Part::Data` variants with well-known keys.
//! This module provides convenience functions for both directions.

use a2a_rs::{Artifact, Message, Part, Role};
use serde_json::{Map, Value};

use crate::error::Result;
use crate::types::{
    CartMandate, IntentMandate, PaymentMandate, PaymentReceipt, CART_MANDATE_DATA_KEY,
    INTENT_MANDATE_DATA_KEY, PAYMENT_MANDATE_DATA_KEY, PAYMENT_RECEIPT_DATA_KEY, RISK_DATA_KEY,
};

// ---------------------------------------------------------------------------
// Mandate → Part
// ---------------------------------------------------------------------------

/// Serialize an [`IntentMandate`] into a `Part::Data`.
pub fn intent_mandate_to_part(mandate: &IntentMandate) -> Result<Part> {
    let mut data = Map::new();
    data.insert(
        INTENT_MANDATE_DATA_KEY.into(),
        serde_json::to_value(mandate)?,
    );
    Ok(Part::data(data))
}

/// Serialize a [`CartMandate`] into a `Part::Data`.
pub fn cart_mandate_to_part(mandate: &CartMandate) -> Result<Part> {
    let mut data = Map::new();
    data.insert(
        CART_MANDATE_DATA_KEY.into(),
        serde_json::to_value(mandate)?,
    );
    Ok(Part::data(data))
}

/// Serialize a [`PaymentMandate`] into a `Part::Data`.
pub fn payment_mandate_to_part(mandate: &PaymentMandate) -> Result<Part> {
    let mut data = Map::new();
    data.insert(
        PAYMENT_MANDATE_DATA_KEY.into(),
        serde_json::to_value(mandate)?,
    );
    Ok(Part::data(data))
}

/// Serialize a [`PaymentReceipt`] into a `Part::Data`.
pub fn payment_receipt_to_part(receipt: &PaymentReceipt) -> Result<Part> {
    let mut data = Map::new();
    data.insert(
        PAYMENT_RECEIPT_DATA_KEY.into(),
        serde_json::to_value(receipt)?,
    );
    Ok(Part::data(data))
}

/// Create a `Part::Data` containing implementation-defined risk signals.
pub fn risk_data_to_part(risk_signals: Map<String, Value>) -> Part {
    let mut data = Map::new();
    data.insert(RISK_DATA_KEY.into(), Value::Object(risk_signals));
    Part::data(data)
}

// ---------------------------------------------------------------------------
// Part → Mandate (extract from a single part)
// ---------------------------------------------------------------------------

/// Try to extract an [`IntentMandate`] from a `Part::Data`.
///
/// Returns `Ok(None)` if the part is not a data part or does not contain the
/// intent mandate key.
pub fn extract_intent_mandate(part: &Part) -> Result<Option<IntentMandate>> {
    extract_from_part(part, INTENT_MANDATE_DATA_KEY)
}

/// Try to extract a [`CartMandate`] from a `Part::Data`.
pub fn extract_cart_mandate(part: &Part) -> Result<Option<CartMandate>> {
    extract_from_part(part, CART_MANDATE_DATA_KEY)
}

/// Try to extract a [`PaymentMandate`] from a `Part::Data`.
pub fn extract_payment_mandate(part: &Part) -> Result<Option<PaymentMandate>> {
    extract_from_part(part, PAYMENT_MANDATE_DATA_KEY)
}

/// Try to extract a [`PaymentReceipt`] from a `Part::Data`.
pub fn extract_payment_receipt(part: &Part) -> Result<Option<PaymentReceipt>> {
    extract_from_part(part, PAYMENT_RECEIPT_DATA_KEY)
}

// ---------------------------------------------------------------------------
// Message / Artifact → Mandate (search across parts)
// ---------------------------------------------------------------------------

/// Find and extract the first [`IntentMandate`] from a `Message`'s parts.
pub fn find_intent_mandate(message: &Message) -> Result<Option<IntentMandate>> {
    find_in_parts(&message.parts, INTENT_MANDATE_DATA_KEY)
}

/// Find and extract the first [`CartMandate`] from an `Artifact`'s parts.
pub fn find_cart_mandate(artifact: &Artifact) -> Result<Option<CartMandate>> {
    find_in_parts(&artifact.parts, CART_MANDATE_DATA_KEY)
}

/// Find and extract the first [`PaymentMandate`] from a `Message`'s parts.
pub fn find_payment_mandate(message: &Message) -> Result<Option<PaymentMandate>> {
    find_in_parts(&message.parts, PAYMENT_MANDATE_DATA_KEY)
}

/// Find and extract the first [`PaymentReceipt`] from a slice of parts.
pub fn find_payment_receipt_in_parts(parts: &[Part]) -> Result<Option<PaymentReceipt>> {
    find_in_parts(parts, PAYMENT_RECEIPT_DATA_KEY)
}

// ---------------------------------------------------------------------------
// Message / Artifact builders
// ---------------------------------------------------------------------------

/// Create a user `Message` containing an [`IntentMandate`].
pub fn intent_mandate_message(
    mandate: &IntentMandate,
    message_id: String,
) -> Result<Message> {
    let part = intent_mandate_to_part(mandate)?;
    Ok(Message::builder()
        .role(Role::User)
        .parts(vec![part])
        .message_id(message_id)
        .extensions(vec![crate::types::AP2_EXTENSION_URI.to_string()])
        .build())
}

/// Create an `Artifact` containing a [`CartMandate`].
pub fn cart_mandate_artifact(
    mandate: &CartMandate,
    artifact_id: String,
    name: Option<String>,
) -> Result<Artifact> {
    let part = cart_mandate_to_part(mandate)?;
    Ok(Artifact {
        artifact_id,
        name,
        description: None,
        parts: vec![part],
        metadata: None,
        extensions: Some(vec![crate::types::AP2_EXTENSION_URI.to_string()]),
    })
}

/// Create a user `Message` containing a [`PaymentMandate`].
pub fn payment_mandate_message(
    mandate: &PaymentMandate,
    message_id: String,
) -> Result<Message> {
    let part = payment_mandate_to_part(mandate)?;
    Ok(Message::builder()
        .role(Role::User)
        .parts(vec![part])
        .message_id(message_id)
        .extensions(vec![crate::types::AP2_EXTENSION_URI.to_string()])
        .build())
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn extract_from_part<T: serde::de::DeserializeOwned>(
    part: &Part,
    key: &str,
) -> Result<Option<T>> {
    match part {
        Part::Data { data, .. } => match data.get(key) {
            Some(value) => {
                let t = serde_json::from_value(value.clone())?;
                Ok(Some(t))
            }
            None => Ok(None),
        },
        _ => Ok(None),
    }
}

fn find_in_parts<T: serde::de::DeserializeOwned>(
    parts: &[Part],
    key: &str,
) -> Result<Option<T>> {
    for part in parts {
        if let Some(t) = extract_from_part(part, key)? {
            return Ok(Some(t));
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{
        CartContents, PaymentCurrencyAmount, PaymentDetailsInit, PaymentItem, PaymentMethodData,
    };

    fn sample_intent() -> IntentMandate {
        IntentMandate {
            user_cart_confirmation_required: false,
            natural_language_description: "Red shoes size 10".into(),
            merchants: None,
            skus: None,
            requires_refundability: Some(true),
            intent_expiry: "2026-12-31T23:59:59Z".into(),
        }
    }

    fn sample_cart() -> CartMandate {
        CartMandate {
            contents: CartContents {
                id: "cart_1".into(),
                user_cart_confirmation_required: false,
                payment_request: crate::types::PaymentRequest {
                    method_data: vec![PaymentMethodData {
                        supported_methods: "CARD".into(),
                        data: None,
                    }],
                    details: PaymentDetailsInit {
                        id: "order_1".into(),
                        display_items: vec![],
                        shipping_options: None,
                        modifiers: None,
                        total: PaymentItem {
                            label: "Total".into(),
                            amount: PaymentCurrencyAmount { currency: "USD".into(), value: 50.0 },
                            pending: None,
                            refund_period: 30,
                        },
                    },
                    options: None,
                    shipping_address: None,
                },
                cart_expiry: "2026-12-31T23:59:59Z".into(),
                merchant_name: "Test Store".into(),
            },
            merchant_authorization: None,
        }
    }

    #[test]
    fn roundtrip_intent_via_part() {
        let intent = sample_intent();
        let part = intent_mandate_to_part(&intent).unwrap();
        let extracted = extract_intent_mandate(&part).unwrap().unwrap();
        assert_eq!(intent, extracted);
    }

    #[test]
    fn roundtrip_cart_via_part() {
        let cart = sample_cart();
        let part = cart_mandate_to_part(&cart).unwrap();
        let extracted = extract_cart_mandate(&part).unwrap().unwrap();
        assert_eq!(cart, extracted);
    }

    #[test]
    fn find_intent_in_message() {
        let intent = sample_intent();
        let msg = intent_mandate_message(&intent, "msg-1".into()).unwrap();
        let found = find_intent_mandate(&msg).unwrap().unwrap();
        assert_eq!(intent, found);
        assert_eq!(
            msg.extensions.as_ref().unwrap()[0],
            crate::types::AP2_EXTENSION_URI
        );
    }

    #[test]
    fn find_cart_in_artifact() {
        let cart = sample_cart();
        let artifact =
            cart_mandate_artifact(&cart, "art-1".into(), Some("Cart".into())).unwrap();
        let found = find_cart_mandate(&artifact).unwrap().unwrap();
        assert_eq!(cart, found);
    }

    #[test]
    fn extract_returns_none_for_wrong_key() {
        let intent = sample_intent();
        let part = intent_mandate_to_part(&intent).unwrap();
        assert!(extract_cart_mandate(&part).unwrap().is_none());
    }

    #[test]
    fn extract_returns_none_for_text_part() {
        let part = Part::text("hello".into());
        assert!(extract_intent_mandate(&part).unwrap().is_none());
    }

    #[test]
    fn risk_data_part() {
        let mut signals = Map::new();
        signals.insert("score".into(), Value::Number(95.into()));
        let part = risk_data_to_part(signals);
        match &part {
            Part::Data { data, .. } => {
                assert!(data.contains_key(RISK_DATA_KEY));
            }
            _ => panic!("expected data part"),
        }
    }
}
