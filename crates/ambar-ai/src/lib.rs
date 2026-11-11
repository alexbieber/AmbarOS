//! # ambar-ai
//!
//! The Ambar AI automation builder. Natural-language requests become workflow
//! drafts that the user previews, edits and approves. The model never gets
//! direct access to anything: its output is untrusted text that must pass the
//! same parsing, validation, risk review and permission consent as a
//! hand-written workflow.

pub mod config;
pub mod generate;
pub mod provider;
/// Risk review (lives in `ambar-automation`; re-exported for convenience).
pub use ambar_automation::risk;

pub use config::{AiConfig, ApiKey, ProviderKind};
pub use generate::{Draft, GenerateOptions, SuggestContext, Suggestion, generate, suggest};
pub use provider::{CompletionRequest, HttpProvider, LlmProvider, Message, ScriptedProvider};
pub use risk::{Risk, RiskLevel, review};

/// Errors from the AI builder.
#[derive(Debug, thiserror::Error)]
pub enum AiError {
    /// Configuration problem (missing key, bad URL, …).
    #[error("AI configuration: {0}")]
    Config(String),
    /// The provider call failed.
    #[error("AI provider: {0}")]
    Provider(String),
    /// No valid workflow could be produced.
    #[error("{0}")]
    Generation(String),
    /// The request needs abilities Ambar workflows don't have.
    #[error("Ambar workflows can't do that yet: {0}")]
    Unsupported(String),
}
