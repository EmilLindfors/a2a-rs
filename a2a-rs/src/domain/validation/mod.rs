//! Cross-cutting validation logic for the A2A protocol

use crate::domain::error::A2AError;

/// Validation result type
pub type ValidationResult<T> = Result<T, A2AError>;

/// Trait for validating domain objects
pub trait Validate {
    /// Validate the object and return a result
    fn validate(&self) -> ValidationResult<()>;
}

/// Utility functions for common validations
pub mod validators {
    use super::*;

    /// Validate that a string is not empty
    pub fn not_empty(value: &str, field_name: &str) -> ValidationResult<()> {
        if value.trim().is_empty() {
            Err(A2AError::ValidationError {
                field: field_name.to_string(),
                message: format!("{} cannot be empty", field_name),
            })
        } else {
            Ok(())
        }
    }

    /// Validate that an optional string, if present, is not empty
    pub fn optional_not_empty(value: &Option<String>, field_name: &str) -> ValidationResult<()> {
        if let Some(ref val) = value {
            not_empty(val, field_name)
        } else {
            Ok(())
        }
    }

    /// Validate UUID format
    pub fn valid_uuid(value: &str, field_name: &str) -> ValidationResult<()> {
        if uuid::Uuid::parse_str(value).is_err() {
            Err(A2AError::ValidationError {
                field: field_name.to_string(),
                message: format!("{} must be a valid UUID", field_name),
            })
        } else {
            Ok(())
        }
    }
}
