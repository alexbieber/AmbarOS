//! Natural language → workflow drafts, and automation suggestions.
//!
//! The model's reply is treated as untrusted text: it is parsed as TOML
//! (unknown fields and action types are rejected), validated, and
//! risk-reviewed. Nothing is executed or registered here; the caller shows the
//! draft to the user, who approves it through the normal permission flow.

use ambar_automation::{ValidationOptions, Workflow};
use serde::{Deserialize, Serialize};

use crate::AiError;
use crate::provider::{CompletionRequest, LlmProvider, Message};
use ambar_automation::risk::{Risk, review};

const FORMAT_SPEC: &str = include_str!("../../../docs/workflow-format.md");

fn system_prompt(home: &str) -> String {
    format!(
        "You write automation workflows for Ambar OS. Reply with exactly one TOML \
document inside a ```toml code block and nothing else.

Rules:
- Use only the triggers, conditions, actions and fields in the specification below. \
There is no action that runs programs, scripts or shell commands; never invent one. \
If the request cannot be done with these actions, reply with a TOML comment line \
starting with `# CANNOT:` explaining why, and nothing else.
- Request the narrowest permissions that work: list only the specific folders the \
actions touch, never the whole home folder, never system folders, never credential \
folders such as ~/.ssh.
- Paths must start with ~/ (the user's home is {home}) or be absolute. No `..`.
- Prefer non-destructive actions. Use trash_file only if the user explicitly asks to \
delete or clean up. Never set overwrite = true unless asked.
- id: lowercase letters, digits and dashes.

Specification:
{FORMAT_SPEC}"
    )
}

/// Options for [`generate`].
#[derive(Debug, Clone)]
pub struct GenerateOptions {
    /// First try plus repairs.
    pub max_attempts: u32,
    /// Output token limit per request.
    pub max_tokens: u32,
    /// Validation limits (should match the service's).
    pub validation: ValidationOptions,
    /// Shown to the model so it can write `~/` paths correctly.
    pub home: String,
}

impl Default for GenerateOptions {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            max_tokens: 2048,
            validation: ValidationOptions::default(),
            home: std::env::var("HOME").unwrap_or_else(|_| "~".into()),
        }
    }
}

/// A validated workflow draft awaiting the user's approval.
#[derive(Debug, Clone)]
pub struct Draft {
    /// TOML exactly as it will be saved (the model's text, after extraction).
    pub toml: String,
    /// Parsed workflow.
    pub workflow: Workflow,
    /// Requests it took (1 = first try was valid).
    pub attempts: u32,
    /// Risk review, most severe first.
    pub risks: Vec<Risk>,
}

/// Pulls the TOML out of a model reply.
pub fn extract_toml(reply: &str) -> Option<String> {
    let fence = |tag: &str| -> Option<String> {
        let start = reply.find(tag)? + tag.len();
        let rest = &reply[start..];
        let rest = rest.strip_prefix('\n').unwrap_or(rest);
        let end = rest.find("```")?;
        Some(rest[..end].trim().to_string())
    };
    fence("```toml").or_else(|| fence("```")).or_else(|| {
        let t = reply.trim();
        (t.contains("[trigger]") || t.starts_with("# CANNOT:")).then(|| t.to_string())
    })
}

/// Turns a description into a validated draft, asking the model to fix its
/// own mistakes up to `max_attempts` times.
pub async fn generate(
    provider: &dyn LlmProvider,
    description: &str,
    opts: &GenerateOptions,
) -> Result<Draft, AiError> {
    let description = description.trim();
    if description.is_empty() {
        return Err(AiError::Generation(
            "describe what the automation should do".into(),
        ));
    }
    let mut req = CompletionRequest {
        system: system_prompt(&opts.home),
        messages: vec![Message::user(description)],
        max_tokens: opts.max_tokens,
    };
    let mut last_problems = String::new();
    for attempt in 1..=opts.max_attempts.max(1) {
        let reply = provider.complete(&req).await?;
        let problems = match extract_toml(&reply) {
            None => "Your reply did not contain a ```toml code block.".to_string(),
            Some(t) if t.starts_with("# CANNOT:") => {
                return Err(AiError::Unsupported(
                    t.trim_start_matches("# CANNOT:").trim().to_string(),
                ));
            }
            Some(toml) => match Workflow::from_toml(&toml) {
                Err(e) => format!("The TOML could not be parsed: {e}"),
                Ok(wf) => match wf.validate(&opts.validation) {
                    Ok(()) => {
                        let risks = review(&wf);
                        return Ok(Draft {
                            toml,
                            workflow: wf,
                            attempts: attempt,
                            risks,
                        });
                    }
                    Err(errs) => {
                        let list: Vec<String> = errs.iter().map(|e| format!("- {e}")).collect();
                        format!("The workflow is invalid:\n{}", list.join("\n"))
                    }
                },
            },
        };
        tracing::debug!(attempt, %problems, "draft rejected; asking for a fix");
        last_problems = problems.clone();
        req.messages.push(Message::assistant(reply));
        req.messages.push(Message::user(format!(
            "{problems}\nFix these problems and reply with the corrected TOML only."
        )));
    }
    Err(AiError::Generation(format!(
        "the model did not produce a valid workflow after {} attempts. Last problems:\n{last_problems}",
        opts.max_attempts.max(1)
    )))
}

/// An automation idea.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Suggestion {
    /// Short title.
    pub title: String,
    /// A description that can be passed straight to [`generate`].
    pub description: String,
}

/// What [`suggest`] tells the model. Only these fields leave the machine:
/// workflow ids/names/triggers, run counts, and the user's own goal text.
#[derive(Debug, Clone, Default, Serialize)]
pub struct SuggestContext {
    /// Existing workflows: `(name, trigger kind, runs, failures)`.
    pub workflows: Vec<(String, String, usize, usize)>,
    /// Optional goal in the user's words.
    pub goal: Option<String>,
}

/// Asks the model for automation ideas.
pub async fn suggest(
    provider: &dyn LlmProvider,
    ctx: &SuggestContext,
    max_tokens: u32,
) -> Result<Vec<Suggestion>, AiError> {
    let system = "You suggest useful personal-computer automations for Ambar OS. \
Ambar workflows can: watch a folder, run on a schedule, or run manually; check file \
extension, name or size; create folders, copy, move, write or append text files, move \
files to a recoverable trash, and show notifications. They cannot run programs, read \
email or browse the web. Suggest 3 to 5 automations that fit these abilities and do \
not duplicate the existing ones. Reply with only a JSON array of objects with \
\"title\" and \"description\" (one sentence the user could ask for verbatim)."
        .to_string();
    let mut user = String::from("Existing workflows:\n");
    if ctx.workflows.is_empty() {
        user.push_str("(none)\n");
    }
    for (name, trigger, runs, failures) in &ctx.workflows {
        user.push_str(&format!(
            "- {name} ({trigger}; {runs} runs, {failures} failed)\n"
        ));
    }
    if let Some(g) = &ctx.goal {
        user.push_str(&format!("\nThe user's goal: {g}\n"));
    }
    let reply = provider
        .complete(&CompletionRequest {
            system,
            messages: vec![Message::user(user)],
            max_tokens,
        })
        .await?;
    parse_suggestions(&reply)
}

/// Parses a JSON array of suggestions, tolerating a surrounding code fence.
pub fn parse_suggestions(reply: &str) -> Result<Vec<Suggestion>, AiError> {
    let start = reply.find('[');
    let end = reply.rfind(']');
    let (Some(s), Some(e)) = (start, end) else {
        return Err(AiError::Generation(
            "the model did not return a list of suggestions".into(),
        ));
    };
    let list: Vec<Suggestion> = serde_json::from_str(&reply[s..=e])
        .map_err(|e| AiError::Generation(format!("could not read suggestions: {e}")))?;
    Ok(list
        .into_iter()
        .filter(|s| !s.title.trim().is_empty() && !s.description.trim().is_empty())
        .take(10)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_toml_variants() {
        assert_eq!(
            extract_toml("hi\n```toml\na = 1\n```\nbye").as_deref(),
            Some("a = 1")
        );
        assert_eq!(extract_toml("```\nb = 2\n```").as_deref(), Some("b = 2"));
        assert!(extract_toml("id = 'x'\n[trigger]\ntype='manual'").is_some());
        assert!(extract_toml("Sure! I can help with that.").is_none());
    }

    #[test]
    fn parses_suggestions() -> Result<(), AiError> {
        let s = parse_suggestions(
            "```json\n[{\"title\":\"A\",\"description\":\"Do a\"},{\"title\":\"\",\"description\":\"x\"}]\n```",
        )?;
        assert_eq!(s.len(), 1);
        assert!(parse_suggestions("no").is_err());
        Ok(())
    }

    #[test]
    fn prompt_forbids_shell_and_includes_spec() {
        let p = system_prompt("/home/a");
        assert!(p.contains("never invent one"));
        assert!(p.contains("move_file"));
        assert!(p.contains("/home/a"));
    }
}
