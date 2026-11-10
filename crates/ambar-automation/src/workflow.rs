//! Workflow definitions and validation.
//!
//! Workflows are written in TOML. See `docs/workflow-format.md` and the
//! `examples/` directory for the full format.

use std::path::{Path, PathBuf};
use std::time::Duration;

use globset::Glob;
use serde::{Deserialize, Serialize};

use crate::error::{EngineError, Result, ValidationError};
use crate::permissions::{Access, PermissionRequest, lexical_path};
use crate::template::{FILE_VARS, GLOBAL_VARS, referenced_vars};

/// Maximum number of actions in one workflow.
pub const MAX_ACTIONS: usize = 50;

/// A complete workflow definition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Workflow {
    /// Stable identifier: lowercase letters, digits and `-`, max 64 chars.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Optional description.
    #[serde(default)]
    pub description: String,
    /// What starts the workflow.
    pub trigger: Trigger,
    /// All conditions must hold for the actions to run.
    #[serde(default)]
    pub conditions: Vec<Condition>,
    /// Steps executed in order. The first failure stops the run.
    pub actions: Vec<Action>,
    /// Access the workflow asks the user for.
    #[serde(default)]
    pub permissions: PermissionRequest,
}

/// What starts a workflow.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Trigger {
    /// Only runs when started explicitly.
    Manual,
    /// Runs on a fixed interval (`every = "1h"`) or a cron expression
    /// (`cron = "0 9 * * 1-5"`, local time). Exactly one must be set.
    Schedule {
        /// Interval in humantime format, e.g. `30s`, `15m`, `1h`.
        #[serde(default)]
        every: Option<String>,
        /// Five-field cron expression.
        #[serde(default)]
        cron: Option<String>,
    },
    /// Runs when files change in a folder.
    FolderWatch {
        /// Folder to watch.
        path: String,
        /// Which events fire the workflow. Default: `["created"]`.
        #[serde(default = "default_events")]
        events: Vec<FileEventKind>,
        /// Optional glob matched against the file name, e.g. `*.pdf`.
        #[serde(default)]
        pattern: Option<String>,
        /// Watch subfolders too. Default: false.
        #[serde(default)]
        recursive: bool,
    },
}

fn default_events() -> Vec<FileEventKind> {
    vec![FileEventKind::Created]
}

/// File-system event kinds a folder trigger can react to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileEventKind {
    /// A file appeared (created or moved in).
    Created,
    /// A file's contents changed.
    Modified,
    /// A file was removed or moved out.
    Removed,
}

impl FileEventKind {
    /// Lowercase name, as used in templates and history.
    pub fn as_str(&self) -> &'static str {
        match self {
            FileEventKind::Created => "created",
            FileEventKind::Modified => "modified",
            FileEventKind::Removed => "removed",
        }
    }
}

/// A guard evaluated against the triggering file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Condition {
    /// File extension (case-insensitive, without the dot) is one of these.
    FileExtension {
        /// Allowed extensions, e.g. `["pdf", "png"]`.
        any_of: Vec<String>,
    },
    /// File name matches a glob.
    FileNameMatches {
        /// Glob pattern, e.g. `invoice-*`.
        glob: String,
    },
    /// File size is within bounds (inclusive).
    FileSize {
        /// Minimum size in bytes.
        #[serde(default)]
        min_bytes: Option<u64>,
        /// Maximum size in bytes.
        #[serde(default)]
        max_bytes: Option<u64>,
    },
}

fn default_from() -> String {
    "{file}".to_string()
}

/// One step of a workflow. Paths may use `{variables}`; see `template`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    /// Create a directory (and parents) if missing.
    CreateDirectory {
        /// Directory to create.
        path: String,
    },
    /// Copy a file into a folder, keeping its name.
    CopyFile {
        /// Source file. Default: the triggering file.
        #[serde(default = "default_from")]
        from: String,
        /// Destination folder (created if missing).
        to_dir: String,
        /// Replace an existing file. Sensitive.
        #[serde(default)]
        overwrite: bool,
    },
    /// Move a file into a folder, keeping its name.
    MoveFile {
        /// Source file. Default: the triggering file.
        #[serde(default = "default_from")]
        from: String,
        /// Destination folder (created if missing).
        to_dir: String,
        /// Replace an existing file. Sensitive.
        #[serde(default)]
        overwrite: bool,
    },
    /// Write or append text to a file.
    WriteFile {
        /// Target file.
        path: String,
        /// Content (templated).
        content: String,
        /// Append instead of replacing. Replacing an existing file is sensitive.
        #[serde(default)]
        append: bool,
    },
    /// Move a file to the Ambar trash (recoverable). Always sensitive.
    TrashFile {
        /// File to trash. Default: the triggering file.
        #[serde(default = "default_from")]
        path: String,
    },
    /// Send a desktop notification.
    Notify {
        /// Title (templated).
        title: String,
        /// Body (templated).
        #[serde(default)]
        body: String,
    },
}

/// A permission an action needs, expressed over a templated path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Requirement {
    /// Kind of access.
    pub access: Access,
    /// Templated path, `None` for notifications.
    pub path: Option<String>,
}

impl Action {
    /// Short machine name, as stored in history.
    pub fn kind(&self) -> &'static str {
        match self {
            Action::CreateDirectory { .. } => "create_directory",
            Action::CopyFile { .. } => "copy_file",
            Action::MoveFile { .. } => "move_file",
            Action::WriteFile { .. } => "write_file",
            Action::TrashFile { .. } => "trash_file",
            Action::Notify { .. } => "notify",
        }
    }

    /// Whether the action can destroy or replace data and therefore needs
    /// explicit approval. `WriteFile` without `append` is treated as sensitive
    /// only if the target exists at run time (checked by the executor).
    pub fn is_always_sensitive(&self) -> bool {
        match self {
            Action::TrashFile { .. } => true,
            Action::CopyFile { overwrite, .. } | Action::MoveFile { overwrite, .. } => *overwrite,
            _ => false,
        }
    }

    /// Permissions the action needs.
    pub fn requirements(&self) -> Vec<Requirement> {
        let r = |access, path: &str| Requirement {
            access,
            path: Some(path.to_string()),
        };
        match self {
            Action::CreateDirectory { path } => vec![r(Access::Write, path)],
            Action::CopyFile { from, to_dir, .. } => {
                vec![r(Access::Read, from), r(Access::Write, to_dir)]
            }
            Action::MoveFile { from, to_dir, .. } => {
                vec![r(Access::Write, from), r(Access::Write, to_dir)]
            }
            Action::WriteFile { path, .. } => vec![r(Access::Write, path)],
            Action::TrashFile { path } => vec![r(Access::Write, path)],
            Action::Notify { .. } => vec![Requirement {
                access: Access::Notify,
                path: None,
            }],
        }
    }

    fn templates(&self) -> Vec<(&'static str, &str)> {
        match self {
            Action::CreateDirectory { path } => vec![("path", path)],
            Action::CopyFile { from, to_dir, .. } | Action::MoveFile { from, to_dir, .. } => {
                vec![("from", from), ("to_dir", to_dir)]
            }
            Action::WriteFile { path, content, .. } => vec![("path", path), ("content", content)],
            Action::TrashFile { path } => vec![("path", path)],
            Action::Notify { title, body } => vec![("title", title), ("body", body)],
        }
    }
}

impl Trigger {
    /// Whether file variables (`{file}` etc.) are available.
    pub fn provides_file(&self) -> bool {
        matches!(self, Trigger::FolderWatch { .. })
    }

    /// Short machine name.
    pub fn kind(&self) -> &'static str {
        match self {
            Trigger::Manual => "manual",
            Trigger::Schedule { .. } => "schedule",
            Trigger::FolderWatch { .. } => "folder_watch",
        }
    }
}

/// Parsed schedule.
#[derive(Debug, Clone)]
pub enum Schedule {
    /// Fixed interval.
    Every(Duration),
    /// Cron expression (local time).
    Cron(Box<croner::Cron>),
}

/// Parses a schedule trigger's fields.
pub fn parse_schedule(every: Option<&str>, cron: Option<&str>) -> Result<Schedule, String> {
    match (every, cron) {
        (Some(e), None) => humantime::parse_duration(e)
            .map(Schedule::Every)
            .map_err(|err| format!("invalid interval `{e}`: {err}")),
        (None, Some(c)) => c
            .parse::<croner::Cron>()
            .map(|c| Schedule::Cron(Box::new(c)))
            .map_err(|err| format!("invalid cron expression `{c}`: {err}")),
        (Some(_), Some(_)) => Err("set either `every` or `cron`, not both".into()),
        (None, None) => Err("a schedule needs `every` or `cron`".into()),
    }
}

/// Knobs for validation.
#[derive(Debug, Clone)]
pub struct ValidationOptions {
    /// Shortest allowed schedule interval.
    pub min_interval: Duration,
}

impl Default for ValidationOptions {
    fn default() -> Self {
        Self {
            min_interval: Duration::from_secs(10),
        }
    }
}

impl Workflow {
    /// Parses a TOML workflow definition (does not validate).
    pub fn from_toml(source: &str) -> Result<Self> {
        toml::from_str(source).map_err(|e| EngineError::Parse(e.to_string()))
    }

    /// Reads and parses a workflow file (does not validate).
    pub fn from_file(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)?;
        Self::from_toml(&text)
    }

    /// Serializes back to TOML.
    pub fn to_toml(&self) -> Result<String> {
        toml::to_string_pretty(self).map_err(|e| EngineError::Parse(e.to_string()))
    }

    /// Validates the definition. Returns every problem found, not just the first.
    pub fn validate(&self, opts: &ValidationOptions) -> Result<(), Vec<ValidationError>> {
        let mut errs = Vec::new();
        let mut err = |f: String, m: String| errs.push(ValidationError::new(f, m));

        // Identity.
        if self.id.is_empty()
            || self.id.len() > 64
            || !self
                .id
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
            || self.id.starts_with('-')
        {
            err(
                "id".into(),
                "must be 1-64 chars of lowercase letters, digits and `-`".into(),
            );
        }
        if self.name.trim().is_empty() {
            err("name".into(), "must not be empty".into());
        }

        // Permission request paths must be safe.
        let mut req_read = Vec::new();
        let mut req_write = Vec::new();
        for (list, out, label) in [
            (&self.permissions.read, &mut req_read, "read"),
            (&self.permissions.write, &mut req_write, "write"),
        ] {
            for (i, p) in list.iter().enumerate() {
                match lexical_path(p) {
                    Ok(path) => out.push(path),
                    Err(e) => err(format!("permissions.{label}[{i}]"), e.to_string()),
                }
            }
        }

        // Trigger.
        let mut watched: Option<PathBuf> = None;
        match &self.trigger {
            Trigger::Manual => {}
            Trigger::Schedule { every, cron } => {
                match parse_schedule(every.as_deref(), cron.as_deref()) {
                    Ok(Schedule::Every(d)) if d < opts.min_interval => err(
                        "trigger.every".into(),
                        format!(
                            "interval must be at least {}",
                            humantime::format_duration(opts.min_interval)
                        ),
                    ),
                    Ok(_) => {}
                    Err(m) => err("trigger".into(), m),
                }
            }
            Trigger::FolderWatch {
                path,
                events,
                pattern,
                ..
            } => {
                match lexical_path(path) {
                    Ok(p) => {
                        if !covered(&p, &req_read) && !covered(&p, &req_write) {
                            err(
                                "trigger.path".into(),
                                "watched folder must be listed in permissions.read or permissions.write".into(),
                            );
                        }
                        watched = Some(p);
                    }
                    Err(e) => err("trigger.path".into(), e.to_string()),
                }
                if events.is_empty() {
                    err(
                        "trigger.events".into(),
                        "must list at least one event".into(),
                    );
                }
                if let Some(g) = pattern
                    && let Err(e) = Glob::new(g)
                {
                    err("trigger.pattern".into(), e.to_string());
                }
            }
        }

        // Conditions.
        let file_ctx = self.trigger.provides_file();
        for (i, c) in self.conditions.iter().enumerate() {
            let field = format!("conditions[{i}]");
            if !file_ctx {
                err(
                    field.clone(),
                    "file conditions need a folder_watch trigger".into(),
                );
            }
            match c {
                Condition::FileExtension { any_of } if any_of.is_empty() => {
                    err(field, "any_of must not be empty".into())
                }
                Condition::FileNameMatches { glob } => {
                    if let Err(e) = Glob::new(glob) {
                        err(field, e.to_string());
                    }
                }
                Condition::FileSize {
                    min_bytes,
                    max_bytes,
                } => {
                    if min_bytes.is_none() && max_bytes.is_none() {
                        err(field, "set min_bytes and/or max_bytes".into());
                    } else if let (Some(a), Some(b)) = (min_bytes, max_bytes)
                        && a > b
                    {
                        err(field, "min_bytes is greater than max_bytes".into());
                    }
                }
                Condition::FileExtension { .. } => {}
            }
        }

        // Actions.
        if self.actions.is_empty() {
            err("actions".into(), "at least one action is required".into());
        }
        if self.actions.len() > MAX_ACTIONS {
            err(
                "actions".into(),
                format!("at most {MAX_ACTIONS} actions are allowed"),
            );
        }
        for (i, action) in self.actions.iter().enumerate() {
            let base = format!("actions[{i}]");
            for (name, tpl) in action.templates() {
                match referenced_vars(tpl) {
                    Ok(vars) => {
                        for v in vars {
                            let global = GLOBAL_VARS.contains(&v.as_str());
                            let file = FILE_VARS.contains(&v.as_str());
                            if !global && !file {
                                err(
                                    format!("{base}.{name}"),
                                    format!("unknown variable `{{{v}}}`"),
                                );
                            } else if file && !file_ctx {
                                err(
                                    format!("{base}.{name}"),
                                    format!(
                                        "`{{{v}}}` is only available with a folder_watch trigger"
                                    ),
                                );
                            }
                        }
                    }
                    Err(m) => err(format!("{base}.{name}"), m),
                }
            }
            for req in action.requirements() {
                match (&req.access, &req.path) {
                    (Access::Notify, _) => {
                        if !self.permissions.notify {
                            err(
                                base.clone(),
                                "notifications need `permissions.notify = true`".into(),
                            );
                        }
                    }
                    (access, Some(tpl)) => {
                        let Some(path) = placeholder_path(tpl, watched.as_deref()) else {
                            continue; // template error already reported
                        };
                        match lexical_path(&path) {
                            Ok(p) => {
                                let ok = match access {
                                    Access::Write => covered(&p, &req_write),
                                    _ => covered(&p, &req_read) || covered(&p, &req_write),
                                };
                                if !ok {
                                    err(
                                        base.clone(),
                                        format!(
                                            "{} needs {access} access to `{tpl}`, which is not in permissions.{access}",
                                            action.kind()
                                        ),
                                    );
                                }
                            }
                            Err(e) => err(base.clone(), e.to_string()),
                        }
                    }
                    (_, None) => {}
                }
            }
        }

        if errs.is_empty() { Ok(()) } else { Err(errs) }
    }
}

fn covered(path: &Path, roots: &[PathBuf]) -> bool {
    roots.iter().any(|r| path.starts_with(r))
}

/// Replaces template variables with representative values so a path can be
/// checked statically. File variables resolve to a file inside the watched
/// folder. Values never contain `/` or `..`, so containment is preserved.
fn placeholder_path(tpl: &str, watched: Option<&Path>) -> Option<String> {
    let vars = referenced_vars(tpl).ok()?;
    let mut out = tpl.to_string();
    for v in vars {
        let value = match v.as_str() {
            "file" => watched?.join("_file").display().to_string(),
            "file_dir" => watched?.display().to_string(),
            _ => "_value".to_string(),
        };
        out = out.replace(&format!("{{{v}}}"), &value);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = r#"
id = "organize-pdfs"
name = "Organize PDFs"

[trigger]
type = "folder_watch"
path = "/data/inbox"
pattern = "*.pdf"

[[conditions]]
type = "file_size"
max_bytes = 10_000_000

[[actions]]
type = "move_file"
to_dir = "/data/pdfs/{date}"

[[actions]]
type = "notify"
title = "Filed {file_name}"

[permissions]
write = ["/data/inbox", "/data/pdfs"]
notify = true
"#;

    #[test]
    fn valid_workflow_passes() -> Result<(), Box<dyn std::error::Error>> {
        let wf = Workflow::from_toml(GOOD)?;
        assert_eq!(wf.validate(&ValidationOptions::default()), Ok(()));
        // round-trips through TOML
        let again = Workflow::from_toml(&wf.to_toml()?)?;
        assert_eq!(wf, again);
        Ok(())
    }

    #[test]
    fn unknown_fields_are_rejected() {
        let bad = GOOD.replace(
            "pattern = \"*.pdf\"",
            "pattern = \"*.pdf\"\nshell = \"rm -rf /\"",
        );
        assert!(Workflow::from_toml(&bad).is_err());
        let bad = GOOD.replace("type = \"notify\"", "type = \"run_shell\"");
        assert!(Workflow::from_toml(&bad).is_err());
    }

    fn errors_for(src: &str) -> Vec<String> {
        match Workflow::from_toml(src) {
            Ok(wf) => wf
                .validate(&ValidationOptions::default())
                .err()
                .unwrap_or_default()
                .into_iter()
                .map(|e| e.to_string())
                .collect(),
            Err(e) => vec![e.to_string()],
        }
    }

    #[test]
    fn missing_permissions_are_reported() {
        let src = GOOD.replace("notify = true", "notify = false").replace(
            "write = [\"/data/inbox\", \"/data/pdfs\"]",
            "write = [\"/data/inbox\"]",
        );
        let errs = errors_for(&src);
        assert!(errs.iter().any(|e| e.contains("/data/pdfs")), "{errs:?}");
        assert!(
            errs.iter().any(|e| e.contains("permissions.notify")),
            "{errs:?}"
        );
    }

    #[test]
    fn bad_values_are_reported() {
        let src = r#"
id = "Bad ID"
name = ""
[trigger]
type = "schedule"
every = "1s"
[[conditions]]
type = "file_extension"
any_of = ["pdf"]
[[actions]]
type = "write_file"
path = "relative/{file_name}"
content = "{nope}"
"#;
        let errs = errors_for(src);
        for needle in [
            "id:",
            "name:",
            "trigger.every",
            "conditions[0]",
            "only available",
            "unknown variable",
            "absolute",
        ] {
            assert!(
                errs.iter().any(|e| e.contains(needle)),
                "missing `{needle}` in {errs:?}"
            );
        }
    }

    #[test]
    fn schedule_needs_exactly_one_spec() {
        let both = r#"
id = "s"
name = "s"
[trigger]
type = "schedule"
every = "1h"
cron = "0 9 * * *"
[[actions]]
type = "create_directory"
path = "/tmp/x"
[permissions]
write = ["/tmp"]
"#;
        assert!(errors_for(both).iter().any(|e| e.contains("not both")));
        assert!(errors_for(&both.replace("every = \"1h\"\n", "")).is_empty());
        assert!(
            !errors_for(
                &both
                    .replace("every = \"1h\"\n", "")
                    .replace("0 9 * * *", "61 * * * *")
            )
            .is_empty()
        );
    }

    #[test]
    fn traversal_in_paths_is_rejected() {
        let src = GOOD.replace("/data/pdfs/{date}", "/data/pdfs/../../etc");
        assert!(errors_for(&src).iter().any(|e| e.contains("..")));
    }
}
