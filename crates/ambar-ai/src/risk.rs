//! Risk review of workflows before the user approves them.
//!
//! Validation answers "is this well-formed and self-consistent?". Risk review
//! answers "should a human look twice?" — especially for AI-generated
//! workflows. High-risk findings require explicit, interactive confirmation.

use std::path::{Path, PathBuf};
use std::time::Duration;

use ambar_automation::template::expand_home;
use ambar_automation::workflow::parse_schedule;
use ambar_automation::{Action, Trigger, Workflow};
use serde::Serialize;

/// Severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RiskLevel {
    /// Worth knowing.
    Info,
    /// Check before approving.
    Warning,
    /// Needs explicit confirmation.
    High,
}

/// One finding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Risk {
    /// Severity.
    pub level: RiskLevel,
    /// Explanation.
    pub message: String,
}

const SYSTEM_DIRS: &[&str] = &[
    "/",
    "/etc",
    "/usr",
    "/bin",
    "/sbin",
    "/lib",
    "/boot",
    "/var",
    "/opt",
    "/System",
    "/Library",
    "/Applications",
    "/private",
];

const SECRET_DIRS: &[&str] = &[
    ".ssh",
    ".gnupg",
    ".aws",
    ".kube",
    ".docker",
    ".config",
    ".password-store",
    "Library/Keychains",
    ".local/share/keyrings",
    ".local/share/ambar",
];

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

fn norm(p: &str) -> PathBuf {
    PathBuf::from(expand_home(p))
}

/// Reviews a workflow. Findings are sorted most severe first.
pub fn review(wf: &Workflow) -> Vec<Risk> {
    let mut out = Vec::new();
    let mut add = |level, message: String| out.push(Risk { level, message });
    let home = home();

    let check_path = |p: &Path, access: &str, add: &mut dyn FnMut(RiskLevel, String)| {
        if SYSTEM_DIRS.iter().any(|s| p == Path::new(s)) {
            add(
                RiskLevel::High,
                format!("{access} access to system folder {}", p.display()),
            );
        }
        if let Some(h) = &home {
            if p == h {
                let level = if access == "write" {
                    RiskLevel::High
                } else {
                    RiskLevel::Warning
                };
                add(level, format!("{access} access to your entire home folder"));
            }
            for s in SECRET_DIRS {
                let secret = h.join(s);
                if p.starts_with(&secret) || secret.starts_with(p) && p != Path::new("/") && p != h
                {
                    add(
                        RiskLevel::High,
                        format!("{access} access covers credentials or settings in ~/{s}"),
                    );
                }
            }
        }
    };

    for p in &wf.permissions.write {
        check_path(&norm(p), "write", &mut add);
    }
    for p in &wf.permissions.read {
        check_path(&norm(p), "read", &mut add);
    }

    for a in &wf.actions {
        match a {
            Action::TrashFile { path } => add(
                RiskLevel::Warning,
                format!(
                    "moves {path} to the trash (recoverable); in the background this needs --allow-sensitive"
                ),
            ),
            Action::CopyFile {
                overwrite: true, ..
            }
            | Action::MoveFile {
                overwrite: true, ..
            } => add(
                RiskLevel::Warning,
                "replaces existing files (overwrite = true)".into(),
            ),
            Action::WriteFile {
                append: false,
                path,
                ..
            } => add(
                RiskLevel::Info,
                format!("replaces {path} if it already exists (asks first)"),
            ),
            _ => {}
        }
    }

    match &wf.trigger {
        Trigger::Schedule { every, cron } => {
            if let Ok(ambar_automation::workflow::Schedule::Every(d)) =
                parse_schedule(every.as_deref(), cron.as_deref())
                && d < Duration::from_secs(60)
            {
                add(
                    RiskLevel::Info,
                    format!("runs very often (every {})", every.as_deref().unwrap_or("")),
                );
            }
        }
        Trigger::FolderWatch {
            path, recursive, ..
        } => {
            if *recursive && home.as_deref() == Some(norm(path).as_path()) {
                add(
                    RiskLevel::Warning,
                    "watches your whole home folder recursively".into(),
                );
            }
            // A move into a subfolder of a recursively watched folder re-triggers itself.
            let watched = norm(path);
            if *recursive {
                for a in &wf.actions {
                    if let Action::MoveFile { to_dir, .. } | Action::CopyFile { to_dir, .. } = a
                        && norm(to_dir).starts_with(&watched)
                    {
                        add(
                            RiskLevel::High,
                            format!(
                                "moves/copies into {to_dir}, inside the folder it watches recursively — it would trigger itself"
                            ),
                        );
                    }
                }
            }
        }
        Trigger::Manual => {}
    }

    out.sort_by_key(|r| std::cmp::Reverse(r.level));
    out.dedup();
    out
}

/// Highest level present.
pub fn max_level(risks: &[Risk]) -> Option<RiskLevel> {
    risks.iter().map(|r| r.level).max()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wf(perm: &str, extra: &str) -> Workflow {
        let src = format!(
            r#"
id = "t"
name = "t"
[trigger]
type = "manual"
{extra}
[[actions]]
type = "create_directory"
path = "/tmp/x"
[permissions]
{perm}
"#
        );
        Workflow::from_toml(&src).unwrap_or_else(|e| panic!("{e}"))
    }

    #[test]
    fn flags_broad_and_secret_paths() {
        let home = std::env::var("HOME").unwrap_or_default();
        let r = review(&wf(r#"write = ["~"]"#, ""));
        assert_eq!(max_level(&r), Some(RiskLevel::High), "{r:?}");
        let r = review(&wf(r#"read = ["~/.ssh"]"#, ""));
        assert!(r.iter().any(|x| x.message.contains(".ssh")), "{r:?}");
        let r = review(&wf(r#"write = ["/etc"]"#, ""));
        assert_eq!(max_level(&r), Some(RiskLevel::High));
        let r = review(&wf(&format!(r#"write = ["{home}/Documents/Reports"]"#), ""));
        assert_eq!(max_level(&r), None, "{r:?}");
    }
}
