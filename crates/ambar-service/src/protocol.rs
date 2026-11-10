//! Wire protocol between `ambar` and `ambard`.
//!
//! Transport: a Unix domain socket. Framing: one JSON object per line
//! (newline-delimited), at most [`MAX_FRAME`] bytes. The first request on a
//! connection must be [`Request::Hello`] with the current token; anything else
//! closes the connection.

use std::io;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// Protocol version. Bumped on incompatible changes.
pub const PROTOCOL_VERSION: u32 = 1;

/// Largest accepted request or response line.
pub const MAX_FRAME: usize = 1024 * 1024;

/// Requests the CLI can send.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    /// Authenticate. Must be the first request on a connection.
    Hello {
        /// Contents of the token file.
        token: String,
        /// Client protocol version.
        protocol: u32,
        /// Free-form client name, recorded in the audit log.
        client: String,
    },
    /// Service health.
    Health,
    /// All registered workflows.
    ListWorkflows,
    /// Register (or replace) a workflow from TOML.
    Register {
        /// Workflow TOML.
        definition: String,
        /// Grant the workflow's declared permissions. The client must have
        /// obtained the user's consent first.
        grant: Option<GrantSpec>,
        /// Replace an existing workflow (it is disabled first).
        #[serde(default)]
        replace: bool,
    },
    /// Start a workflow's trigger. Restored automatically after restart.
    Enable {
        /// Workflow id.
        id: String,
    },
    /// Stop a workflow's trigger; a run in progress finishes.
    Disable {
        /// Workflow id.
        id: String,
    },
    /// Skip new firings and hold an in-progress run before its next action.
    Pause {
        /// Workflow id.
        id: String,
    },
    /// Undo `Pause`.
    Resume {
        /// Workflow id.
        id: String,
    },
    /// Stop triggers and abort an in-progress run at its next action boundary.
    Cancel {
        /// Workflow id.
        id: String,
    },
    /// Run a manual or scheduled workflow now (refused if already running).
    RunNow {
        /// Workflow id.
        id: String,
    },
    /// Withdraw a workflow's permission grant.
    Revoke {
        /// Workflow id.
        id: String,
    },
    /// Shut the service down gracefully.
    Shutdown,

    // ---- identity & agents (Phase 6) ----
    /// Who am I authenticated as, and with which scopes.
    WhoAmI,
    /// Recent executions.
    History {
        /// Only this workflow.
        #[serde(default)]
        workflow: Option<String>,
        /// Max records (capped at 1000).
        #[serde(default = "default_limit")]
        limit: usize,
    },
    /// Agent: submit a workflow for the owner's approval.
    Propose {
        /// Workflow TOML.
        definition: String,
        /// Why the agent wants it.
        #[serde(default)]
        reason: String,
    },
    /// Proposals (all for the owner, own for an agent).
    ProposalList,
    /// Owner: approve a pending proposal (registers + grants, optionally enables).
    ProposalApprove {
        /// Proposal id.
        id: String,
        /// Pre-approve sensitive actions.
        #[serde(default)]
        allow_sensitive: bool,
        /// Enable right away.
        #[serde(default)]
        enable: bool,
    },
    /// Owner: reject a pending proposal.
    ProposalReject {
        /// Proposal id.
        id: String,
    },
    /// Agent: read a memory value.
    MemoryGet {
        /// Key.
        key: String,
    },
    /// Agent: write a memory value.
    MemoryPut {
        /// Key.
        key: String,
        /// Value (≤ 64 KiB).
        value: String,
    },
    /// Agent: list memory keys.
    MemoryList,
    /// Agent: delete a memory value.
    MemoryDelete {
        /// Key.
        key: String,
    },
    /// Owner: create an agent identity. The token is returned once.
    AgentCreate {
        /// Unique name.
        name: String,
        /// What it is for.
        #[serde(default)]
        description: String,
        /// Scopes, e.g. `read`, `propose`, `memory`, `run:daily-log`, `control:*`.
        scopes: Vec<String>,
    },
    /// Owner: list agents with activity counters.
    AgentList,
    /// Owner: revoke an agent (by id or name).
    AgentRevoke {
        /// Agent id or name.
        id: String,
    },
}

fn default_limit() -> usize {
    50
}

impl Request {
    /// Operation name, used in audit entries.
    pub fn op(&self) -> &'static str {
        match self {
            Request::Hello { .. } => "hello",
            Request::Health => "health",
            Request::ListWorkflows => "list_workflows",
            Request::Register { .. } => "register",
            Request::Enable { .. } => "enable",
            Request::Disable { .. } => "disable",
            Request::Pause { .. } => "pause",
            Request::Resume { .. } => "resume",
            Request::Cancel { .. } => "cancel",
            Request::RunNow { .. } => "run_now",
            Request::Revoke { .. } => "revoke",
            Request::Shutdown => "shutdown",
            Request::WhoAmI => "whoami",
            Request::History { .. } => "history",
            Request::Propose { .. } => "propose",
            Request::ProposalList => "proposal_list",
            Request::ProposalApprove { .. } => "proposal_approve",
            Request::ProposalReject { .. } => "proposal_reject",
            Request::MemoryGet { .. } => "memory_get",
            Request::MemoryPut { .. } => "memory_put",
            Request::MemoryList => "memory_list",
            Request::MemoryDelete { .. } => "memory_delete",
            Request::AgentCreate { .. } => "agent_create",
            Request::AgentList => "agent_list",
            Request::AgentRevoke { .. } => "agent_revoke",
        }
    }

    /// Whether this request changes state (and is therefore audited).
    pub fn is_control(&self) -> bool {
        !matches!(
            self,
            Request::Hello { .. }
                | Request::Health
                | Request::ListWorkflows
                | Request::WhoAmI
                | Request::History { .. }
                | Request::ProposalList
                | Request::MemoryGet { .. }
                | Request::MemoryList
                | Request::AgentList
        )
    }
}

/// Permission grant options sent with `Register`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GrantSpec {
    /// Pre-approve sensitive actions (trash, overwrite). In the background
    /// there is nobody to ask, so without this they are refused.
    pub allow_sensitive: bool,
}

/// Error categories.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// Missing or wrong token, wrong user, or no `Hello`.
    Unauthenticated,
    /// Authenticated, but this identity may not do that.
    Forbidden,
    /// Too many requests from this agent.
    RateLimited,
    /// Malformed request.
    BadRequest,
    /// Unknown workflow.
    NotFound,
    /// Workflow failed validation.
    Invalid,
    /// Permission grant missing or insufficient.
    Permission,
    /// Not allowed in the current state (e.g. already running).
    State,
    /// The service is shutting down.
    ShuttingDown,
    /// Anything else.
    Internal,
}

/// A response. Exactly one of `data` / `error` is set.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Response {
    /// Whether the request succeeded.
    pub ok: bool,
    /// Result payload.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
    /// Error details.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorBody>,
}

/// Error details.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorBody {
    /// Category.
    pub code: ErrorCode,
    /// Human-readable message.
    pub message: String,
}

impl Response {
    /// Success with a payload.
    pub fn ok(data: impl Serialize) -> Self {
        match serde_json::to_value(data) {
            Ok(v) => Self {
                ok: true,
                data: Some(v),
                error: None,
            },
            Err(e) => Self::err(ErrorCode::Internal, e.to_string()),
        }
    }

    /// Failure.
    pub fn err(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            ok: false,
            data: None,
            error: Some(ErrorBody {
                code,
                message: message.into(),
            }),
        }
    }
}

/// Reply to `Hello`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Welcome {
    /// Server protocol version.
    pub protocol: u32,
    /// Server version.
    pub version: String,
    /// Server pid.
    pub pid: u32,
}

/// Reply to `Health`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Health {
    /// `ok`, or `degraded` when something needs attention.
    pub status: String,
    /// Reasons for `degraded`.
    pub problems: Vec<String>,
    /// Service version.
    pub version: String,
    /// Process id.
    pub pid: u32,
    /// Seconds since start.
    pub uptime_secs: u64,
    /// Start time (RFC 3339).
    pub started_at: String,
    /// Data directory.
    pub data_dir: String,
    /// Whether the database answered a query.
    pub database_ok: bool,
    /// Workflow counts by state.
    pub workflows: StateCounts,
    /// Runs in progress right now.
    pub running: usize,
    /// Runs found marked `running` at start (left by a crash) and closed as failed.
    pub recovered_interrupted_runs: usize,
    /// Workflows that should have been restored but could not be.
    pub restore_failures: Vec<RestoreFailure>,
    /// Whether shutdown has begun.
    pub shutting_down: bool,
}

/// Counts of workflows by state.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateCounts {
    /// Enabled and firing.
    pub active: usize,
    /// Enabled but paused.
    pub paused: usize,
    /// Registered but disabled.
    pub inactive: usize,
    /// Cancelled.
    pub cancelled: usize,
}

/// A workflow that failed to restore at start.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RestoreFailure {
    /// Workflow id.
    pub id: String,
    /// Why.
    pub error: String,
}

/// One row of `ListWorkflows`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowInfo {
    /// Id.
    pub id: String,
    /// Display name.
    pub name: String,
    /// `manual`, `schedule` or `folder_watch`.
    pub trigger: String,
    /// `active`, `paused`, `inactive` or `cancelled`.
    pub state: String,
    /// A run is in progress.
    pub running: bool,
    /// Whether a permission grant is stored.
    pub granted: bool,
    /// Most recent run.
    pub last_run: Option<LastRun>,
}

/// Summary of the most recent run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LastRun {
    /// Execution id.
    pub id: String,
    /// Outcome.
    pub status: String,
    /// Start time (RFC 3339).
    pub started_at: String,
    /// Error, if any.
    pub error: Option<String>,
}

/// Reads one newline-terminated frame. `Ok(None)` on clean end of stream.
/// Frames longer than [`MAX_FRAME`] are an error (and nothing more is read
/// into memory than that limit).
pub async fn read_frame<R: AsyncBufRead + Unpin>(reader: &mut R) -> io::Result<Option<String>> {
    let mut buf = Vec::new();
    let n = (&mut *reader)
        .take(MAX_FRAME as u64 + 1)
        .read_until(b'\n', &mut buf)
        .await?;
    if n == 0 {
        return Ok(None);
    }
    if buf.last() != Some(&b'\n') {
        if buf.len() > MAX_FRAME {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "request too large",
            ));
        }
        return Ok(None); // peer closed mid-frame
    }
    buf.pop();
    String::from_utf8(buf)
        .map(Some)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "request is not UTF-8"))
}

/// Writes one frame.
pub async fn write_frame<W: AsyncWrite + Unpin, T: Serialize>(
    writer: &mut W,
    value: &T,
) -> io::Result<()> {
    let mut line = serde_json::to_vec(value)?;
    line.push(b'\n');
    writer.write_all(&line).await?;
    writer.flush().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_round_trip_and_reject_unknown_fields() -> Result<(), serde_json::Error> {
        let r = Request::Register {
            definition: "id = 'x'".into(),
            grant: Some(GrantSpec {
                allow_sensitive: false,
            }),
            replace: true,
        };
        let json = serde_json::to_string(&r)?;
        assert_eq!(serde_json::from_str::<Request>(&json)?, r);
        assert!(
            serde_json::from_str::<Request>(r#"{"op":"enable","id":"a","as_root":true}"#).is_err()
        );
        assert!(serde_json::from_str::<Request>(r#"{"op":"exec","cmd":"rm"}"#).is_err());
        Ok(())
    }

    #[test]
    fn control_ops_are_classified() {
        assert!(!Request::Health.is_control());
        assert!(Request::Shutdown.is_control());
        assert!(Request::Pause { id: "a".into() }.is_control());
    }
}
