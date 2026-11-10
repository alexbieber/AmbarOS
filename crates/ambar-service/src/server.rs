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
    let engine = Engine::new(store, config.notifier, config.approver, config.engine)?;

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
    let client = match tokio::time::timeout(HELLO_TIMEOUT, read_frame(&mut reader)).await {
        Ok(Ok(Some(line))) => match serde_json::from_str::<Request>(&line) {
            Ok(Request::Hello {
                token,
                protocol,
                client,
            }) => {
                if !constant_time_eq(&token, &shared.token) {
                    let _ = write_frame(&mut write_half, &deny("invalid token")).await;
                    return;
                }
                if protocol != PROTOCOL_VERSION {
                    let msg =
                        format!("protocol mismatch: client {protocol}, service {PROTOCOL_VERSION}");
                    let _ =
                        write_frame(&mut write_half, &Response::err(ErrorCode::BadRequest, msg))
                            .await;
                    return;
                }
                client
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
                let stop = matches!(request, Request::Shutdown);
                (dispatch(&shared, request, &client, &pid).await, stop)
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

async fn dispatch(shared: &Shared, request: Request, client: &str, pid: &str) -> Response {
    let engine = &shared.engine;
    let op = request.op();
    let audit_id = match &request {
        Request::Enable { id }
        | Request::Disable { id }
        | Request::Pause { id }
        | Request::Resume { id }
        | Request::Cancel { id }
        | Request::RunNow { id }
        | Request::Revoke { id } => id.clone(),
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
        Request::Revoke { id } => match engine.workflow(&id) {
            Ok(_) => match engine.revoke_permissions(&id) {
                Ok(removed) => Response::ok(serde_json::json!({ "revoked": removed })),
                Err(e) => engine_error(&e),
            },
            Err(e) => engine_error(&e),
        },
        Request::RunNow { id } => match engine.run_now(&id).await {
            Ok(record) => Response::ok(record),
            Err(e) => engine_error(&e),
        },
        Request::Shutdown => Response::ok(serde_json::json!({ "stopping": true })),
    };

    if is_control {
        let result = match &response.error {
            None => "ok".to_string(),
            Some(e) => format!("error: {}", e.message),
        };
        let _ = engine.store().audit(
            &audit_id,
            &format!("ipc.{op}"),
            &format!("client={client} pid={pid} result={result}"),
        );
    }
    response
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
        shutting_down: engine.is_shutting_down(),
    }
}
