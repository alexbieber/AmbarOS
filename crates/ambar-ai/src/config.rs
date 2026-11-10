//! Model configuration and API-key handling.
//!
//! Config lives in `$XDG_CONFIG_HOME/ambar/ai.toml` (default `~/.config/ambar/ai.toml`):
//!
//! ```toml
//! provider = "anthropic"          # anthropic | openai | ollama
//! model = "claude-sonnet-5-5"
//! # base_url = "https://api.anthropic.com"
//! # api_key_env = "ANTHROPIC_API_KEY"
//! ```
//!
//! API keys are never stored in the config file. They come from the
//! environment variable named by `api_key_env`, or from a 0600 secrets file
//! written by `ambar ai set-key`.

use std::fmt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::AiError;

/// Supported provider APIs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    /// Anthropic Messages API (Claude).
    Anthropic,
    /// Any OpenAI-compatible Chat Completions API (OpenAI, vLLM, LM Studio, …).
    Openai,
    /// Local Ollama server. No key, no data leaves the machine.
    Ollama,
}

impl ProviderKind {
    /// Default endpoint.
    pub fn default_base_url(&self) -> &'static str {
        match self {
            ProviderKind::Anthropic => "https://api.anthropic.com",
            ProviderKind::Openai => "https://api.openai.com",
            ProviderKind::Ollama => "http://localhost:11434",
        }
    }

    /// Default environment variable holding the key.
    pub fn default_key_env(&self) -> Option<&'static str> {
        match self {
            ProviderKind::Anthropic => Some("ANTHROPIC_API_KEY"),
            ProviderKind::Openai => Some("OPENAI_API_KEY"),
            ProviderKind::Ollama => None,
        }
    }

    /// Default model, where there is a sensible one.
    pub fn default_model(&self) -> Option<&'static str> {
        match self {
            ProviderKind::Anthropic => Some("claude-sonnet-5-5"),
            _ => None,
        }
    }
}

impl fmt::Display for ProviderKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            ProviderKind::Anthropic => "anthropic",
            ProviderKind::Openai => "openai",
            ProviderKind::Ollama => "ollama",
        })
    }
}

/// Contents of `ai.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AiConfig {
    /// Which API to call.
    pub provider: ProviderKind,
    /// Model name. Required unless the provider has a default.
    #[serde(default)]
    pub model: Option<String>,
    /// Endpoint override.
    #[serde(default)]
    pub base_url: Option<String>,
    /// Environment variable holding the API key.
    #[serde(default)]
    pub api_key_env: Option<String>,
    /// Max output tokens per request.
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
    /// Generation attempts (first try + repairs).
    #[serde(default = "default_attempts")]
    pub max_attempts: u32,
}

fn default_max_tokens() -> u32 {
    2048
}
fn default_attempts() -> u32 {
    3
}

impl AiConfig {
    /// A config for `provider` with defaults.
    pub fn new(provider: ProviderKind) -> Self {
        Self {
            provider,
            model: None,
            base_url: None,
            api_key_env: None,
            max_tokens: default_max_tokens(),
            max_attempts: default_attempts(),
        }
    }

    /// `$XDG_CONFIG_HOME/ambar/ai.toml` or `~/.config/ambar/ai.toml`.
    pub fn default_path() -> Option<PathBuf> {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
        Some(base.join("ambar/ai.toml"))
    }

    /// Loads a config file.
    pub fn load(path: &Path) -> Result<Self, AiError> {
        let text = std::fs::read_to_string(path).map_err(|e| {
            AiError::Config(format!(
                "{}: {e}. Set one up with `ambar ai config --provider …`",
                path.display()
            ))
        })?;
        toml::from_str(&text).map_err(|e| AiError::Config(format!("{}: {e}", path.display())))
    }

    /// Saves the config (creating parent directories).
    pub fn save(&self, path: &Path) -> Result<(), AiError> {
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p).map_err(|e| AiError::Config(e.to_string()))?;
        }
        let text = toml::to_string_pretty(self).map_err(|e| AiError::Config(e.to_string()))?;
        std::fs::write(path, text).map_err(|e| AiError::Config(e.to_string()))
    }

    /// Effective model.
    pub fn model(&self) -> Result<String, AiError> {
        self.model
            .clone()
            .or_else(|| self.provider.default_model().map(String::from))
            .ok_or_else(|| AiError::Config(format!("set `model` for provider {}", self.provider)))
    }

    /// Effective base URL, checked: remote endpoints must use HTTPS so keys
    /// and prompts are never sent in clear text.
    pub fn base_url(&self) -> Result<String, AiError> {
        let url = self
            .base_url
            .clone()
            .unwrap_or_else(|| self.provider.default_base_url().to_string());
        let url = url.trim_end_matches('/').to_string();
        if let Some(rest) = url.strip_prefix("http://") {
            let host = rest.split(['/', ':']).next().unwrap_or("");
            if !matches!(host, "localhost" | "127.0.0.1" | "[::1]") {
                return Err(AiError::Config(format!(
                    "refusing plain-HTTP endpoint {url}: use https:// (http is only allowed for localhost)"
                )));
            }
        } else if !url.starts_with("https://") {
            return Err(AiError::Config(format!("invalid base_url {url}")));
        }
        Ok(url)
    }

    /// Looks up the API key: environment variable first, then the secrets file.
    pub fn api_key(&self, secrets_file: Option<&Path>) -> Result<Option<ApiKey>, AiError> {
        let Some(default_env) = self.provider.default_key_env() else {
            return Ok(None); // provider needs no key
        };
        let env = self.api_key_env.as_deref().unwrap_or(default_env);
        if let Some(v) = std::env::var_os(env).filter(|v| !v.is_empty()) {
            return Ok(Some(ApiKey(v.to_string_lossy().trim().to_string())));
        }
        if let Some(f) = secrets_file
            && let Ok(v) = std::fs::read_to_string(f)
            && !v.trim().is_empty()
        {
            return Ok(Some(ApiKey(v.trim().to_string())));
        }
        Err(AiError::Config(format!(
            "no API key: set ${env} or run `ambar ai set-key`"
        )))
    }
}

/// An API key. `Debug` and `Display` never show it.
#[derive(Clone, PartialEq, Eq)]
pub struct ApiKey(pub(crate) String);

impl ApiKey {
    /// Wraps a key.
    pub fn new(key: impl Into<String>) -> Self {
        Self(key.into())
    }
}

impl fmt::Debug for ApiKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ApiKey(<redacted>)")
    }
}

/// Writes an API key to a 0600 file.
#[cfg(unix)]
pub fn store_key(path: &Path, key: &str) -> Result<(), AiError> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p).map_err(|e| AiError::Config(e.to_string()))?;
    }
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .map_err(|e| AiError::Config(e.to_string()))?;
    f.write_all(key.trim().as_bytes())
        .map_err(|e| AiError::Config(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_http_only_for_localhost() {
        let mut c = AiConfig::new(ProviderKind::Openai);
        c.base_url = Some("http://example.com".into());
        assert!(c.base_url().is_err());
        c.base_url = Some("http://localhost:8080/".into());
        assert_eq!(c.base_url().ok().as_deref(), Some("http://localhost:8080"));
        c.base_url = Some("http://127.0.0.1:1234".into());
        assert!(c.base_url().is_ok());
        c.base_url = Some("ftp://x".into());
        assert!(c.base_url().is_err());
        assert!(AiConfig::new(ProviderKind::Ollama).base_url().is_ok());
    }

    #[test]
    fn keys_are_redacted_and_loaded_from_file() -> Result<(), Box<dyn std::error::Error>> {
        let k = ApiKey::new("sk-secret");
        assert!(!format!("{k:?}").contains("secret"));
        let dir = tempfile::tempdir()?;
        let f = dir.path().join("key");
        store_key(&f, "sk-from-file\n")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&f)?.permissions().mode() & 0o777, 0o600);
        }
        let mut c = AiConfig::new(ProviderKind::Anthropic);
        c.api_key_env = Some("AMBAR_TEST_KEY_THAT_IS_NOT_SET".into());
        assert_eq!(c.api_key(Some(&f))?, Some(ApiKey::new("sk-from-file")));
        assert!(c.api_key(None).is_err());
        assert_eq!(AiConfig::new(ProviderKind::Ollama).api_key(None)?, None);
        Ok(())
    }

    #[test]
    fn config_round_trip_and_model_default() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let p = dir.path().join("a/ai.toml");
        let c = AiConfig::new(ProviderKind::Anthropic);
        c.save(&p)?;
        assert_eq!(AiConfig::load(&p)?, c);
        assert_eq!(c.model()?, "claude-sonnet-5-5");
        assert!(AiConfig::new(ProviderKind::Ollama).model().is_err());
        Ok(())
    }
}
