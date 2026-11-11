//! The `ambard` daemon: owns one [`Engine`], restores enabled workflows at
//! start, serves the IPC socket, and shuts down gracefully.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ambar_automation::{
    Approver, Engine, EngineConfig, EngineError, Notifier, ShutdownReport, Store, Workflow,
    WorkflowState,
};
use chrono::{DateTime, Utc};
use tokio::io::BufReader;
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::Semaphore;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::agents::{AgentError, AgentStore, Principal, Scope, authorize};
use crate::approvals::ApprovalBroker;
use crate::auth::{authorize_peer, constant_time_eq, generate_token, write_token};
use crate::paths::{InstanceLock, PathError, ServicePaths, current_uid};
use crate::protocol::{
    ErrorCode, Health, LastRun, PROTOCOL_VERSION, Request, Response, RestoreFailure, StateCounts,
    Welcome, WorkflowInfo, read_frame, write_frame,
};

/// Time a client has to send `Hello` after connecting.
pub const HELLO_TIMEOUT: Duration = Duration::from_secs(5);
/// Idle connections are closed after this long.
pub const IDLE_TIMEOUT: Duration = Duration::from_secs(300);
/// Maximum simultaneous client connections.
pub const MAX_CONNECTIONS: usize = 64;

/// Service-level audit entries use this in place of a workflow id.
pub const SERVICE_AUDIT_ID: &str = "-";

/// Daemon settings.
#[derive(Debug, Clone)]
pub struct DaemonConfig {
    /// Data directory layout.
    pub paths: ServicePaths,
    /// Engine settings.
    pub engine: EngineConfig,
    /// Notification backend.
    pub notifier: Arc<dyn Notifier>,
    /// Approver for sensitive actions. In the background this should normally
    /// be [`ambar_automation::DenyAll`]: sensitive actions then need
    /// `allow_sensitive` on the workflow's grant.
    pub approver: Arc<dyn Approver>,
    /// How long shutdown waits for in-progress runs.
    pub grace: Duration,
    /// When set, sensitive actions that are not pre-approved wait this long
    /// for the owner to allow them (live approvals) instead of using
    /// `approver`. No answer means deny.
    pub live_approvals: Option<Duration>,
}

impl DaemonConfig {
    /// Defaults for a data directory.
    pub fn new(
        paths: ServicePaths,
        notifier: Arc<dyn Notifier>,
        approver: Arc<dyn Approver>,
    ) -> Self {
        Self {
            engine: EngineConfig::new(&paths.data_dir),
            paths,
            notifier,
            approver,
            grace: Duration::from_secs(30),
            live_approvals: None,
        }
    }
}

/// Errors starting the daemon.
#[derive(Debug, thiserror::Error)]
pub enum DaemonError {
    /// Path, permission or lock problem.
    #[error(transparent)]
    Path(#[from] PathError),
    /// Engine or database problem.
    #[error(transparent)]
    Engine(#[from] EngineError),
    /// Socket problem.
    #[error("could not listen on {path}: {source}")]
    Bind {
        /// Socket path.
        path: String,
        /// Underlying error.
        source: std::io::Error,
    },
    /// Random number generator failure.
    #[error("could not generate an auth token: {0}")]
    Token(String),
    /// Agent registry problem.
    #[error(transparent)]
    Agents(#[from] AgentError),
}

#[derive(Debug)]
struct Shared {
    engine: Engine,
    paths: ServicePaths,
    uid: u32,
    token: String,
    started: Instant,
    started_at: DateTime<Utc>,
    recovered: usize,
    restore_failures: Mutex<Vec<RestoreFailure>>,
    shutdown: CancellationToken,
    agents: AgentStore,
    broker: Option<Arc<ApprovalBroker>>,
}

/// A started daemon.
#[derive(Debug)]
pub struct RunningDaemon {
    shared: Arc<Shared>,
    task: JoinHandle<ShutdownReport>,
}

impl RunningDaemon {
    /// Cancelling this token starts a graceful shutdown.
    pub fn shutdown_token(&self) -> CancellationToken {
        self.shared.shutdown.clone()
    }

    /// The engine (for embedding and tests).
    pub fn engine(&self) -> &Engine {
        &self.shared.engine
    }

    /// Paths in use.
    pub fn paths(&self) -> &ServicePaths {
        &self.shared.paths
    }

    /// Waits until the daemon has shut down (after the token is cancelled).
    pub async fn wait(self) -> ShutdownReport {
        self.task.await.unwrap_or(ShutdownReport { interrupted: 0 })
    }

    /// Requests shutdown and waits for it to complete.
    pub async fn stop(self) -> ShutdownReport {
        self.shared.shutdown.cancel();
        self.wait().await
    }
}

/// Starts the daemon: lock, recover, restore, listen.
pub async fn start(config: DaemonConfig) -> Result<RunningDaemon, DaemonError> {
    let paths = config.paths.clone();
    let lock = paths.acquire_lock()?;

    // We hold the lock, so any socket or token on disk is stale.
    let _ = std::fs::remove_file(paths.socket());
    let _ = std::fs::remove_file(paths.token());

    let store = Store::open(&paths.db())?;
    let interrupted_before = store
        .executions(None, 10_000)
        .map(|r| r.iter().filter(|e| e.status.as_str() == "running").count())
        .unwrap_or(0);
    let broker = config.live_approvals.map(|timeout| {
        Arc::new(ApprovalBroker::new(timeout, config.notifier.clone(), store.clone()))
    });
    let approver: Arc<dyn ambar_automation::Approver> = match &broker {
        Some(b) => b.clone(),
        None => config.approver.clone(),
    };
    let engine = Engine::new(store, config.notifier, approver, config.engine)?;

    let token = generate_token().map_err(|e| DaemonError::Token(e.to_string()))?;
    write_token(&paths.token(), &token)?;

    let shared = Arc::new(Shared {
        engine,
        paths: paths.clone(),
        uid: current_uid(),
        token,
        started: Instant::now(),
        started_at: Utc::now(),
        recovered: interrupted_before,
        restore_failures: Mutex::new(Vec::new()),
        shutdown: CancellationToken::new(),
        agents: AgentStore::open(&paths.db())?,
        broker,
    });

    let restored = restore(&shared);

    let socket = paths.socket();
    let listener = UnixListener::bind(&socket).map_err(|source| DaemonError::Bind {
        path: socket.display().to_string(),
        source,
    })?;
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600)).map_err(
            |source| DaemonError::Bind {
                path: socket.display().to_string(),
                source,
            },
        )?;
    }

    let failures = shared.restore_failures.lock().map(|f| f.len()).unwrap_or(0);
    service_audit(
        &shared,
        "service.started",
        &format!(
            "pid={} restored={restored} restore_failures={failures} recovered_interrupted_runs={}",
            std::process::id(),
            shared.recovered
        ),
    );
    tracing::info!(
        socket = %socket.display(),
        restored,
        failures,
        recovered = shared.recovered,
        "ambard started"
    );

    let task = tokio::spawn(serve(shared.clone(), listener, lock, config.grace));
    Ok(RunningDaemon { shared, task })
}

fn service_audit(shared: &Shared, event: &str, detail: &str) {
    if let Err(e) = shared.engine.store().audit(SERVICE_AUDIT_ID, event, detail) {
        tracing::error!(error = %e, "could not write audit entry");
    }
}

/// Re-registers every stored workflow and re-enables those that were active
/// or paused. Returns how many were re-enabled.
fn restore(shared: &Shared) -> usize {
    let engine = &shared.engine;
    let stored = match engine.store().list_workflows() {
        Ok(s) => s,
        Err(e) => {
            record_failure(shared, "*", &format!("could not read workflows: {e}"));
            return 0;
        }
    };
    let mut restored = 0;
    for (id, definition, state) in stored {
        let saved: WorkflowState = state.parse().unwrap_or(WorkflowState::Inactive);
        let result = (|| -> Result<bool, EngineError> {
            let wf = Workflow::from_toml(&definition)?;
            engine.register(wf)?;
            match saved {
                WorkflowState::Active => engine.activate(&id).map(|()| true),
                WorkflowState::Paused => engine
                    .activate(&id)
                    .and_then(|()| engine.pause(&id))
                    .map(|()| true),
                // Registered (listable) but not running.
                WorkflowState::Inactive | WorkflowState::Cancelled => Ok(false),
            }
        })();
        match result {
            Ok(true) => restored += 1,
            Ok(false) => {}
            Err(e) => {
                record_failure(shared, &id, &e.to_string());
                // Keep the desired state, so the next start tries again (for
                // example once an unplugged drive is back).
                let _ = engine.store().set_workflow_state(&id, saved.as_str());
            }
        }
    }
    restored
}

fn record_failure(shared: &Shared, id: &str, error: &str) {
    tracing::warn!(workflow = %id, error, "could not restore workflow");
    let _ = shared
        .engine
        .store()
        .audit(id, "service.restore_failed", error);
    if let Ok(mut f) = shared.restore_failures.lock() {
        f.push(RestoreFailure {
            id: id.into(),
            error: error.into(),
        });
    }
}

async fn serve(
    shared: Arc<Shared>,
    listener: UnixListener,
    lock: InstanceLock,
    grace: Duration,
) -> ShutdownReport {
    let permits = Arc::new(Semaphore::new(MAX_CONNECTIONS));
    loop {
        tokio::select! {
            _ = shared.shutdown.cancelled() => break,
            accepted = listener.accept() => match accepted {
                Ok((stream, _)) => match permits.clone().try_acquire_owned() {
                    Ok(permit) => {
                        let shared = shared.clone();
                        tokio::spawn(async move {
                            handle_connection(shared, stream).await;
                            drop(permit);
                        });
                    }
                    Err(_) => {
                        tracing::warn!("too many connections; refusing one");
                        drop(stream);
                    }
                },
                Err(e) => {
                    tracing::warn!(error = %e, "accept failed");
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            }
        }
    }

    tracing::info!("shutting down");
    drop(listener);
    let _ = std::fs::remove_file(shared.paths.socket());
    let _ = std::fs::remove_file(shared.paths.token());
    if let Some(b) = &shared.broker {
        b.close(); // waiting approvals are denied so runs can finish
    }
    let report = shared.engine.shutdown(grace).await;
    service_audit(
        &shared,
        "service.stopped",
        &format!("graceful=true interrupted_runs={}", report.interrupted),
    );
    tracing::info!(interrupted = report.interrupted, "ambard stopped");
    drop(lock);
    report
}

async fn handle_connection(shared: Arc<Shared>, stream: UnixStream) {
    let (peer_uid, peer_pid) = match stream.peer_cred() {
        Ok(c) => (c.uid(), c.pid()),
        Err(e) => {
            tracing::warn!(error = %e, "could not read peer credentials");
            return;
        }
    };
    let pid = peer_pid
        .map(|p| p.to_string())
        .unwrap_or_else(|| "?".into());
    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);

    let deny = |reason: &str| {
        service_audit(
            &shared,
            "ipc.denied",
            &format!("pid={pid} uid={peer_uid} reason={reason}"),
        );
        Response::err(ErrorCode::Unauthenticated, reason)
    };

    if let Err(msg) = authorize_peer(peer_uid, shared.uid) {
        let _ = write_frame(&mut write_half, &deny(&msg)).await;
        return;
    }

    // Authenticate.
    let (client, principal) = match tokio::time::timeout(HELLO_TIMEOUT, read_frame(&mut reader))
        .await
    {
        Ok(Ok(Some(line))) => match serde_json::from_str::<Request>(&line) {
            Ok(Request::Hello {
                token,
                protocol,
                client,
            }) => {
                let principal = if constant_time_eq(&token, &shared.token) {
                    Principal::Owner
                } else {
                    match shared.agents.authenticate(&token) {
                        Ok(Some(agent)) => Principal::Agent(agent),
                        _ => {
                            let _ = write_frame(&mut write_half, &deny("invalid token")).await;
                            return;
                        }
                    }
                };
                if protocol != PROTOCOL_VERSION {
                    let msg =
                        format!("protocol mismatch: client {protocol}, service {PROTOCOL_VERSION}");
                    let _ =
                        write_frame(&mut write_half, &Response::err(ErrorCode::BadRequest, msg))
                            .await;
                    return;
                }
                (client, principal)
            }
            Ok(_) => {
                let _ = write_frame(&mut write_half, &deny("first request must be hello")).await;
                return;
            }
            Err(_) => {
                let _ = write_frame(&mut write_half, &deny("malformed hello")).await;
                return;
            }
        },
        Ok(Ok(None)) => return,
        Ok(Err(e)) => {
            let _ = write_frame(&mut write_half, &deny(&e.to_string())).await;
            return;
        }
        Err(_) => {
            let _ = write_frame(&mut write_half, &deny("hello timed out")).await;
            return;
        }
    };
    let welcome = Welcome {
        protocol: PROTOCOL_VERSION,
        version: env!("CARGO_PKG_VERSION").into(),
        pid: std::process::id(),
    };
    if write_frame(&mut write_half, &Response::ok(welcome))
        .await
        .is_err()
    {
        return;
    }

    // Serve requests.
    loop {
        let line = match tokio::time::timeout(IDLE_TIMEOUT, read_frame(&mut reader)).await {
            Ok(Ok(Some(line))) => line,
            Ok(Err(e)) => {
                let _ = write_frame(
                    &mut write_half,
                    &Response::err(ErrorCode::BadRequest, e.to_string()),
                )
                .await;
                return;
            }
            _ => return,
        };
        let (response, stop) = match serde_json::from_str::<Request>(&line) {
            Ok(Request::Hello { .. }) => (
                Response::err(ErrorCode::BadRequest, "already authenticated"),
                false,
            ),
            Ok(request) => {
                let is_shutdown = matches!(request, Request::Shutdown);
                let response = dispatch(&shared, &principal, request, &client, &pid).await;
                let stop = is_shutdown && response.ok;
                (response, stop)
            }
            Err(e) => (
                Response::err(ErrorCode::BadRequest, format!("invalid request: {e}")),
                false,
            ),
        };
        if write_frame(&mut write_half, &response).await.is_err() {
            return;
        }
        if stop {
            shared.shutdown.cancel();
            return;
        }
    }
}

fn engine_error(e: &EngineError) -> Response {
    let code = match e {
        EngineError::Invalid(_) | EngineError::Parse(_) => ErrorCode::Invalid,
        EngineError::NotFound(_) => ErrorCode::NotFound,
        EngineError::Permission(_) => ErrorCode::Permission,
        EngineError::State(_) | EngineError::Trigger(_) => ErrorCode::State,
        EngineError::Storage(_) | EngineError::Io(_) => ErrorCode::Internal,
    };
    Response::err(code, e.to_string())
}

async fn dispatch(
    shared: &Shared,
    principal: &Principal,
    request: Request,
    client: &str,
    pid: &str,
) -> Response {
    let engine = &shared.engine;
    let op = request.op();
    let actor = principal.actor();
    let target = target_of(&request);

    // Re-read agent identities on every request: revocation and scope changes
    // apply to connections that are already open.
    let fresh;
    let principal = match principal {
        Principal::Owner => principal,
        Principal::Agent(a) => match shared.agents.get(&a.id) {
            Ok(current) if !current.revoked => {
                fresh = Principal::Agent(current);
                &fresh
            }
            _ => {
                let _ = engine.store().audit(
                    SERVICE_AUDIT_ID,
                    "ipc.denied",
                    &format!("actor={actor} client={client} pid={pid} reason=agent revoked"),
                );
                return Response::err(ErrorCode::Unauthenticated, "this agent has been revoked");
            }
        },
    };

    // Authorization, then per-agent rate limiting.
    let allowed = authorize(principal, &request);
    if let Principal::Agent(a) = principal {
        match shared.agents.record(&a.id, allowed.is_ok()) {
            Ok(true) => {}
            Ok(false) => {
                let _ = engine.store().audit(
                    SERVICE_AUDIT_ID,
                    "ipc.rate_limited",
                    &format!("actor={actor} client={client} pid={pid} op={op}"),
                );
                return Response::err(
                    ErrorCode::RateLimited,
                    format!(
                        "agent `{}` exceeded {} requests per minute",
                        a.name,
                        crate::agents::RATE_LIMIT_PER_MINUTE
                    ),
                );
            }
            Err(e) => return Response::err(ErrorCode::Internal, e.to_string()),
        }
    }
    if let Err(msg) = allowed {
        let _ = engine.store().audit(
            SERVICE_AUDIT_ID,
            "ipc.forbidden",
            &format!("actor={actor} client={client} pid={pid} op={op}{target} reason={msg}"),
        );
        return Response::err(ErrorCode::Forbidden, msg);
    }

    let audit_id = match &request {
        Request::Enable { id }
        | Request::Disable { id }
        | Request::Pause { id }
        | Request::Resume { id }
        | Request::Cancel { id }
        | Request::RunNow { id }
        | Request::Revoke { id }
        | Request::Remove { id } => id.clone(),
        Request::Register { definition, .. } => Workflow::from_toml(definition)
            .map(|w| w.id)
            .unwrap_or_else(|_| SERVICE_AUDIT_ID.into()),
        _ => SERVICE_AUDIT_ID.into(),
    };
    let is_control = request.is_control();

    if is_control && engine.is_shutting_down() && !matches!(request, Request::Shutdown) {
        return Response::err(ErrorCode::ShuttingDown, "the service is shutting down");
    }

    let response = match request {
        Request::Hello { .. } => Response::err(ErrorCode::BadRequest, "already authenticated"),
        Request::Health => Response::ok(health(shared)),
        Request::ListWorkflows => match list(shared) {
            Ok(l) => Response::ok(l),
            Err(e) => engine_error(&e),
        },
        Request::Register {
            definition,
            grant,
            replace,
        } => match register(shared, &definition, grant, replace) {
            Ok(info) => Response::ok(info),
            Err(e) => engine_error(&e),
        },
        Request::Enable { id } => unit(engine.activate(&id)),
        Request::Disable { id } => unit(engine.disable(&id)),
        Request::Pause { id } => unit(engine.pause(&id)),
        Request::Resume { id } => unit(engine.resume(&id)),
        Request::Cancel { id } => unit(engine.cancel(&id)),
        Request::Remove { id } => unit(engine.remove(&id)),
        Request::Revoke { id } => match engine.workflow(&id) {
            Ok(_) => match engine.revoke_permissions(&id) {
                Ok(removed) => Response::ok(serde_json::json!({ "revoked": removed })),
                Err(e) => engine_error(&e),
            },
            Err(e) => engine_error(&e),
        },
        Request::RunNow { id } => match engine.run_now_as(&id, &run_label(principal)).await {
            Ok(record) => Response::ok(record),
            Err(e) => engine_error(&e),
        },
        Request::Shutdown => Response::ok(serde_json::json!({ "stopping": true })),
        other => agent_request(shared, principal, other),
    };

    if is_control {
        let result = match &response.error {
            None => "ok".to_string(),
            Some(e) => format!("error: {}", e.message),
        };
        let _ = engine.store().audit(
            &audit_id,
            &format!("ipc.{op}"),
            &format!("actor={actor} client={client} pid={pid}{target} result={result}"),
        );
    }
    response
}

fn run_label(principal: &Principal) -> String {
    match principal {
        Principal::Owner => "manual".into(),
        Principal::Agent(a) => format!("agent:{}", a.id),
    }
}

fn target_of(request: &Request) -> String {
    match request {
        Request::ProposalApprove { id, .. } | Request::ProposalReject { id } => {
            format!(" proposal={id}")
        }
        Request::AgentRevoke { id } => format!(" agent={id}"),
        Request::AgentCreate { name, scopes, .. } => {
            format!(" agent_name={name} scopes={}", scopes.join(","))
        }
        Request::MemoryPut { key, .. } | Request::MemoryDelete { key } => format!(" key={key}"),
        Request::ApprovalDecide { id, allow } => format!(" approval={id} allow={allow}"),
        _ => String::new(),
    }
}

fn agent_error(e: &AgentError) -> Response {
    let code = match e {
        AgentError::Invalid(_) => ErrorCode::BadRequest,
        AgentError::NotFound(_) => ErrorCode::NotFound,
        AgentError::Db(_) => ErrorCode::Internal,
    };
    Response::err(code, e.to_string())
}

fn ok_or<T: serde::Serialize>(r: Result<T, AgentError>) -> Response {
    match r {
        Ok(v) => Response::ok(v),
        Err(e) => agent_error(&e),
    }
}

/// Identity, agent and proposal operations. Authorization already passed.
fn agent_request(shared: &Shared, principal: &Principal, request: Request) -> Response {
    let engine = &shared.engine;
    let agents = &shared.agents;
    let me = match principal {
        Principal::Agent(a) => Some(a),
        Principal::Owner => None,
    };
    match request {
        Request::WhoAmI => Response::ok(serde_json::json!({
            "actor": principal.actor(),
            "name": me.map(|a| a.name.clone()).unwrap_or_else(|| "owner".into()),
            "scopes": me.map(|a| a.scopes.iter().map(Scope::to_string).collect::<Vec<_>>())
                .unwrap_or_else(|| vec!["*".into()]),
        })),
        Request::History { workflow, limit } => {
            match engine.history(workflow.as_deref(), limit.min(1000)) {
                Ok(h) => Response::ok(h),
                Err(e) => engine_error(&e),
            }
        }
        Request::Propose { definition, reason } => {
            let Some(agent) = me else {
                return Response::err(ErrorCode::Forbidden, "only agents propose");
            };
            let wf = match Workflow::from_toml(&definition) {
                Ok(w) => w,
                Err(e) => return engine_error(&e),
            };
            if let Err(e) = engine.validate(&wf) {
                return engine_error(&e);
            }
            let risks = ambar_automation::risk::review(&wf)
                .into_iter()
                .map(|r| format!("{:?}: {}", r.level, r.message).to_lowercase())
                .collect();
            ok_or(agents.propose(&agent.id, &wf.id, &definition, &reason, risks))
        }
        Request::ProposalList => ok_or(agents.proposals(me.map(|a| a.id.as_str()))),
        Request::ProposalApprove {
            id,
            allow_sensitive,
            enable,
        } => {
            let p = match agents.proposal(&id) {
                Ok(p) if p.status == "pending" => p,
                Ok(p) => {
                    return Response::err(
                        ErrorCode::State,
                        format!("proposal `{id}` is already {}", p.status),
                    );
                }
                Err(e) => return agent_error(&e),
            };
            let grant = Some(crate::protocol::GrantSpec { allow_sensitive });
            let info = match register(shared, &p.definition, grant, true) {
                Ok(i) => i,
                Err(e) => return engine_error(&e),
            };
            if enable && let Err(e) = engine.activate(&info.id) {
                return engine_error(&e);
            }
            let _ = engine.store().audit(
                &info.id,
                "proposal.approved",
                &format!(
                    "proposal={id} agent={} allow_sensitive={allow_sensitive}",
                    p.agent_id
                ),
            );
            ok_or(agents.decide(&id, true))
        }
        Request::ProposalReject { id } => ok_or(agents.decide(&id, false)),
        Request::MemoryGet { key } => match me {
            Some(a) => ok_or(agents.memory_get(&a.id, &key)),
            None => Response::err(ErrorCode::Forbidden, "agents only"),
        },
        Request::MemoryPut { key, value } => match me {
            Some(a) => ok_or(
                agents
                    .memory_put(&a.id, &key, &value)
                    .map(|()| serde_json::json!({})),
            ),
            None => Response::err(ErrorCode::Forbidden, "agents only"),
        },
        Request::MemoryList => match me {
            Some(a) => ok_or(agents.memory_keys(&a.id)),
            None => Response::err(ErrorCode::Forbidden, "agents only"),
        },
        Request::MemoryDelete { key } => match me {
            Some(a) => ok_or(
                agents
                    .memory_delete(&a.id, &key)
                    .map(|d| serde_json::json!({ "deleted": d })),
            ),
            None => Response::err(ErrorCode::Forbidden, "agents only"),
        },
        Request::AgentCreate {
            name,
            description,
            scopes,
        } => {
            let parsed: Result<Vec<Scope>, String> = scopes.iter().map(|s| s.parse()).collect();
            match parsed {
                Ok(scopes) => {
                    ok_or(agents.create(&name, &description, scopes).map(
                        |(agent, token)| serde_json::json!({ "agent": agent, "token": token }),
                    ))
                }
                Err(e) => Response::err(ErrorCode::BadRequest, e),
            }
        }
        Request::AgentList => ok_or(agents.list()),
        Request::ApprovalList => Response::ok(
            shared.broker.as_ref().map(|b| b.list()).unwrap_or_default(),
        ),
        Request::ApprovalDecide { id, allow } => match &shared.broker {
            None => Response::err(ErrorCode::State, "live approvals are not enabled"),
            Some(b) => match b.decide(&id, allow) {
                Some(info) => Response::ok(info),
                None => Response::err(
                    ErrorCode::NotFound,
                    format!("approval `{id}` is not waiting (already decided or expired)"),
                ),
            },
        },
        Request::AgentRevoke { id } => ok_or(agents.revoke(&id)),
        other => Response::err(
            ErrorCode::BadRequest,
            format!("unexpected operation `{}`", other.op()),
        ),
    }
}

fn unit(r: Result<(), EngineError>) -> Response {
    match r {
        Ok(()) => Response::ok(serde_json::json!({})),
        Err(e) => engine_error(&e),
    }
}

fn register(
    shared: &Shared,
    definition: &str,
    grant: Option<crate::protocol::GrantSpec>,
    replace: bool,
) -> Result<WorkflowInfo, EngineError> {
    let engine = &shared.engine;
    let wf = Workflow::from_toml(definition)?;
    engine.validate(&wf)?;
    let id = wf.id.clone();
    if let Ok(state) = engine.state(&id) {
        if !replace {
            return Err(EngineError::State(format!(
                "workflow `{id}` already exists; pass --replace to update it"
            )));
        }
        if matches!(state, WorkflowState::Active | WorkflowState::Paused) {
            engine.disable(&id)?;
        }
    }
    engine.register(wf)?;
    if let Some(g) = grant {
        engine.grant_permissions(&id, g.allow_sensitive)?;
    }
    info(shared, &id)
}

fn info(shared: &Shared, id: &str) -> Result<WorkflowInfo, EngineError> {
    let engine = &shared.engine;
    let wf = engine.workflow(id)?;
    let last = engine.store().executions(Some(id), 1)?.into_iter().next();
    Ok(WorkflowInfo {
        id: id.into(),
        name: wf.name.clone(),
        trigger: wf.trigger.kind().into(),
        state: engine.state(id)?.as_str().into(),
        running: engine.is_running(id)?,
        granted: engine.store().grant(id)?.is_some(),
        last_run: last.map(|r| LastRun {
            id: r.id,
            status: r.status.as_str().into(),
            started_at: r.started_at.to_rfc3339(),
            error: r.error,
        }),
    })
}

fn list(shared: &Shared) -> Result<Vec<WorkflowInfo>, EngineError> {
    shared
        .engine
        .list()?
        .into_iter()
        .map(|(id, _)| info(shared, &id))
        .collect()
}

fn health(shared: &Shared) -> Health {
    let engine = &shared.engine;
    let mut counts = StateCounts::default();
    let mut running = 0;
    for (id, state) in engine.list().unwrap_or_default() {
        match state {
            WorkflowState::Active => counts.active += 1,
            WorkflowState::Paused => counts.paused += 1,
            WorkflowState::Inactive => counts.inactive += 1,
            WorkflowState::Cancelled => counts.cancelled += 1,
        }
        if engine.is_running(&id).unwrap_or(false) {
            running += 1;
        }
    }
    let database_ok = engine.store().executions(None, 1).is_ok();
    let restore_failures = shared
        .restore_failures
        .lock()
        .map(|f| f.clone())
        .unwrap_or_default();
    let mut problems = Vec::new();
    if !database_ok {
        problems.push("database query failed".to_string());
    }
    for f in &restore_failures {
        problems.push(format!("workflow `{}` was not restored: {}", f.id, f.error));
    }
    if engine.is_shutting_down() {
        problems.push("shutting down".into());
    }
    Health {
        status: if problems.is_empty() {
            "ok"
        } else {
            "degraded"
        }
        .into(),
        problems,
        version: env!("CARGO_PKG_VERSION").into(),
        pid: std::process::id(),
        uptime_secs: shared.started.elapsed().as_secs(),
        started_at: shared.started_at.to_rfc3339(),
        data_dir: shared.paths.data_dir.display().to_string(),
        database_ok,
        workflows: counts,
        running,
        recovered_interrupted_runs: shared.recovered,
        restore_failures,
        pending_approvals: shared.broker.as_ref().map(|b| b.list().len()).unwrap_or(0),
        shutting_down: engine.is_shutting_down(),
    }
}
