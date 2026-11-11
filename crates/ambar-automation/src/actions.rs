//! Action execution.
//!
//! Every filesystem operation goes through [`Grant::check_path`] first and then
//! operates on the *resolved* path returned by that check. There is no action
//! that executes programs or shell commands.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::notify::{Notification, Notifier};
use crate::permissions::{Access, Grant, PermissionError};
use crate::template::{Vars, expand_home, render};
use crate::workflow::Action;

/// Everything an action needs at run time.
#[derive(Debug)]
pub struct ActionContext<'a> {
    /// Workflow id.
    pub workflow_id: &'a str,
    /// Execution id (used to name trashed files).
    pub execution_id: &'a str,
    /// Template variables.
    pub vars: &'a Vars,
    /// The user's grant, re-read for this execution.
    pub grant: &'a Grant,
    /// Engine-owned trash folder.
    pub trash_dir: &'a Path,
    /// Notification backend.
    pub notifier: &'a dyn Notifier,
}

/// Why an action did not complete.
#[derive(Debug, thiserror::Error)]
pub enum ActionError {
    /// A permission check failed.
    #[error(transparent)]
    Permission(#[from] PermissionError),
    /// The operation itself failed.
    #[error("{0}")]
    Failed(String),
}

impl From<io::Error> for ActionError {
    fn from(e: io::Error) -> Self {
        ActionError::Failed(e.to_string())
    }
}

fn path_of(tpl: &str, ctx: &ActionContext<'_>) -> Result<PathBuf, ActionError> {
    let rendered = render(tpl, ctx.vars).map_err(ActionError::Failed)?;
    Ok(PathBuf::from(expand_home(&rendered)))
}

fn text_of(tpl: &str, ctx: &ActionContext<'_>) -> Result<String, ActionError> {
    render(tpl, ctx.vars).map_err(ActionError::Failed)
}

/// Whether this action, with these inputs, needs explicit approval right now.
pub fn needs_approval(action: &Action, ctx: &ActionContext<'_>) -> Result<bool, ActionError> {
    if action.is_always_sensitive() {
        return Ok(true);
    }
    if let Action::WriteFile {
        path,
        append: false,
        ..
    } = action
    {
        // Replacing an existing file destroys its old contents.
        return Ok(path_of(path, ctx)?.exists());
    }
    Ok(false)
}

/// Human-readable description used in approval prompts.
pub fn describe(action: &Action, ctx: &ActionContext<'_>) -> String {
    let p = |t: &str| {
        path_of(t, ctx)
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| t.to_string())
    };
    match action {
        Action::CreateDirectory { path } => format!("create folder {}", p(path)),
        Action::CopyFile {
            from,
            to_dir,
            overwrite,
        } => format!(
            "copy {} to {}{}",
            p(from),
            p(to_dir),
            if *overwrite {
                " (replacing existing)"
            } else {
                ""
            }
        ),
        Action::MoveFile {
            from,
            to_dir,
            overwrite,
        } => format!(
            "move {} to {}{}",
            p(from),
            p(to_dir),
            if *overwrite {
                " (replacing existing)"
            } else {
                ""
            }
        ),
        Action::WriteFile { path, append, .. } => {
            format!(
                "{} {}",
                if *append { "append to" } else { "write" },
                p(path)
            )
        }
        Action::TrashFile { path } => format!("move {} to the Ambar trash", p(path)),
        Action::Notify { title, .. } => {
            format!("notify \"{}\"", text_of(title, ctx).unwrap_or_default())
        }
    }
}

/// Runs one action. Returns a message for the history log.
pub fn execute(action: &Action, ctx: &ActionContext<'_>) -> Result<String, ActionError> {
    match action {
        Action::CreateDirectory { path } => {
            let target = ctx.grant.check_path(Access::Write, &path_of(path, ctx)?)?;
            fs::create_dir_all(&target)?;
            Ok(format!("folder ready: {}", target.display()))
        }
        Action::CopyFile {
            from,
            to_dir,
            overwrite,
        } => {
            let src = ctx.grant.check_path(Access::Read, &path_of(from, ctx)?)?;
            let dest = destination(&src, &path_of(to_dir, ctx)?, ctx.grant)?;
            refuse_existing(&dest, *overwrite)?;
            fs::copy(&src, &dest)?;
            Ok(format!("copied {} -> {}", src.display(), dest.display()))
        }
        Action::MoveFile {
            from,
            to_dir,
            overwrite,
        } => {
            let src = ctx.grant.check_path(Access::Write, &path_of(from, ctx)?)?;
            let dest = destination(&src, &path_of(to_dir, ctx)?, ctx.grant)?;
            refuse_existing(&dest, *overwrite)?;
            move_file(&src, &dest)?;
            Ok(format!("moved {} -> {}", src.display(), dest.display()))
        }
        Action::WriteFile {
            path,
            content,
            append,
        } => {
            let target = ctx.grant.check_path(Access::Write, &path_of(path, ctx)?)?;
            let text = text_of(content, ctx)?;
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut file = fs::OpenOptions::new()
                .create(true)
                .write(true)
                .append(*append)
                .truncate(!*append)
                .open(&target)?;
            file.write_all(text.as_bytes())?;
            if *append && !text.ends_with('\n') {
                file.write_all(b"\n")?;
            }
            Ok(format!(
                "wrote {} bytes to {}",
                text.len(),
                target.display()
            ))
        }
        Action::TrashFile { path } => {
            let src = ctx.grant.check_path(Access::Write, &path_of(path, ctx)?)?;
            require_file(&src)?;
            fs::create_dir_all(ctx.trash_dir)?;
            let name = src
                .file_name()
                .ok_or_else(|| ActionError::Failed("path has no file name".into()))?;
            let short: String = ctx.execution_id.chars().take(8).collect();
            let dest = ctx
                .trash_dir
                .join(format!("{short}-{}", name.to_string_lossy()));
            move_file(&src, &dest)?;
            Ok(format!(
                "trashed {} (recoverable at {})",
                src.display(),
                dest.display()
            ))
        }
        Action::Notify { title, body } => {
            ctx.grant.check_notify()?;
            let n = Notification {
                workflow_id: ctx.workflow_id.to_string(),
                title: text_of(title, ctx)?,
                body: text_of(body, ctx)?,
            };
            ctx.notifier.notify(&n).map_err(ActionError::Failed)?;
            Ok(format!("notified: {}", n.title))
        }
    }
}

fn require_file(path: &Path) -> Result<(), ActionError> {
    let meta = fs::symlink_metadata(path)?;
    if meta.is_file() {
        Ok(())
    } else {
        Err(ActionError::Failed(format!(
            "{} is not a regular file",
            path.display()
        )))
    }
}

/// Destination path for copy/move: `to_dir/<source name>`, checked for write
/// access, with `to_dir` created if needed.
fn destination(src: &Path, to_dir: &Path, grant: &Grant) -> Result<PathBuf, ActionError> {
    require_file(src)?;
    let dir = grant.check_path(Access::Write, to_dir)?;
    let name = src
        .file_name()
        .ok_or_else(|| ActionError::Failed("source has no file name".into()))?;
    let dest = grant.check_path(Access::Write, &dir.join(name))?;
    fs::create_dir_all(&dir)?;
    if dest == src {
        return Err(ActionError::Failed(
            "source and destination are the same file".into(),
        ));
    }
    Ok(dest)
}

fn refuse_existing(dest: &Path, overwrite: bool) -> Result<(), ActionError> {
    if !overwrite && fs::symlink_metadata(dest).is_ok() {
        return Err(ActionError::Failed(format!(
            "{} already exists (set overwrite = true to replace it)",
            dest.display()
        )));
    }
    Ok(())
}

fn move_file(src: &Path, dest: &Path) -> io::Result<()> {
    match fs::rename(src, dest) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::CrossesDevices => {
            fs::copy(src, dest)?;
            fs::remove_file(src)
        }
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notify::MemoryNotifier;
    use crate::permissions::PermissionRequest;

    struct Fixture {
        _dir: tempfile::TempDir,
        root: PathBuf,
        grant: Grant,
        vars: Vars,
        notifier: MemoryNotifier,
    }

    fn fixture() -> Result<Fixture, Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let root = dir.path().canonicalize()?;
        fs::create_dir_all(root.join("in"))?;
        fs::create_dir_all(root.join("ro"))?;
        fs::write(root.join("in/a.txt"), "hello")?;
        fs::write(root.join("ro/r.txt"), "readonly")?;
        let grant = Grant::from_request(
            "wf",
            &PermissionRequest {
                read: vec![root.join("ro").display().to_string()],
                write: vec![
                    root.join("in").display().to_string(),
                    root.join("out").display().to_string(),
                ],
                notify: true,
            },
            false,
        )?;
        let mut vars = Vars::new();
        vars.insert("file", root.join("in/a.txt").display().to_string());
        vars.insert("file_name", "a.txt".into());
        Ok(Fixture {
            _dir: dir,
            root,
            grant,
            vars,
            notifier: MemoryNotifier::default(),
        })
    }

    fn ctx<'a>(f: &'a Fixture, trash: &'a Path) -> ActionContext<'a> {
        ActionContext {
            workflow_id: "wf",
            execution_id: "exec-1234567890",
            vars: &f.vars,
            grant: &f.grant,
            trash_dir: trash,
            notifier: &f.notifier,
        }
    }

    #[test]
    fn move_copy_write_trash_notify() -> Result<(), Box<dyn std::error::Error>> {
        let f = fixture()?;
        let trash = f.root.join("trash");
        let c = ctx(&f, &trash);
        let out = f.root.join("out").display().to_string();

        execute(
            &Action::CopyFile {
                from: "{file}".into(),
                to_dir: out.clone(),
                overwrite: false,
            },
            &c,
        )?;
        assert!(f.root.join("out/a.txt").exists());

        // copying again without overwrite refuses
        let again = execute(
            &Action::CopyFile {
                from: "{file}".into(),
                to_dir: out.clone(),
                overwrite: false,
            },
            &c,
        );
        assert!(matches!(again, Err(ActionError::Failed(_))));
        fs::remove_file(f.root.join("out/a.txt"))?;

        execute(
            &Action::MoveFile {
                from: "{file}".into(),
                to_dir: format!("{out}/sub"),
                overwrite: false,
            },
            &c,
        )?;
        assert!(!f.root.join("in/a.txt").exists());
        assert!(f.root.join("out/sub/a.txt").exists());

        let log = f.root.join("out/log.txt").display().to_string();
        execute(
            &Action::WriteFile {
                path: log.clone(),
                content: "one".into(),
                append: true,
            },
            &c,
        )?;
        execute(
            &Action::WriteFile {
                path: log.clone(),
                content: "two".into(),
                append: true,
            },
            &c,
        )?;
        assert_eq!(
            fs::read_to_string(f.root.join("out/log.txt"))?,
            "one\ntwo\n"
        );

        let w = Action::WriteFile {
            path: log.clone(),
            content: "x".into(),
            append: false,
        };
        assert!(needs_approval(&w, &c)?);

        execute(
            &Action::TrashFile {
                path: f.root.join("out/sub/a.txt").display().to_string(),
            },
            &c,
        )?;
        assert!(!f.root.join("out/sub/a.txt").exists());
        assert!(trash.join("exec-123-a.txt").exists());

        execute(
            &Action::Notify {
                title: "Got {file_name}".into(),
                body: String::new(),
            },
            &c,
        )?;
        assert_eq!(f.notifier.sent()[0].title, "Got a.txt");
        Ok(())
    }

    #[test]
    fn out_of_scope_operations_are_denied() -> Result<(), Box<dyn std::error::Error>> {
        let f = fixture()?;
        let trash = f.root.join("trash");
        let c = ctx(&f, &trash);
        // read-only folder cannot be written or moved from
        let ro = f.root.join("ro/r.txt").display().to_string();
        let r = execute(
            &Action::MoveFile {
                from: ro.clone(),
                to_dir: f.root.join("out").display().to_string(),
                overwrite: false,
            },
            &c,
        );
        assert!(matches!(r, Err(ActionError::Permission(_))));
        // but can be copied out of
        execute(
            &Action::CopyFile {
                from: ro,
                to_dir: f.root.join("out").display().to_string(),
                overwrite: false,
            },
            &c,
        )?;
        // writing outside the grant is denied
        let r = execute(
            &Action::WriteFile {
                path: f.root.join("elsewhere.txt").display().to_string(),
                content: "x".into(),
                append: false,
            },
            &c,
        );
        assert!(matches!(r, Err(ActionError::Permission(_))));
        assert!(!f.root.join("elsewhere.txt").exists());
        Ok(())
    }
}
