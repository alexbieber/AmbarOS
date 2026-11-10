//! Agent identities, scopes, proposals and memory.
//!
//! The person who owns the service authenticates with the owner token (the
//! per-start file in `run/`). Everything else, AI agents included, gets an
//! **agent identity**: a name, a set of scopes and a long-lived token that is
//! shown once and stored only as a SHA-256 hash.
//!
//! Agents can never grant permissions, enable workflows, register workflows
//! directly, shut the service down or manage identities. To get a new
//! automation, an agent files a *proposal* that a human approves.

use std::collections::HashMap;
use std::fmt;
use std::path::Path;
use std::str::FromStr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use chrono::Utc;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::protocol::Request;

/// Max memory value size per key.
pub const MAX_MEMORY_VALUE: usize = 64 * 1024;
/// Max keys per agent.
pub const MAX_MEMORY_KEYS: i64 = 1000;
/// Max pending proposals per agent.
pub const MAX_PENDING_PROPOSALS: i64 = 20;
/// Requests allowed per agent per minute.
pub const RATE_LIMIT_PER_MINUTE: u32 = 120;

/// Which workflows a scope applies to.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Target {
    /// Every workflow.
    All,
    /// One workflow id.
    Id(String),
}

impl Target {
    fn matches(&self, id: &str) -> bool {
        match self {
            Target::All => true,
            Target::Id(t) => t == id,
        }
    }
}

/// A permission an agent can hold.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Scope {
    /// Health, workflow list, history.
    Read,
    /// Submit workflow proposals for human approval.
    Propose,
    /// Private key-value memory.
    Memory,
    /// Run already-approved, enabled workflows on demand.
    Run(Target),
    /// Pause, resume, disable and cancel workflows (never enable).
    Control(Target),
}

impl fmt::Display for Scope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let t = |t: &Target| match t {
            Target::All => "*".to_string(),
            Target::Id(i) => i.clone(),
        };
        match self {
            Scope::Read => f.write_str("read"),
            Scope::Propose => f.write_str("propose"),
            Scope::Memory => f.write_str("memory"),
            Scope::Run(x) => write!(f, "run:{}", t(x)),
            Scope::Control(x) => write!(f, "control:{}", t(x)),
        }
    }
}

impl FromStr for Scope {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        let target = |v: &str| -> Result<Target, String> {
            if v == "*" {
                Ok(Target::All)
            } else if !v.is_empty()
                && v.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
            {
                Ok(Target::Id(v.into()))
            } else {
                Err(format!("invalid workflow id `{v}` in scope"))
            }
        };
        match s.split_once(':') {
            None => match s {
                "read" => Ok(Scope::Read),
                "propose" => Ok(Scope::Propose),
                "memory" => Ok(Scope::Memory),
                _ => Err(format!(
                    "unknown scope `{s}` (expected read, propose, memory, run:<id|*>, control:<id|*>)"
                )),
            },
            Some(("run", v)) => Ok(Scope::Run(target(v)?)),
            Some(("control", v)) => Ok(Scope::Control(target(v)?)),
            _ => Err(format!("unknown scope `{s}`")),
        }
    }
}

impl Serialize for Scope {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for Scope {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        String::deserialize(d)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

/// A registered agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Agent {
    /// Stable id (`agent-<8 hex>`).
    pub id: String,
    /// Display name.
    pub name: String,
    /// What it is for.
    pub description: String,
    /// Granted scopes.
    pub scopes: Vec<Scope>,
    /// Creation time (RFC 3339).
    pub created_at: String,
    /// Last authenticated request (RFC 3339).
    pub last_seen: Option<String>,
    /// Requests served.
    pub requests: i64,
    /// Requests refused (scope or rate limit).
    pub denied: i64,
    /// Revoked agents cannot authenticate.
    pub revoked: bool,
}

/// Who is talking to the service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Principal {
    /// The person who owns the service (owner token).
    Owner,
    /// An agent identity.
    Agent(Agent),
}

impl Principal {
    /// Name used in audit entries.
    pub fn actor(&self) -> String {
        match self {
            Principal::Owner => "owner".into(),
            Principal::Agent(a) => format!("agent:{}", a.id),
        }
    }
}

/// Decides whether `principal` may make `request`. Pure function: every
/// rule is here and unit-tested.
pub fn authorize(principal: &Principal, request: &Request) -> Result<(), String> {
    let agent = match principal {
        Principal::Owner => {
            return match request {
                // Agent-only operations make no sense for the owner.
                Request::Propose { .. }
                | Request::MemoryGet { .. }
                | Request::MemoryPut { .. }
                | Request::MemoryList
                | Request::MemoryDelete { .. } => {
                    Err("this operation is for agent identities".into())
                }
                _ => Ok(()),
            };
        }
        Principal::Agent(a) => a,
    };
    let has = |want: &Scope| agent.scopes.contains(want);
    let targeted = |id: &str, run: bool| {
        agent.scopes.iter().any(|s| match s {
            Scope::Run(t) if run => t.matches(id),
            Scope::Control(t) if !run => t.matches(id),
            _ => false,
        })
    };
    let need = |ok: bool, what: &str| {
        if ok {
            Ok(())
        } else {
            Err(format!("agent `{}` lacks the `{what}` scope", agent.name))
        }
    };
    match request {
        Request::Hello { .. } | Request::WhoAmI => Ok(()),
        Request::Health | Request::ListWorkflows | Request::History { .. } => {
            need(has(&Scope::Read), "read")
        }
        Request::Propose { .. } | Request::ProposalList => need(has(&Scope::Propose), "propose"),
        Request::MemoryGet { .. }
        | Request::MemoryPut { .. }
        | Request::MemoryList
        | Request::MemoryDelete { .. } => need(has(&Scope::Memory), "memory"),
        Request::RunNow { id } => need(targeted(id, true), &format!("run:{id}")),
        Request::Pause { id }
        | Request::Resume { id }
        | Request::Disable { id }
        | Request::Cancel { id } => need(targeted(id, false), &format!("control:{id}")),
        // Owner-only, whatever scopes the agent holds.
        Request::Register { .. }
        | Request::Enable { .. }
        | Request::Revoke { .. }
        | Request::Remove { .. }
        | Request::Shutdown
        | Request::AgentCreate { .. }
        | Request::AgentList
        | Request::AgentRevoke { .. }
        | Request::ProposalApprove { .. }
        | Request::ProposalReject { .. } => Err(format!(
            "`{}` can only be done by the owner, not by agent `{}`",
            request.op(),
            agent.name
        )),
    }
}

/// A workflow proposed by an agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Proposal {
    /// Proposal id (`prop-<8 hex>`).
    pub id: String,
    /// Proposing agent.
    pub agent_id: String,
    /// Workflow id inside the definition.
    pub workflow_id: String,
    /// Workflow TOML.
    pub definition: String,
    /// The agent's explanation.
    pub reason: String,
    /// `pending`, `approved` or `rejected`.
    pub status: String,
    /// Risk review lines (`level: message`).
    pub risks: Vec<String>,
    /// Creation time.
    pub created_at: String,
    /// Decision time.
    pub decided_at: Option<String>,
}

/// Errors from the agent store.
#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    /// Database failure.
    #[error("agent store: {0}")]
    Db(#[from] rusqlite::Error),
    /// Bad input.
    #[error("{0}")]
    Invalid(String),
    /// Unknown id.
    #[error("{0} not found")]
    NotFound(String),
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS agents (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL UNIQUE,
    description TEXT NOT NULL,
    scopes      TEXT NOT NULL,
    token_hash  TEXT NOT NULL UNIQUE,
    created_at  TEXT NOT NULL,
    last_seen   TEXT,
    requests    INTEGER NOT NULL DEFAULT 0,
    denied      INTEGER NOT NULL DEFAULT 0,
    revoked     INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS agent_memory (
    agent_id   TEXT NOT NULL,
    key        TEXT NOT NULL,
    value      TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (agent_id, key)
);
CREATE TABLE IF NOT EXISTS proposals (
    id          TEXT PRIMARY KEY,
    agent_id    TEXT NOT NULL,
    workflow_id TEXT NOT NULL,
    definition  TEXT NOT NULL,
    reason      TEXT NOT NULL,
    status      TEXT NOT NULL,
    risks       TEXT NOT NULL,
    created_at  TEXT NOT NULL,
    decided_at  TEXT
);
"#;

fn hash_token(token: &str) -> String {
    Sha256::digest(token.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn short_id(prefix: &str) -> Result<String, AgentError> {
    let mut b = [0u8; 4];
    getrandom::fill(&mut b).map_err(|e| AgentError::Invalid(e.to_string()))?;
    Ok(format!(
        "{prefix}-{}",
        b.iter().map(|x| format!("{x:02x}")).collect::<String>()
    ))
}

/// Persistent agent registry (same SQLite file as the engine, own tables).
#[derive(Debug)]
pub struct AgentStore {
    conn: Mutex<Connection>,
    rate: Mutex<HashMap<String, (Instant, u32)>>,
}

fn row_agent(r: &rusqlite::Row<'_>) -> rusqlite::Result<Agent> {
    let scopes: String = r.get(3)?;
    Ok(Agent {
        id: r.get(0)?,
        name: r.get(1)?,
        description: r.get(2)?,
        scopes: serde_json::from_str(&scopes).unwrap_or_default(),
        created_at: r.get(4)?,
        last_seen: r.get(5)?,
        requests: r.get(6)?,
        denied: r.get(7)?,
        revoked: r.get::<_, i64>(8)? != 0,
    })
}

const AGENT_COLS: &str =
    "id, name, description, scopes, created_at, last_seen, requests, denied, revoked";

impl AgentStore {
    /// Opens the agent tables in `db`.
    pub fn open(db: &Path) -> Result<Self, AgentError> {
        let conn = Connection::open(db)?;
        conn.busy_timeout(Duration::from_secs(5))?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self {
            conn: Mutex::new(conn),
            rate: Mutex::new(HashMap::new()),
        })
    }

    fn conn(&self) -> Result<std::sync::MutexGuard<'_, Connection>, AgentError> {
        self.conn
            .lock()
            .map_err(|_| AgentError::Invalid("agent store lock poisoned".into()))
    }

    /// Creates an agent and returns it with its token (shown once, never stored).
    pub fn create(
        &self,
        name: &str,
        description: &str,
        scopes: Vec<Scope>,
    ) -> Result<(Agent, String), AgentError> {
        let name = name.trim();
        if name.is_empty() || name.len() > 64 {
            return Err(AgentError::Invalid(
                "agent name must be 1-64 characters".into(),
            ));
        }
        if scopes.is_empty() {
            return Err(AgentError::Invalid(
                "give the agent at least one scope".into(),
            ));
        }
        let token = format!(
            "ambar_agent_{}",
            crate::auth::generate_token().map_err(|e| AgentError::Invalid(e.to_string()))?
        );
        let id = short_id("agent")?;
        let now = Utc::now().to_rfc3339();
        let scopes_json =
            serde_json::to_string(&scopes).map_err(|e| AgentError::Invalid(e.to_string()))?;
        self.conn()?
            .execute(
                "INSERT INTO agents (id, name, description, scopes, token_hash, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![id, name, description, scopes_json, hash_token(&token), now],
            )
            .map_err(|e| match e {
                rusqlite::Error::SqliteFailure(f, _)
                    if f.code == rusqlite::ErrorCode::ConstraintViolation =>
                {
                    AgentError::Invalid(format!("an agent named `{name}` already exists"))
                }
                e => e.into(),
            })?;
        let agent = self.get(&id)?;
        Ok((agent, token))
    }

    /// Looks up a non-revoked agent by token.
    pub fn authenticate(&self, token: &str) -> Result<Option<Agent>, AgentError> {
        if !token.starts_with("ambar_agent_") {
            return Ok(None);
        }
        let conn = self.conn()?;
        let agent = conn
            .query_row(
                &format!("SELECT {AGENT_COLS} FROM agents WHERE token_hash = ?1 AND revoked = 0"),
                params![hash_token(token)],
                row_agent,
            )
            .optional()?;
        Ok(agent)
    }

    /// One agent.
    pub fn get(&self, id: &str) -> Result<Agent, AgentError> {
        self.conn()?
            .query_row(
                &format!("SELECT {AGENT_COLS} FROM agents WHERE id = ?1"),
                params![id],
                row_agent,
            )
            .optional()?
            .ok_or_else(|| AgentError::NotFound(format!("agent `{id}`")))
    }

    /// All agents.
    pub fn list(&self) -> Result<Vec<Agent>, AgentError> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(&format!(
            "SELECT {AGENT_COLS} FROM agents ORDER BY created_at"
        ))?;
        let rows = stmt
            .query_map([], row_agent)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Revokes an agent (by id or name). Its token stops working immediately.
    pub fn revoke(&self, id_or_name: &str) -> Result<Agent, AgentError> {
        let n = self.conn()?.execute(
            "UPDATE agents SET revoked = 1 WHERE id = ?1 OR name = ?1",
            params![id_or_name],
        )?;
        if n == 0 {
            return Err(AgentError::NotFound(format!("agent `{id_or_name}`")));
        }
        let conn = self.conn()?;
        Ok(conn.query_row(
            &format!("SELECT {AGENT_COLS} FROM agents WHERE id = ?1 OR name = ?1"),
            params![id_or_name],
            row_agent,
        )?)
    }

    /// Records a request and enforces the per-agent rate limit.
    pub fn record(&self, agent_id: &str, allowed: bool) -> Result<bool, AgentError> {
        let within_limit = {
            let mut rate = self
                .rate
                .lock()
                .map_err(|_| AgentError::Invalid("rate lock poisoned".into()))?;
            let now = Instant::now();
            let entry = rate.entry(agent_id.to_string()).or_insert((now, 0));
            if now.duration_since(entry.0) >= Duration::from_secs(60) {
                *entry = (now, 0);
            }
            entry.1 += 1;
            entry.1 <= RATE_LIMIT_PER_MINUTE
        };
        let ok = allowed && within_limit;
        self.conn()?.execute(
            "UPDATE agents SET last_seen = ?2, requests = requests + 1,
             denied = denied + ?3 WHERE id = ?1",
            params![agent_id, Utc::now().to_rfc3339(), i64::from(!ok)],
        )?;
        Ok(within_limit)
    }

    // ---- memory ----

    /// Reads a memory value.
    pub fn memory_get(&self, agent: &str, key: &str) -> Result<Option<String>, AgentError> {
        Ok(self
            .conn()?
            .query_row(
                "SELECT value FROM agent_memory WHERE agent_id = ?1 AND key = ?2",
                params![agent, key],
                |r| r.get(0),
            )
            .optional()?)
    }

    /// Writes a memory value, enforcing size limits.
    pub fn memory_put(&self, agent: &str, key: &str, value: &str) -> Result<(), AgentError> {
        if key.is_empty() || key.len() > 128 {
            return Err(AgentError::Invalid(
                "memory keys must be 1-128 characters".into(),
            ));
        }
        if value.len() > MAX_MEMORY_VALUE {
            return Err(AgentError::Invalid(format!(
                "memory values are limited to {MAX_MEMORY_VALUE} bytes"
            )));
        }
        let conn = self.conn()?;
        let exists: bool = conn
            .query_row(
                "SELECT 1 FROM agent_memory WHERE agent_id = ?1 AND key = ?2",
                params![agent, key],
                |_| Ok(true),
            )
            .optional()?
            .unwrap_or(false);
        if !exists {
            let count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM agent_memory WHERE agent_id = ?1",
                params![agent],
                |r| r.get(0),
            )?;
            if count >= MAX_MEMORY_KEYS {
                return Err(AgentError::Invalid(format!(
                    "memory is limited to {MAX_MEMORY_KEYS} keys per agent"
                )));
            }
        }
        conn.execute(
            "INSERT INTO agent_memory (agent_id, key, value, updated_at) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(agent_id, key) DO UPDATE SET value = ?3, updated_at = ?4",
            params![agent, key, value, Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    /// Lists memory keys.
    pub fn memory_keys(&self, agent: &str) -> Result<Vec<String>, AgentError> {
        let conn = self.conn()?;
        let mut stmt =
            conn.prepare("SELECT key FROM agent_memory WHERE agent_id = ?1 ORDER BY key")?;
        let rows = stmt
            .query_map(params![agent], |r| r.get(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Deletes a memory value.
    pub fn memory_delete(&self, agent: &str, key: &str) -> Result<bool, AgentError> {
        Ok(self.conn()?.execute(
            "DELETE FROM agent_memory WHERE agent_id = ?1 AND key = ?2",
            params![agent, key],
        )? > 0)
    }

    // ---- proposals ----

    /// Files a proposal.
    pub fn propose(
        &self,
        agent: &str,
        workflow_id: &str,
        definition: &str,
        reason: &str,
        risks: Vec<String>,
    ) -> Result<Proposal, AgentError> {
        let pending: i64 = self.conn()?.query_row(
            "SELECT COUNT(*) FROM proposals WHERE agent_id = ?1 AND status = 'pending'",
            params![agent],
            |r| r.get(0),
        )?;
        if pending >= MAX_PENDING_PROPOSALS {
            return Err(AgentError::Invalid(format!(
                "at most {MAX_PENDING_PROPOSALS} pending proposals per agent"
            )));
        }
        let id = short_id("prop")?;
        let risks_json = serde_json::to_string(&risks).unwrap_or_else(|_| "[]".into());
        self.conn()?.execute(
            "INSERT INTO proposals (id, agent_id, workflow_id, definition, reason, status, risks, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 'pending', ?6, ?7)",
            params![id, agent, workflow_id, definition, reason, risks_json, Utc::now().to_rfc3339()],
        )?;
        self.proposal(&id)
    }

    fn row_proposal(r: &rusqlite::Row<'_>) -> rusqlite::Result<Proposal> {
        let risks: String = r.get(6)?;
        Ok(Proposal {
            id: r.get(0)?,
            agent_id: r.get(1)?,
            workflow_id: r.get(2)?,
            definition: r.get(3)?,
            reason: r.get(4)?,
            status: r.get(5)?,
            risks: serde_json::from_str(&risks).unwrap_or_default(),
            created_at: r.get(7)?,
            decided_at: r.get(8)?,
        })
    }

    /// One proposal.
    pub fn proposal(&self, id: &str) -> Result<Proposal, AgentError> {
        self.conn()?
            .query_row(
                "SELECT id, agent_id, workflow_id, definition, reason, status, risks, created_at, decided_at
                 FROM proposals WHERE id = ?1",
                params![id],
                Self::row_proposal,
            )
            .optional()?
            .ok_or_else(|| AgentError::NotFound(format!("proposal `{id}`")))
    }

    /// Proposals, newest first; optionally only one agent's.
    pub fn proposals(&self, agent: Option<&str>) -> Result<Vec<Proposal>, AgentError> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT id, agent_id, workflow_id, definition, reason, status, risks, created_at, decided_at
             FROM proposals WHERE (?1 IS NULL OR agent_id = ?1) ORDER BY created_at DESC LIMIT 200",
        )?;
        let rows = stmt
            .query_map(params![agent], Self::row_proposal)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Marks a pending proposal approved or rejected.
    pub fn decide(&self, id: &str, approved: bool) -> Result<Proposal, AgentError> {
        let n = self.conn()?.execute(
            "UPDATE proposals SET status = ?2, decided_at = ?3 WHERE id = ?1 AND status = 'pending'",
            params![id, if approved { "approved" } else { "rejected" }, Utc::now().to_rfc3339()],
        )?;
        if n == 0 {
            let p = self.proposal(id)?;
            return Err(AgentError::Invalid(format!(
                "proposal `{id}` is already {}",
                p.status
            )));
        }
        self.proposal(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agent(scopes: &[&str]) -> Principal {
        Principal::Agent(Agent {
            id: "agent-1".into(),
            name: "helper".into(),
            description: String::new(),
            scopes: scopes
                .iter()
                .map(|s| s.parse().unwrap_or(Scope::Read))
                .collect(),
            created_at: String::new(),
            last_seen: None,
            requests: 0,
            denied: 0,
            revoked: false,
        })
    }

    fn id(s: &str) -> String {
        s.to_string()
    }

    #[test]
    fn scope_parsing() {
        assert_eq!("run:*".parse::<Scope>(), Ok(Scope::Run(Target::All)));
        assert_eq!(
            "control:a-b".parse::<Scope>(),
            Ok(Scope::Control(Target::Id("a-b".into())))
        );
        assert!("admin".parse::<Scope>().is_err());
        assert!("run:".parse::<Scope>().is_err());
        assert!("run:../x".parse::<Scope>().is_err());
        assert_eq!(Scope::Run(Target::All).to_string(), "run:*");
    }

    #[test]
    fn owner_can_do_everything_except_agent_ops() {
        let o = Principal::Owner;
        assert!(authorize(&o, &Request::Shutdown).is_ok());
        assert!(authorize(&o, &Request::Enable { id: id("x") }).is_ok());
        assert!(authorize(&o, &Request::MemoryList).is_err());
    }

    #[test]
    fn agents_never_get_owner_operations() {
        let all = agent(&["read", "propose", "memory", "run:*", "control:*"]);
        for r in [
            Request::Shutdown,
            Request::Enable { id: id("x") },
            Request::Revoke { id: id("x") },
            Request::Register {
                definition: String::new(),
                grant: None,
                replace: false,
            },
            Request::AgentCreate {
                name: id("x"),
                description: String::new(),
                scopes: vec![],
            },
            Request::AgentList,
            Request::AgentRevoke { id: id("x") },
            Request::ProposalApprove {
                id: id("p"),
                allow_sensitive: false,
                enable: false,
            },
            Request::ProposalReject { id: id("p") },
        ] {
            assert!(authorize(&all, &r).is_err(), "{r:?} must be owner-only");
        }
    }

    #[test]
    fn scopes_are_targeted() {
        let a = agent(&["run:daily-log", "control:daily-log"]);
        assert!(
            authorize(
                &a,
                &Request::RunNow {
                    id: id("daily-log")
                }
            )
            .is_ok()
        );
        assert!(authorize(&a, &Request::RunNow { id: id("other") }).is_err());
        assert!(
            authorize(
                &a,
                &Request::Pause {
                    id: id("daily-log")
                }
            )
            .is_ok()
        );
        assert!(authorize(&a, &Request::Cancel { id: id("other") }).is_err());
        assert!(
            authorize(&a, &Request::ListWorkflows).is_err(),
            "no read scope"
        );
        assert!(authorize(&a, &Request::WhoAmI).is_ok());
        let r = agent(&["read"]);
        assert!(authorize(&r, &Request::Health).is_ok());
        assert!(authorize(&r, &Request::RunNow { id: id("x") }).is_err());
        assert!(
            authorize(
                &r,
                &Request::Propose {
                    definition: String::new(),
                    reason: String::new()
                }
            )
            .is_err()
        );
    }

    #[test]
    fn tokens_hash_and_revoke() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let s = AgentStore::open(&dir.path().join("a.db"))?;
        let (a, token) = s.create("helper", "", vec![Scope::Read])?;
        assert!(token.starts_with("ambar_agent_"));
        assert_eq!(s.authenticate(&token)?.map(|x| x.id), Some(a.id.clone()));
        assert_eq!(s.authenticate("ambar_agent_wrong")?, None);
        assert!(
            s.create("helper", "", vec![Scope::Read]).is_err(),
            "unique names"
        );
        // The token itself is not stored.
        let db = std::fs::read(dir.path().join("a.db"))?;
        let needle = token.as_bytes();
        assert!(!db.windows(needle.len()).any(|w| w == needle));
        s.revoke("helper")?;
        assert_eq!(s.authenticate(&token)?, None);
        Ok(())
    }

    #[test]
    fn memory_limits() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let s = AgentStore::open(&dir.path().join("a.db"))?;
        s.memory_put("a", "k", "v")?;
        assert_eq!(s.memory_get("a", "k")?.as_deref(), Some("v"));
        assert_eq!(s.memory_get("b", "k")?, None, "memory is per agent");
        assert!(
            s.memory_put("a", "big", &"x".repeat(MAX_MEMORY_VALUE + 1))
                .is_err()
        );
        assert!(s.memory_put("a", "", "v").is_err());
        assert!(s.memory_delete("a", "k")?);
        Ok(())
    }

    #[test]
    fn rate_limit() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let s = AgentStore::open(&dir.path().join("a.db"))?;
        let (a, _) = s.create("busy", "", vec![Scope::Read])?;
        for _ in 0..RATE_LIMIT_PER_MINUTE {
            assert!(s.record(&a.id, true)?);
        }
        assert!(!s.record(&a.id, true)?);
        let a = s.get(&a.id)?;
        assert_eq!(a.requests, i64::from(RATE_LIMIT_PER_MINUTE) + 1);
        assert_eq!(a.denied, 1);
        Ok(())
    }
}
