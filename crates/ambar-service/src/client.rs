//! Client for the `ambard` IPC socket.

use std::io;

use ambar_automation::ExecutionRecord;
use serde::de::DeserializeOwned;
use tokio::io::BufReader;
use tokio::net::UnixStream;
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};

use crate::agents::{Agent, Proposal};
use crate::approvals::PendingApproval;
use crate::auth::read_token;
use crate::paths::ServicePaths;
use crate::protocol::{
    ErrorCode, GrantSpec, Health, PROTOCOL_VERSION, Request, Response, Welcome, WorkflowInfo,
    read_frame, write_frame,
};

/// Client errors.
#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    /// No service is listening.
    #[error("ambard is not running (no socket at {0})")]
    NotRunning(String),
    /// The service refused the request.
    #[error("{message}")]
    Remote {
        /// Category.
        code: ErrorCode,
        /// Message.
        message: String,
    },
    /// Transport failure.
    #[error("connection to ambard failed: {0}")]
    Io(#[from] io::Error),
    /// Unexpected reply.
    #[error("unexpected reply from ambard: {0}")]
    Protocol(String),
}

impl ClientError {
    /// The remote error code, if this is a remote error.
    pub fn code(&self) -> Option<ErrorCode> {
        match self {
            ClientError::Remote { code, .. } => Some(*code),
            _ => None,
        }
    }
}

/// An authenticated connection.
#[derive(Debug)]
pub struct Client {
    reader: BufReader<OwnedReadHalf>,
    writer: OwnedWriteHalf,
    welcome: Welcome,
}

impl Client {
    /// Connects and authenticates with the token in the data directory.
    pub async fn connect(paths: &ServicePaths, client_name: &str) -> Result<Self, ClientError> {
        let socket = paths.socket();
        let stream = match UnixStream::connect(&socket).await {
            Ok(s) => s,
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
                ) =>
            {
                return Err(ClientError::NotRunning(socket.display().to_string()));
            }
            Err(e) => return Err(e.into()),
        };
        let token = read_token(&paths.token())?;
        Self::handshake(stream, &token, client_name).await
    }

    /// Connects with an explicit token (used by tests).
    pub async fn connect_with_token(
        paths: &ServicePaths,
        token: &str,
        client_name: &str,
    ) -> Result<Self, ClientError> {
        let stream = UnixStream::connect(paths.socket()).await?;
        Self::handshake(stream, token, client_name).await
    }

    async fn handshake(stream: UnixStream, token: &str, name: &str) -> Result<Self, ClientError> {
        let (r, w) = stream.into_split();
        let mut client = Self {
            reader: BufReader::new(r),
            writer: w,
            welcome: Welcome {
                protocol: 0,
                version: String::new(),
                pid: 0,
            },
        };
        client.welcome = client
            .call(&Request::Hello {
                token: token.into(),
                protocol: PROTOCOL_VERSION,
                client: name.into(),
            })
            .await?;
        Ok(client)
    }

    /// Server details from the handshake.
    pub fn welcome(&self) -> &Welcome {
        &self.welcome
    }

    /// Sends a request and decodes the payload.
    pub async fn call<T: DeserializeOwned>(&mut self, request: &Request) -> Result<T, ClientError> {
        write_frame(&mut self.writer, request).await?;
        let line = read_frame(&mut self.reader)
            .await?
            .ok_or_else(|| ClientError::Protocol("connection closed".into()))?;
        let response: Response =
            serde_json::from_str(&line).map_err(|e| ClientError::Protocol(e.to_string()))?;
        if let Some(err) = response.error {
            return Err(ClientError::Remote {
                code: err.code,
                message: err.message,
            });
        }
        let data = response.data.unwrap_or(serde_json::Value::Null);
        serde_json::from_value(data).map_err(|e| ClientError::Protocol(e.to_string()))
    }

    async fn control(&mut self, request: Request) -> Result<(), ClientError> {
        self.call::<serde_json::Value>(&request).await.map(|_| ())
    }

    /// Service health.
    pub async fn health(&mut self) -> Result<Health, ClientError> {
        self.call(&Request::Health).await
    }
    /// Registered workflows.
    pub async fn list(&mut self) -> Result<Vec<WorkflowInfo>, ClientError> {
        self.call(&Request::ListWorkflows).await
    }
    /// Registers a workflow.
    pub async fn register(
        &mut self,
        definition: &str,
        grant: Option<GrantSpec>,
        replace: bool,
    ) -> Result<WorkflowInfo, ClientError> {
        self.call(&Request::Register {
            definition: definition.into(),
            grant,
            replace,
        })
        .await
    }
    /// Enables a workflow.
    pub async fn enable(&mut self, id: &str) -> Result<(), ClientError> {
        self.control(Request::Enable { id: id.into() }).await
    }
    /// Disables a workflow.
    pub async fn disable(&mut self, id: &str) -> Result<(), ClientError> {
        self.control(Request::Disable { id: id.into() }).await
    }
    /// Pauses a workflow.
    pub async fn pause(&mut self, id: &str) -> Result<(), ClientError> {
        self.control(Request::Pause { id: id.into() }).await
    }
    /// Resumes a workflow.
    pub async fn resume(&mut self, id: &str) -> Result<(), ClientError> {
        self.control(Request::Resume { id: id.into() }).await
    }
    /// Cancels a workflow.
    pub async fn cancel(&mut self, id: &str) -> Result<(), ClientError> {
        self.control(Request::Cancel { id: id.into() }).await
    }
    /// Removes a workflow (owner only).
    pub async fn remove(&mut self, id: &str) -> Result<(), ClientError> {
        self.control(Request::Remove { id: id.into() }).await
    }
    /// Withdraws a grant.
    pub async fn revoke(&mut self, id: &str) -> Result<(), ClientError> {
        self.control(Request::Revoke { id: id.into() }).await
    }
    /// Runs a workflow now and waits for the result.
    pub async fn run_now(&mut self, id: &str) -> Result<ExecutionRecord, ClientError> {
        self.call(&Request::RunNow { id: id.into() }).await
    }
    /// Asks the service to shut down.
    pub async fn shutdown(&mut self) -> Result<(), ClientError> {
        self.control(Request::Shutdown).await
    }

    // ---- identity & agents ----

    /// Identity and scopes of this connection.
    pub async fn whoami(&mut self) -> Result<serde_json::Value, ClientError> {
        self.call(&Request::WhoAmI).await
    }
    /// Recent executions.
    pub async fn history(
        &mut self,
        workflow: Option<&str>,
        limit: usize,
    ) -> Result<Vec<ExecutionRecord>, ClientError> {
        self.call(&Request::History {
            workflow: workflow.map(String::from),
            limit,
        })
        .await
    }
    /// Agent: propose a workflow for approval.
    pub async fn propose(
        &mut self,
        definition: &str,
        reason: &str,
    ) -> Result<Proposal, ClientError> {
        self.call(&Request::Propose {
            definition: definition.into(),
            reason: reason.into(),
        })
        .await
    }
    /// Proposals visible to this identity.
    pub async fn proposals(&mut self) -> Result<Vec<Proposal>, ClientError> {
        self.call(&Request::ProposalList).await
    }
    /// Owner: approve a proposal.
    pub async fn approve(
        &mut self,
        id: &str,
        allow_sensitive: bool,
        enable: bool,
    ) -> Result<Proposal, ClientError> {
        self.call(&Request::ProposalApprove {
            id: id.into(),
            allow_sensitive,
            enable,
        })
        .await
    }
    /// Owner: reject a proposal.
    pub async fn reject(&mut self, id: &str) -> Result<Proposal, ClientError> {
        self.call(&Request::ProposalReject { id: id.into() }).await
    }
    /// Agent: read memory.
    pub async fn memory_get(&mut self, key: &str) -> Result<Option<String>, ClientError> {
        self.call(&Request::MemoryGet { key: key.into() }).await
    }
    /// Agent: write memory.
    pub async fn memory_put(&mut self, key: &str, value: &str) -> Result<(), ClientError> {
        self.control(Request::MemoryPut {
            key: key.into(),
            value: value.into(),
        })
        .await
    }
    /// Agent: list memory keys.
    pub async fn memory_list(&mut self) -> Result<Vec<String>, ClientError> {
        self.call(&Request::MemoryList).await
    }
    /// Owner: create an agent. Returns the agent and its one-time token.
    pub async fn agent_create(
        &mut self,
        name: &str,
        description: &str,
        scopes: &[&str],
    ) -> Result<(Agent, String), ClientError> {
        let v: serde_json::Value = self
            .call(&Request::AgentCreate {
                name: name.into(),
                description: description.into(),
                scopes: scopes.iter().map(|s| s.to_string()).collect(),
            })
            .await?;
        let agent = serde_json::from_value(v["agent"].clone())
            .map_err(|e| ClientError::Protocol(e.to_string()))?;
        let token = v["token"]
            .as_str()
            .ok_or_else(|| ClientError::Protocol("missing token".into()))?
            .to_string();
        Ok((agent, token))
    }
    /// Owner: list agents.
    pub async fn agent_list(&mut self) -> Result<Vec<Agent>, ClientError> {
        self.call(&Request::AgentList).await
    }
    /// Owner: sensitive actions waiting for a decision.
    pub async fn approvals(&mut self) -> Result<Vec<PendingApproval>, ClientError> {
        self.call(&Request::ApprovalList).await
    }
    /// Owner: allow or deny a waiting sensitive action.
    pub async fn decide(&mut self, id: &str, allow: bool) -> Result<PendingApproval, ClientError> {
        self.call(&Request::ApprovalDecide { id: id.into(), allow }).await
    }
    /// Owner: revoke an agent.
    pub async fn agent_revoke(&mut self, id: &str) -> Result<Agent, ClientError> {
        self.call(&Request::AgentRevoke { id: id.into() }).await
    }
}
