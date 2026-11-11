//! Notification backends.

use std::process::Command;
use std::sync::{Arc, Mutex};

/// A notification produced by a workflow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notification {
    /// Workflow that sent it.
    pub workflow_id: String,
    /// Title line.
    pub title: String,
    /// Body text.
    pub body: String,
}

/// Something that can deliver notifications.
pub trait Notifier: Send + Sync + std::fmt::Debug {
    /// Delivers a notification. Errors are reported in the step record.
    fn notify(&self, notification: &Notification) -> Result<(), String>;
}

/// Writes notifications to the structured log only.
#[derive(Debug, Default, Clone)]
pub struct LogNotifier;

impl Notifier for LogNotifier {
    fn notify(&self, n: &Notification) -> Result<(), String> {
        tracing::info!(workflow = %n.workflow_id, title = %n.title, body = %n.body, "notification");
        Ok(())
    }
}

/// Keeps notifications in memory. Useful for tests and UIs.
#[derive(Debug, Default, Clone)]
pub struct MemoryNotifier {
    sent: Arc<Mutex<Vec<Notification>>>,
}

impl MemoryNotifier {
    /// Notifications delivered so far.
    pub fn sent(&self) -> Vec<Notification> {
        self.sent.lock().map(|v| v.clone()).unwrap_or_default()
    }
}

impl Notifier for MemoryNotifier {
    fn notify(&self, n: &Notification) -> Result<(), String> {
        self.sent
            .lock()
            .map_err(|_| "notification store poisoned".to_string())?
            .push(n.clone());
        Ok(())
    }
}

/// Native desktop notifications.
///
/// Linux uses `notify-send` (libnotify, available on KDE Plasma and GNOME);
/// macOS uses `osascript` for development machines. Programs are invoked
/// directly with argument vectors — never through a shell. Also logs.
#[derive(Debug, Default, Clone)]
pub struct DesktopNotifier;

fn applescript_quote(s: &str) -> String {
    let escaped = s.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

impl Notifier for DesktopNotifier {
    fn notify(&self, n: &Notification) -> Result<(), String> {
        LogNotifier.notify(n)?;
        let mut cmd = if cfg!(target_os = "macos") {
            let script = format!(
                "display notification {} with title {}",
                applescript_quote(&n.body),
                applescript_quote(&n.title)
            );
            let mut c = Command::new("osascript");
            c.arg("-e").arg(script);
            c
        } else {
            let mut c = Command::new("notify-send");
            c.arg("--app-name=Ambar")
                .arg("--")
                .arg(&n.title)
                .arg(&n.body);
            c
        };
        let status = cmd
            .status()
            .map_err(|e| format!("could not run notification program: {e}"))?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("notification program exited with {status}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn applescript_strings_are_escaped() {
        assert_eq!(applescript_quote(r#"a"b\c"#), r#""a\"b\\c""#);
    }

    #[test]
    fn memory_notifier_records() {
        let m = MemoryNotifier::default();
        let n = Notification {
            workflow_id: "w".into(),
            title: "t".into(),
            body: "b".into(),
        };
        assert!(m.notify(&n).is_ok());
        assert_eq!(m.sent(), vec![n]);
    }
}
