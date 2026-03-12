/// Errors specific to the AP2 (Agent Payments Protocol) extension.
#[derive(Debug, thiserror::Error)]
pub enum Ap2Error {
    /// A required field is missing or empty.
    #[error("missing required field: {0}")]
    MissingField(String),

    /// A field value failed validation.
    #[error("validation error in `{field}`: {message}")]
    ValidationError { field: String, message: String },

    /// Failed to serialize or deserialize an AP2 type.
    #[error("serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),

    /// Attempted to extract an AP2 type from a non-data part.
    #[error("extraction error: {0}")]
    ExtractionError(String),
}

/// Convenience alias for `Result<T, Ap2Error>`.
pub type Result<T> = std::result::Result<T, Ap2Error>;
