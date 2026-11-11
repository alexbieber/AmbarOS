//! Client for the `ambard` IPC socket.

use std::io;

use ambar_automation::ExecutionRecord;
use serde::de::DeserializeOwned;
use tokio::io::BufReader;
use tokio::net::UnixStream;
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};

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
}
