//! Permission model.
//!
//! A workflow *requests* access in its definition. Nothing runs until the user
//! *grants* that request (the [`Grant`] is persisted). Every file operation is
//! then checked again at run time against the stored grant, using fully
//! resolved paths, so symlinks or `..` tricks cannot escape the granted scope.
//!
//! Rules:
//! - Write access implies read access to the same subtree.
//! - Sensitive actions (trashing files, overwriting files) additionally need
//!   either `allow_sensitive` on the grant or a per-action approval.

use std::io;
use std::path::{Component, Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::template::expand_home;

/// What a workflow asks for, as written in its definition.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PermissionRequest {
    /// Folders the workflow may read from.
    #[serde(default)]
    pub read: Vec<String>,
    /// Folders the workflow may create, modify, move or trash files in.
    #[serde(default)]
    pub write: Vec<String>,
    /// Whether the workflow may send desktop notifications.
    #[serde(default)]
    pub notify: bool,
}

/// What the user approved. Paths are stored fully resolved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Grant {
    /// Workflow this grant belongs to.
    pub workflow_id: String,
    /// Resolved readable roots.
    pub read: Vec<PathBuf>,
    /// Resolved writable roots.
    pub write: Vec<PathBuf>,
    /// Notifications allowed.
    pub notify: bool,
    /// Sensitive actions (trash, overwrite) run without asking each time.
    pub allow_sensitive: bool,
    /// When the user granted this.
    pub granted_at: DateTime<Utc>,
}

/// The kind of access an operation needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    /// Read a path.
    Read,
    /// Create, modify, move or remove a path.
    Write,
    /// Send a notification.
    Notify,
}

impl std::fmt::Display for Access {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Access::Read => "read",
            Access::Write => "write",
            Access::Notify => "notify",
        })
    }
}

/// A permission failure.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PermissionError {
    /// The user has not granted this workflow's permissions.
    #[error("workflow `{0}` has no permission grant; the user must approve its permissions first")]
    NotGranted(String),
    /// The grant does not cover what the workflow currently requests.
    #[error("the grant for `{workflow}` does not cover {access} access to {path}")]
    GrantTooNarrow {
        /// Workflow id.
        workflow: String,
        /// Access needed.
        access: Access,
        /// Path (or `notifications`).
        path: String,
    },
    /// A run-time operation falls outside the grant.
    #[error("{access} access to {path} is outside the granted scope")]
    OutOfScope {
        /// Access needed.
        access: Access,
        /// Resolved path.
        path: String,
    },
    /// The path is malformed (relative, contains `..`, etc).
    #[error("unsafe path {path}: {reason}")]
    UnsafePath {
        /// Offending path.
        path: String,
        /// Why it was rejected.
        reason: String,
    },
    /// A sensitive action was not approved.
    #[error("sensitive action `{0}` was not approved")]
    NotApproved(String),
}

/// Checks that a path string is absolute and free of `..` components,
/// after expanding `~`. Does not touch the filesystem.
pub fn lexical_path(raw: &str) -> Result<PathBuf, PermissionError> {
    let expanded = expand_home(raw);
    let path = PathBuf::from(&expanded);
    if !path.is_absolute() {
        return Err(PermissionError::UnsafePath {
            path: raw.to_string(),
            reason: "must be absolute (or start with ~/)".into(),
        });
    }
    if path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(PermissionError::UnsafePath {
            path: raw.to_string(),
            reason: "must not contain `..`".into(),
        });
    }
    Ok(path)
}

/// Resolves a path that may not exist yet: the longest existing ancestor is
/// canonicalized (following symlinks) and the remaining components appended.
pub fn resolve_path(path: &Path) -> Result<PathBuf, PermissionError> {
    let lexical = lexical_path(&path.to_string_lossy())?;
    let mut existing = lexical.as_path();
    let mut rest: Vec<&std::ffi::OsStr> = Vec::new();
    loop {
        match existing.canonicalize() {
            Ok(canon) => {
                let mut out = canon;
                for part in rest.iter().rev() {
                    out.push(part);
                }
                return Ok(out);
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                let (Some(parent), Some(name)) = (existing.parent(), existing.file_name()) else {
                    return Err(PermissionError::UnsafePath {
                        path: path.display().to_string(),
                        reason: "no existing ancestor".into(),
                    });
                };
                rest.push(name);
                existing = parent;
            }
            Err(e) => {
                return Err(PermissionError::UnsafePath {
                    path: path.display().to_string(),
                    reason: e.to_string(),
                });
            }
        }
    }
}

fn within_any(path: &Path, roots: &[PathBuf]) -> bool {
    roots.iter().any(|root| path.starts_with(root))
}

impl Grant {
    /// Builds a grant covering exactly `request`, resolving every path.
    /// Call this only after the user has approved the request.
    pub fn from_request(
        workflow_id: &str,
        request: &PermissionRequest,
        allow_sensitive: bool,
    ) -> Result<Self, PermissionError> {
        let resolve_all = |paths: &[String]| -> Result<Vec<PathBuf>, PermissionError> {
            paths
                .iter()
                .map(|p| resolve_path(&lexical_path(p)?))
                .collect()
        };
        Ok(Self {
            workflow_id: workflow_id.to_string(),
            read: resolve_all(&request.read)?,
            write: resolve_all(&request.write)?,
            notify: request.notify,
            allow_sensitive,
            granted_at: Utc::now(),
        })
    }

    /// Verifies that this grant covers everything in `request`.
    pub fn covers(&self, request: &PermissionRequest) -> Result<(), PermissionError> {
        let too_narrow = |access, path: String| PermissionError::GrantTooNarrow {
            workflow: self.workflow_id.clone(),
            access,
            path,
        };
        for raw in &request.write {
            let p = resolve_path(&lexical_path(raw)?)?;
            if !within_any(&p, &self.write) {
                return Err(too_narrow(Access::Write, p.display().to_string()));
            }
        }
        for raw in &request.read {
            let p = resolve_path(&lexical_path(raw)?)?;
            if !within_any(&p, &self.read) && !within_any(&p, &self.write) {
                return Err(too_narrow(Access::Read, p.display().to_string()));
            }
        }
        if request.notify && !self.notify {
            return Err(too_narrow(Access::Notify, "notifications".into()));
        }
        Ok(())
    }

    /// Run-time check for a concrete path. Returns the resolved path to use,
    /// so callers operate on exactly what was checked.
    pub fn check_path(&self, access: Access, path: &Path) -> Result<PathBuf, PermissionError> {
        let resolved = resolve_path(path)?;
        let ok = match access {
            Access::Read => within_any(&resolved, &self.read) || within_any(&resolved, &self.write),
            Access::Write => within_any(&resolved, &self.write),
            Access::Notify => self.notify,
        };
        if ok {
            Ok(resolved)
        } else {
            Err(PermissionError::OutOfScope {
                access,
                path: resolved.display().to_string(),
            })
        }
    }

    /// Run-time check for notifications.
    pub fn check_notify(&self) -> Result<(), PermissionError> {
        if self.notify {
            Ok(())
        } else {
            Err(PermissionError::OutOfScope {
                access: Access::Notify,
                path: "notifications".into(),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_relative_and_parent_paths() {
        assert!(lexical_path("relative/path").is_err());
        assert!(lexical_path("/tmp/a/../etc").is_err());
        assert!(lexical_path("/tmp/a").is_ok());
    }

    #[test]
    fn grant_scopes_paths() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let inbox = dir.path().join("inbox");
        let out = dir.path().join("out");
        std::fs::create_dir_all(&inbox)?;
        let req = PermissionRequest {
            read: vec![inbox.display().to_string()],
            write: vec![out.display().to_string()],
            notify: false,
        };
        let grant = Grant::from_request("wf", &req, false)?;
        grant.covers(&req)?;
        assert!(grant.check_path(Access::Read, &inbox.join("a.txt")).is_ok());
        assert!(
            grant
                .check_path(Access::Write, &inbox.join("a.txt"))
                .is_err()
        );
        assert!(
            grant
                .check_path(Access::Write, &out.join("x/y.txt"))
                .is_ok()
        );
        assert!(grant.check_path(Access::Read, &out.join("y.txt")).is_ok());
        assert!(grant.check_path(Access::Read, dir.path()).is_err());
        assert!(grant.check_notify().is_err());
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn symlink_escape_is_blocked() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let allowed = dir.path().join("allowed");
        let secret = dir.path().join("secret");
        std::fs::create_dir_all(&allowed)?;
        std::fs::create_dir_all(&secret)?;
        std::os::unix::fs::symlink(&secret, allowed.join("link"))?;
        let req = PermissionRequest {
            write: vec![allowed.display().to_string()],
            ..Default::default()
        };
        let grant = Grant::from_request("wf", &req, false)?;
        let err = grant.check_path(Access::Write, &allowed.join("link/file.txt"));
        assert!(matches!(err, Err(PermissionError::OutOfScope { .. })));
        Ok(())
    }

    #[test]
    fn wider_request_is_not_covered() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let a = dir.path().join("a");
        std::fs::create_dir_all(&a)?;
        let grant = Grant::from_request(
            "wf",
            &PermissionRequest {
                write: vec![a.display().to_string()],
                ..Default::default()
            },
            false,
        )?;
        let wider = PermissionRequest {
            write: vec![dir.path().display().to_string()],
            notify: true,
            ..Default::default()
        };
        assert!(matches!(
            grant.covers(&wider),
            Err(PermissionError::GrantTooNarrow { .. })
        ));
        Ok(())
    }
}
