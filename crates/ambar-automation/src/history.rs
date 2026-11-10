//! Persistent storage: workflows, permission grants, execution history and an
//! audit log, in a local SQLite database.

use std::path::Path;
use std::str::FromStr;
use std::sync::{Arc, Mutex, MutexGuard};

use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::error::{EngineError, Result};
use crate::permissions::Grant;

const SCHEMA: &str = r#"
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;
CREATE TABLE IF NOT EXISTS schema_version (version INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS workflows (
    id          TEXT PRIMARY KEY,
    definition  TEXT NOT NULL,
    state       TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS grants (
    workflow_id TEXT PRIMARY KEY,
    grant_json  TEXT NOT NULL,
    granted_at  TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS executions (
    id          TEXT PRIMARY KEY,
    workflow_id TEXT NOT NULL,
    trigger     TEXT NOT NULL,
    status      TEXT NOT NULL,
    started_at  TEXT NOT NULL,
    finished_at TEXT,
    error       TEXT
);
CREATE INDEX IF NOT EXISTS executions_by_workflow ON executions (workflow_id, started_at);
CREATE TABLE IF NOT EXISTS steps (
    execution_id TEXT NOT NULL REFERENCES executions(id) ON DELETE CASCADE,
    idx          INTEGER NOT NULL,
    action       TEXT NOT NULL,
    status       TEXT NOT NULL,
    message      TEXT NOT NULL,
    finished_at  TEXT NOT NULL,
    PRIMARY KEY (execution_id, idx)
);
CREATE TABLE IF NOT EXISTS audit (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    at          TEXT NOT NULL,
    workflow_id TEXT NOT NULL,
    event       TEXT NOT NULL,
    detail      TEXT NOT NULL
);
"#;

/// Outcome of an execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionStatus {
    /// Still running.
    Running,
    /// Every action succeeded.
    Succeeded,
    /// An action failed.
    Failed,
    /// The workflow was cancelled during the run.
    Cancelled,
    /// A permission check or approval was refused.
    Denied,
    /// The trigger fired while the workflow was paused.
    Skipped,
}

/// Outcome of one action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    /// Action completed.
    Succeeded,
    /// Action returned an error.
    Failed,
    /// Permission or approval refused.
    Denied,
}

macro_rules! str_enum {
    ($t:ty { $($v:ident => $s:literal),* $(,)? }) => {
        impl $t {
            /// Stored representation.
            pub fn as_str(&self) -> &'static str {
                match self { $(Self::$v => $s),* }
            }
        }
        impl FromStr for $t {
            type Err = String;
            fn from_str(s: &str) -> std::result::Result<Self, String> {
                match s { $($s => Ok(Self::$v),)* other => Err(format!("unknown status `{other}`")) }
            }
        }
        impl std::fmt::Display for $t {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.as_str())
            }
        }
    };
}

str_enum!(ExecutionStatus {
    Running => "running",
    Succeeded => "succeeded",
    Failed => "failed",
    Cancelled => "cancelled",
    Denied => "denied",
    Skipped => "skipped",
});

str_enum!(StepStatus {
    Succeeded => "succeeded",
    Failed => "failed",
    Denied => "denied",
});

/// One recorded action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepRecord {
    /// Position in the workflow's action list.
    pub index: usize,
    /// Action kind, e.g. `move_file`.
    pub action: String,
    /// Outcome.
    pub status: StepStatus,
    /// What happened, or the error.
    pub message: String,
    /// When the step ended.
    pub finished_at: DateTime<Utc>,
}

/// One recorded workflow run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionRecord {
    /// Unique execution id.
    pub id: String,
    /// Workflow that ran.
    pub workflow_id: String,
    /// What started it, e.g. `schedule`, `manual`, `created: /path/file.pdf`.
    pub trigger: String,
    /// Outcome.
    pub status: ExecutionStatus,
    /// Start time.
    pub started_at: DateTime<Utc>,
    /// End time, if finished.
    pub finished_at: Option<DateTime<Utc>>,
    /// Error summary for failed/denied/cancelled runs.
    pub error: Option<String>,
    /// Steps that ran.
    pub steps: Vec<StepRecord>,
}

/// An audit log entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditEntry {
    /// When it happened.
    pub at: DateTime<Utc>,
    /// Workflow concerned.
    pub workflow_id: String,
    /// Event name, e.g. `granted`, `activated`, `denied`.
    pub event: String,
    /// Details.
    pub detail: String,
}

/// Thread-safe handle to the database. Cheap to clone.
#[derive(Debug, Clone)]
pub struct Store {
    conn: Arc<Mutex<Connection>>,
}

fn ts(s: String) -> rusqlite::Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(&s)
        .map(|d| d.with_timezone(&Utc))
        .map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
        })
}

fn parse_err(e: String) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        0,
        rusqlite::types::Type::Text,
        Box::new(std::io::Error::other(e)),
    )
}

impl Store {
    /// Opens (or creates) a database file.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        Self::init(Connection::open(path)?)
    }

    /// Opens a private in-memory database.
    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.execute_batch(SCHEMA)?;
        let version: Option<i64> = conn
            .query_row("SELECT version FROM schema_version", [], |r| r.get(0))
            .optional()?;
        if version.is_none() {
            conn.execute("INSERT INTO schema_version (version) VALUES (1)", [])?;
        }
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    fn conn(&self) -> Result<MutexGuard<'_, Connection>> {
        self.conn
            .lock()
            .map_err(|_| EngineError::State("database lock poisoned".into()))
    }

    // ---- workflows ----

    /// Stores a workflow definition and its state.
    pub fn save_workflow(&self, id: &str, definition_toml: &str, state: &str) -> Result<()> {
        self.conn()?.execute(
            "INSERT INTO workflows (id, definition, state, updated_at) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(id) DO UPDATE SET definition = ?2, state = ?3, updated_at = ?4",
            params![id, definition_toml, state, Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    /// Updates a stored workflow's state.
    pub fn set_workflow_state(&self, id: &str, state: &str) -> Result<()> {
        self.conn()?.execute(
            "UPDATE workflows SET state = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, state, Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    /// Deletes a stored workflow definition (history and audit are kept).
    pub fn delete_workflow(&self, id: &str) -> Result<bool> {
        let n = self
            .conn()?
            .execute("DELETE FROM workflows WHERE id = ?1", params![id])?;
        Ok(n > 0)
    }

    /// Lists stored workflows as `(id, definition, state)`.
    pub fn list_workflows(&self) -> Result<Vec<(String, String, String)>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare("SELECT id, definition, state FROM workflows ORDER BY id")?;
        let rows = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    // ---- grants ----

    /// Saves (replaces) the grant for a workflow.
    pub fn save_grant(&self, grant: &Grant) -> Result<()> {
        let json = serde_json::to_string(grant).map_err(|e| EngineError::State(e.to_string()))?;
        self.conn()?.execute(
            "INSERT INTO grants (workflow_id, grant_json, granted_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(workflow_id) DO UPDATE SET grant_json = ?2, granted_at = ?3",
            params![grant.workflow_id, json, grant.granted_at.to_rfc3339()],
        )?;
        Ok(())
    }

    /// Loads the grant for a workflow, if any.
    pub fn grant(&self, workflow_id: &str) -> Result<Option<Grant>> {
        let json: Option<String> = self
            .conn()?
            .query_row(
                "SELECT grant_json FROM grants WHERE workflow_id = ?1",
                params![workflow_id],
                |r| r.get(0),
            )
            .optional()?;
        json.map(|j| serde_json::from_str(&j).map_err(|e| EngineError::State(e.to_string())))
            .transpose()
    }

    /// Removes a workflow's grant.
    pub fn revoke_grant(&self, workflow_id: &str) -> Result<bool> {
        let n = self.conn()?.execute(
            "DELETE FROM grants WHERE workflow_id = ?1",
            params![workflow_id],
        )?;
        Ok(n > 0)
    }

    // ---- executions ----

    /// Records the start of an execution.
    pub fn begin_execution(&self, record: &ExecutionRecord) -> Result<()> {
        self.conn()?.execute(
            "INSERT INTO executions (id, workflow_id, trigger, status, started_at, finished_at, error)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                record.id,
                record.workflow_id,
                record.trigger,
                record.status.as_str(),
                record.started_at.to_rfc3339(),
                record.finished_at.map(|t| t.to_rfc3339()),
                record.error,
            ],
        )?;
        Ok(())
    }

    /// Records a finished step.
    pub fn record_step(&self, execution_id: &str, step: &StepRecord) -> Result<()> {
        self.conn()?.execute(
            "INSERT INTO steps (execution_id, idx, action, status, message, finished_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                execution_id,
                step.index as i64,
                step.action,
                step.status.as_str(),
                step.message,
                step.finished_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    /// Marks an execution finished.
    pub fn finish_execution(
        &self,
        execution_id: &str,
        status: ExecutionStatus,
        error: Option<&str>,
    ) -> Result<()> {
        self.conn()?.execute(
            "UPDATE executions SET status = ?2, finished_at = ?3, error = ?4 WHERE id = ?1",
            params![
                execution_id,
                status.as_str(),
                Utc::now().to_rfc3339(),
                error
            ],
        )?;
        Ok(())
    }

    /// Most recent executions (newest first), optionally for one workflow.
    pub fn executions(
        &self,
        workflow_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<ExecutionRecord>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT id, workflow_id, trigger, status, started_at, finished_at, error
             FROM executions WHERE (?1 IS NULL OR workflow_id = ?1)
             ORDER BY started_at DESC, rowid DESC LIMIT ?2",
        )?;
        let mut records = stmt
            .query_map(params![workflow_id, limit as i64], |r| {
                Ok(ExecutionRecord {
                    id: r.get(0)?,
                    workflow_id: r.get(1)?,
                    trigger: r.get(2)?,
                    status: r.get::<_, String>(3)?.parse().map_err(parse_err)?,
                    started_at: ts(r.get(4)?)?,
                    finished_at: r.get::<_, Option<String>>(5)?.map(ts).transpose()?,
                    error: r.get(6)?,
                    steps: Vec::new(),
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut step_stmt = conn.prepare(
            "SELECT idx, action, status, message, finished_at FROM steps
             WHERE execution_id = ?1 ORDER BY idx",
        )?;
        for rec in &mut records {
            rec.steps = step_stmt
                .query_map(params![rec.id], |r| {
                    Ok(StepRecord {
                        index: r.get::<_, i64>(0)? as usize,
                        action: r.get(1)?,
                        status: r.get::<_, String>(2)?.parse().map_err(parse_err)?,
                        message: r.get(3)?,
                        finished_at: ts(r.get(4)?)?,
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
        }
        Ok(records)
    }

    /// Marks executions left `running` by a crash as failed. Returns how many.
    pub fn recover_interrupted(&self) -> Result<usize> {
        let n = self.conn()?.execute(
            "UPDATE executions SET status = 'failed', finished_at = ?1,
             error = 'interrupted: the engine stopped during this run' WHERE status = 'running'",
            params![Utc::now().to_rfc3339()],
        )?;
        Ok(n)
    }

    // ---- audit ----

    /// Appends to the audit log.
    pub fn audit(&self, workflow_id: &str, event: &str, detail: &str) -> Result<()> {
        self.conn()?.execute(
            "INSERT INTO audit (at, workflow_id, event, detail) VALUES (?1, ?2, ?3, ?4)",
            params![Utc::now().to_rfc3339(), workflow_id, event, detail],
        )?;
        Ok(())
    }

    /// Audit entries, oldest first, optionally for one workflow.
    pub fn audit_entries(&self, workflow_id: Option<&str>) -> Result<Vec<AuditEntry>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT at, workflow_id, event, detail FROM audit
             WHERE (?1 IS NULL OR workflow_id = ?1) ORDER BY id",
        )?;
        let rows = stmt
            .query_map(params![workflow_id], |r| {
                Ok(AuditEntry {
                    at: ts(r.get(0)?)?,
                    workflow_id: r.get(1)?,
                    event: r.get(2)?,
                    detail: r.get(3)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn execution_round_trip() -> Result<()> {
        let store = Store::open_in_memory()?;
        let rec = ExecutionRecord {
            id: "e1".into(),
            workflow_id: "wf".into(),
            trigger: "manual".into(),
            status: ExecutionStatus::Running,
            started_at: Utc::now(),
            finished_at: None,
            error: None,
            steps: vec![],
        };
        store.begin_execution(&rec)?;
        store.record_step(
            "e1",
            &StepRecord {
                index: 0,
                action: "notify".into(),
                status: StepStatus::Succeeded,
                message: "sent".into(),
                finished_at: Utc::now(),
            },
        )?;
        store.finish_execution("e1", ExecutionStatus::Succeeded, None)?;
        let got = store.executions(Some("wf"), 10)?;
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].status, ExecutionStatus::Succeeded);
        assert_eq!(got[0].steps.len(), 1);
        assert!(got[0].finished_at.is_some());
        assert!(store.executions(Some("other"), 10)?.is_empty());
        Ok(())
    }

    #[test]
    fn interrupted_runs_are_recovered() -> Result<()> {
        let store = Store::open_in_memory()?;
        store.begin_execution(&ExecutionRecord {
            id: "e".into(),
            workflow_id: "wf".into(),
            trigger: "manual".into(),
            status: ExecutionStatus::Running,
            started_at: Utc::now(),
            finished_at: None,
            error: None,
            steps: vec![],
        })?;
        assert_eq!(store.recover_interrupted()?, 1);
        assert_eq!(
            store.executions(None, 1)?[0].status,
            ExecutionStatus::Failed
        );
        Ok(())
    }
}
