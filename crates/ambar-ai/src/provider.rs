//! LLM provider abstraction.
//!
//! Providers only turn messages into text. They have no access to the
//! engine, the filesystem or the shell.

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::AiError;
use crate::config::{AiConfig, ApiKey, ProviderKind};

/// Conversation role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// The person (or Ambar on their behalf).
    User,
    /// The model.
    Assistant,
}

/// One message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    /// Who said it.
    pub role: Role,
    /// Text.
    pub content: String,
}

impl Message {
    /// A user message.
    pub fn user(c: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: c.into(),
        }
    }
    /// An assistant message.
    pub fn assistant(c: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: c.into(),
        }
    }
}

/// A completion request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionRequest {
    /// System prompt.
    pub system: String,
    /// Conversation so far.
    pub messages: Vec<Message>,
    /// Output token limit.
    pub max_tokens: u32,
}

/// Boxed future returned by providers.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Something that completes text.
pub trait LlmProvider: Send + Sync + std::fmt::Debug {
    /// Returns the model's reply text.
    fn complete<'a>(&'a self, req: &'a CompletionRequest)
    -> BoxFuture<'a, Result<String, AiError>>;
    /// Short description for display, e.g. `anthropic/claude-sonnet-5-5`.
    fn describe(&self) -> String;
}

/// HTTP-backed provider for the three supported APIs.
#[derive(Debug)]
pub struct HttpProvider {
    kind: ProviderKind,
    base_url: String,
    model: String,
    key: Option<ApiKey>,
    http: reqwest::Client,
}

impl HttpProvider {
    /// Builds a provider from config and an (optional) key.
    pub fn new(config: &AiConfig, key: Option<ApiKey>) -> Result<Self, AiError> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(120))
            .connect_timeout(Duration::from_secs(10))
            .user_agent(concat!("ambar/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| AiError::Provider(e.to_string()))?;
        Ok(Self {
            kind: config.provider,
            base_url: config.base_url()?,
            model: config.model()?,
            key,
            http,
        })
    }

    fn body(&self, req: &CompletionRequest) -> (String, Value) {
        match self.kind {
            ProviderKind::Anthropic => (
                format!("{}/v1/messages", self.base_url),
                json!({
                    "model": self.model,
                    "max_tokens": req.max_tokens,
                    "system": req.system,
                    "messages": req.messages,
                }),
            ),
            ProviderKind::Openai | ProviderKind::Ollama => {
                let mut msgs = vec![json!({"role": "system", "content": req.system})];
                msgs.extend(req.messages.iter().map(|m| json!(m)));
                if self.kind == ProviderKind::Openai {
                    (
                        format!("{}/v1/chat/completions", self.base_url),
                        json!({ "model": self.model, "max_tokens": req.max_tokens, "messages": msgs }),
                    )
                } else {
                    (
                        format!("{}/api/chat", self.base_url),
                        json!({ "model": self.model, "stream": false, "messages": msgs,
                                "options": { "num_predict": req.max_tokens } }),
                    )
                }
            }
        }
    }

    fn extract(&self, v: &Value) -> Option<String> {
        match self.kind {
            ProviderKind::Anthropic => {
                let parts: Vec<&str> = v["content"]
                    .as_array()?
                    .iter()
                    .filter(|b| b["type"] == "text")
                    .filter_map(|b| b["text"].as_str())
                    .collect();
                (!parts.is_empty()).then(|| parts.join(""))
            }
            ProviderKind::Openai => v["choices"][0]["message"]["content"]
                .as_str()
                .map(String::from),
            ProviderKind::Ollama => v["message"]["content"].as_str().map(String::from),
        }
    }
}

impl LlmProvider for HttpProvider {
    fn complete<'a>(
        &'a self,
        req: &'a CompletionRequest,
    ) -> BoxFuture<'a, Result<String, AiError>> {
        Box::pin(async move {
            let (url, body) = self.body(req);
            let mut rb = self.http.post(&url).json(&body);
            match (self.kind, &self.key) {
                (ProviderKind::Anthropic, Some(k)) => {
                    rb = rb
                        .header("x-api-key", &k.0)
                        .header("anthropic-version", "2023-06-01");
                }
                (ProviderKind::Openai, Some(k)) => rb = rb.bearer_auth(&k.0),
                _ => {}
            }
            let resp = rb
                .send()
                .await
                .map_err(|e| AiError::Provider(format!("request to {} failed: {e}", self.kind)))?;
            let status = resp.status();
            let text = resp
                .text()
                .await
                .map_err(|e| AiError::Provider(e.to_string()))?;
            if !status.is_success() {
                // Provider error bodies describe the problem and never echo the key.
                let detail: String = serde_json::from_str::<Value>(&text)
                    .ok()
                    .and_then(|v| {
                        v["error"]["message"]
                            .as_str()
                            .or_else(|| v["error"].as_str())
                            .map(String::from)
                    })
                    .unwrap_or_else(|| text.chars().take(300).collect());
                return Err(AiError::Provider(format!(
                    "{} returned {status}: {detail}",
                    self.kind
                )));
            }
            let v: Value = serde_json::from_str(&text)
                .map_err(|e| AiError::Provider(format!("invalid JSON from {}: {e}", self.kind)))?;
            self.extract(&v)
                .ok_or_else(|| AiError::Provider(format!("no text in {} response", self.kind)))
        })
    }

    fn describe(&self) -> String {
        format!("{}/{}", self.kind, self.model)
    }
}

/// Scripted provider for tests and offline demos: returns replies in order.
#[derive(Debug, Default)]
pub struct ScriptedProvider {
    replies: std::sync::Mutex<std::collections::VecDeque<String>>,
    /// Every request received.
    pub seen: std::sync::Mutex<Vec<CompletionRequest>>,
}

impl ScriptedProvider {
    /// Provider that answers with `replies` in order.
    pub fn new(replies: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            replies: std::sync::Mutex::new(replies.into_iter().map(Into::into).collect()),
            seen: std::sync::Mutex::new(Vec::new()),
        }
    }
}

impl LlmProvider for ScriptedProvider {
    fn complete<'a>(
        &'a self,
        req: &'a CompletionRequest,
    ) -> BoxFuture<'a, Result<String, AiError>> {
        Box::pin(async move {
            if let Ok(mut s) = self.seen.lock() {
                s.push(req.clone());
            }
            self.replies
                .lock()
                .ok()
                .and_then(|mut r| r.pop_front())
                .ok_or_else(|| AiError::Provider("scripted provider ran out of replies".into()))
        })
    }
    fn describe(&self) -> String {
        "scripted".into()
    }
}
