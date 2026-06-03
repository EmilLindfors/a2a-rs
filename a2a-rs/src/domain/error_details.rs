//! Typed error details, surfaced in the JSON-RPC `error.data` array.
//!
//! The A2A spec (following the Go/C#/Python SDKs) carries machine-readable error
//! details as a list of Google-RPC `Any`-shaped objects in `error.data`. Each
//! entry is tagged by an `@type` URL (`type.googleapis.com/google.rpc.*`). We
//! model the two A2A actually uses — [`ErrorDetail::BadRequest`] for field-level
//! validation failures and [`ErrorDetail::ErrorInfo`] for a stable machine
//! `reason` code — as plain serde types so the adapter layer can attach them
//! without hand-writing JSON, and the client can round-trip them back.
//!
//! These are pure domain value objects: no I/O, no framework types. The
//! [`A2AError`](crate::domain::A2AError) ⇄ wire mapping lives in the transport
//! adapter; [`A2AError::error_details`](crate::domain::A2AError::error_details)
//! derives the default detail set for each variant.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// A single field-level validation failure
/// (mirrors `google.rpc.BadRequest.FieldViolation`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldViolation {
    /// Path to the offending field (e.g. `"history_length"`).
    pub field: String,
    /// Human-readable explanation of why the field is invalid.
    pub description: String,
}

impl FieldViolation {
    /// Construct a field violation from any string-likes.
    pub fn new(field: impl Into<String>, description: impl Into<String>) -> Self {
        Self { field: field.into(), description: description.into() }
    }
}

/// Stable, machine-readable error metadata (mirrors `google.rpc.ErrorInfo`).
///
/// `reason` is a `SCREAMING_SNAKE_CASE` constant identifying the failure (e.g.
/// `"TASK_NOT_FOUND"`), `domain` scopes the reason namespace, and `metadata`
/// carries arbitrary key/value context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorInfo {
    /// Stable reason code, unique within `domain`.
    pub reason: String,
    /// Logical owner of the reason namespace (always `"a2a-rs"` here).
    pub domain: String,
    /// Additional structured context; omitted from the wire when empty.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, String>,
}

impl ErrorInfo {
    /// Construct an `ErrorInfo` in the `a2a-rs` domain with no metadata.
    pub fn new(reason: impl Into<String>) -> Self {
        Self { reason: reason.into(), domain: DOMAIN.to_string(), metadata: BTreeMap::new() }
    }

    /// Attach a metadata key/value pair (builder-style).
    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }
}

/// The `domain` namespace for every [`ErrorInfo`] this crate emits.
pub const DOMAIN: &str = "a2a-rs";

/// One typed entry of the JSON-RPC `error.data` array.
///
/// Serializes as a Google-RPC `Any`: an `@type` discriminator plus the payload
/// fields inline (`{"@type": "…/google.rpc.ErrorInfo", "reason": …}`), exactly
/// the shape the official SDKs read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "@type")]
pub enum ErrorDetail {
    /// Field-level validation failures.
    #[serde(rename = "type.googleapis.com/google.rpc.BadRequest")]
    BadRequest {
        /// The set of violated fields.
        #[serde(rename = "fieldViolations")]
        field_violations: Vec<FieldViolation>,
    },
    /// A stable machine-readable reason code.
    #[serde(rename = "type.googleapis.com/google.rpc.ErrorInfo")]
    ErrorInfo(ErrorInfo),
}

impl ErrorDetail {
    /// Convenience constructor for a single-field `BadRequest`.
    pub fn bad_request(field: impl Into<String>, description: impl Into<String>) -> Self {
        Self::BadRequest { field_violations: vec![FieldViolation::new(field, description)] }
    }

    /// Convenience constructor for an `ErrorInfo` reason in the `a2a-rs` domain.
    pub fn reason(reason: impl Into<String>) -> Self {
        Self::ErrorInfo(ErrorInfo::new(reason))
    }
}
