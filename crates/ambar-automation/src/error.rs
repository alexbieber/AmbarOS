//! Error types for the automation engine.

use std::fmt;

use crate::permissions::PermissionError;

/// A single problem found while validating a workflow definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationError {
    /// Dotted path to the offending field, e.g. `actions[1].to_dir`.
    pub field: String,
    /// Human-readable description of the problem.
    pub message: String,
}

impl ValidationError {
    pub(crate) fn new(field: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            message: message.into(),
        }
    }
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.field, self.message)
    }
}

fn join_errors(errors: &[ValidationError]) -> String {
    errors
        .iter()
        .map(|e| format!("  - {e}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Errors returned by the engine's public API.
#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    /// The workflow definition failed validation.
    #[error("workflow is invalid:\n{}", join_errors(.0))]
    Invalid(Vec<ValidationError>),

    /// The workflow file could not be parsed.
    #[error("could not parse workflow: {0}")]
    Parse(String),

    /// No workflow with this id is registered.
    #[error("workflow `{0}` is not registered")]
    NotFound(String),

    /// A permission check failed.
    #[error(transparent)]
    Permission(#[from] PermissionError),

    /// The requested operation is not allowed in the workflow's current state.
    #[error("{0}")]
    State(String),

    /// A trigger could not be started.
    #[error("trigger error: {0}")]
    Trigger(String),

    /// The history database returned an error.
    #[error("storage error: {0}")]
    Storage(#[from] rusqlite::Error),

    /// A filesystem error outside of an action.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

/// Convenience alias.
pub type Result<T, E = EngineError> = std::result::Result<T, E>;
