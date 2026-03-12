//! # a2a-ap2 — Agent Payments Protocol extension for A2A
//!
//! This crate implements the [AP2 (Agent Payments Protocol) v0.1](https://ap2-protocol.org/)
//! as a companion library to [`a2a-rs`]. It provides:
//!
//! - Strongly-typed Rust models for all AP2 types (mandates, payment requests,
//!   receipts, roles)
//! - Helpers for embedding AP2 data into A2A `Message` and `Artifact` parts
//! - Helpers for extracting AP2 data from A2A parts
//! - `AgentExtension` builders for declaring AP2 support in `AgentCard`s
//! - Validation for all AP2 types
//!
//! ## Quick Start
//!
//! ```rust
//! use a2a_ap2::{
//!     IntentMandate, intent_mandate_message,
//!     ap2_extension, Ap2Role,
//! };
//!
//! // Declare AP2 support in an agent card
//! let ext = ap2_extension(vec![Ap2Role::Shopper], false);
//!
//! // Create an intent mandate and wrap it in an A2A message
//! let intent = IntentMandate {
//!     user_cart_confirmation_required: true,
//!     natural_language_description: "Buy red shoes under $100".into(),
//!     merchants: None,
//!     skus: None,
//!     requires_refundability: Some(true),
//!     intent_expiry: "2026-12-31T23:59:59Z".into(),
//! };
//! let message = intent_mandate_message(&intent, "msg-1".into()).unwrap();
//! ```

pub mod error;
pub mod extension;
pub mod helpers;
pub mod types;
pub mod validation;

// Re-export types at crate root for convenience.
pub use error::{Ap2Error, Result};

pub use types::{
    Ap2Role, CartContents, CartMandate, ContactAddress, IntentMandate, PaymentCurrencyAmount,
    PaymentDetailsInit, PaymentDetailsModifier, PaymentItem, PaymentMandate,
    PaymentMandateContents, PaymentMethodData, PaymentOptions, PaymentReceipt, PaymentRequest,
    PaymentResponse, PaymentShippingOption, PaymentStatus,
    // Receipt variant structs
    Error, Failure, Success,
    // Constants
    AP2_EXTENSION_URI, CART_MANDATE_DATA_KEY, INTENT_MANDATE_DATA_KEY,
    PAYMENT_MANDATE_DATA_KEY, PAYMENT_RECEIPT_DATA_KEY, RISK_DATA_KEY,
};

pub use helpers::{
    cart_mandate_artifact, cart_mandate_to_part, extract_cart_mandate, extract_intent_mandate,
    extract_payment_mandate, extract_payment_receipt, find_cart_mandate, find_intent_mandate,
    find_payment_mandate, find_payment_receipt_in_parts, intent_mandate_message,
    intent_mandate_to_part, payment_mandate_message, payment_mandate_to_part,
    payment_receipt_to_part, risk_data_to_part,
};

pub use extension::{ap2_extension, get_ap2_roles, has_ap2_role, supports_ap2, with_ap2};

pub use validation::Validate;
