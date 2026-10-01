use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    InvalidInput,
    NotFound,
    InvalidState,
    ProjectBoundaryViolation,
    StorageUnavailable,
    Internal,
}

#[derive(Debug, Error)]
#[error("{message}")]
pub struct DomainError {
    pub code: ErrorCode,
    pub message: String,
    pub retryable: bool,
    pub field: Option<String>,
}

impl DomainError {
    pub fn invalid_input(message: impl Into<String>, field: impl Into<String>) -> Self {
        Self {
            code: ErrorCode::InvalidInput,
            message: message.into(),
            retryable: false,
            field: Some(field.into()),
        }
    }

    pub fn storage(message: impl Into<String>) -> Self {
        Self {
            code: ErrorCode::StorageUnavailable,
            message: message.into(),
            retryable: true,
            field: None,
        }
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self {
            code: ErrorCode::Internal,
            message: message.into(),
            retryable: false,
            field: None,
        }
    }
}

impl Serialize for DomainError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("DomainError", 4)?;
        state.serialize_field("code", &self.code)?;
        state.serialize_field("message", &self.message)?;
        state.serialize_field("retryable", &self.retryable)?;
        state.serialize_field("field", &self.field)?;
        state.end()
    }
}
