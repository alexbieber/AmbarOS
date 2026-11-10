//! The automation engine: workflow lifecycle, trigger dispatch, execution,
//! approvals, pause/resume/cancel and history recording.
//!
//! ## Lifecycle
//!
//! ```text
//! register ──► Inactive ──activate──► Active ◄──resume── Paused
//!                 ▲                     │ └────pause────►  │
//!                 │                   cancel             cancel
//!                 └──── (activate) ── Cancelled ◄──────────┘
//! ```
//!
//! - `activate` requires a stored grant covering the workflow's request.
//! - While `Paused`, new trigger firings are recorded as `skipped`, and a run
//!   already in progress waits before its next action until resumed.
//! - `cancel` stops the triggers and stops any in-progress run at the next
//!   action boundary (recorded as `cancelled`). Actions themselves are short
//!   filesystem operations and are never interrupted half-way.
//! - The grant is re-read at the start of every run, so revoking it takes
//!   effect immediately.

use std::collections::HashMap;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use chrono::Utc;
use globset::Glob;
use tokio::sync::{broadcast, mpsc, watch};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::actions::{self, ActionContext, ActionError};
use crate::error::{EngineError, Result};
use crate::history::{ExecutionRecord, ExecutionStatus, StepRecord, StepStatus, Store};
use crate::notify::Notifier;
use crate::permissions::{Grant, PermissionError};
use crate::template::Vars;
use crate::triggers::{self, Fire, WatchSpec};
use crate::workflow::{Condition, Trigger, ValidationOptions, Workflow, parse_schedule};

/// Engine settings.
#[derive(Debug, Clone)]
pub struct EngineConfig {
    /// Where trashed files go. Engine-owned; not subject to workflow grants.
    pub trash_dir: PathBuf,
    /// Validation limits.
    pub validation: ValidationOptions,
    /// How long a file must be quiet before its event fires.
    pub debounce: Duration,
}

impl EngineConfig {
    /// Defaults rooted at a data directory (e.g. `~/.local/share/ambar`).
    pub fn new(data_dir: impl Into<PathBuf>) -> Self {
        Self {
            trash_dir: data_dir.into().join("trash"),
            validation: ValidationOptions::default(),
            debounce: Duration::from_millis(500),
        }
    }
}

/// Lifecycle state of a registered workflow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkflowState {
    /// Registered, triggers not running.
    Inactive,
    /// Triggers running, firings execute.
    Active,
    /// Triggers running, firings skipped, in-flight runs hold.
    Paused,
    /// Stopped by the user.
    Cancelled,
}

impl WorkflowState {
    /// Stored representation.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Inactive => "inactive",
            Self::Active => "active",
            Self::Paused => "paused",
            Self::Cancelled => "cancelled",
        }
    }
}

impl FromStr for WorkflowState {
    type Err = String;
    fn from_str(s: &str) -> std::result::Result<Self, String> {
        Ok(match s {
            "inactive" => Self::Inactive,
            "active" => Self::Active,
            "paused" => Self::Paused,
            "cancelled" => Self::Cancelled,
            other => return Err(format!("unknown state `{other}`")),
        })
    }
}

impl std::fmt::Display for WorkflowState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A request for human approval of a sensitive action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovalRequest {
    /// Workflow id.
    pub workflow_id: String,
    /// Execution id.
    pub execution_id: String,
    /// Index of the action.
    pub action_index: usize,
    /// Action kind.
    pub action: String,
    /// Plain-language description, e.g. "move /a/b.txt to the Ambar trash".
    pub description: String,
}

/// Decides whether a sensitive action may run. Called on a blocking thread,
/// so implementations may wait for a human.
pub trait Approver: Send + Sync + std::fmt::Debug {
    /// Returns `true` to allow the action.
    fn approve(&self, request: &ApprovalRequest) -> bool;
}

/// Refuses every sensitive action. The safe default.
#[derive(Debug, Default, Clone)]
pub struct DenyAll;

impl Approver for DenyAll {
    fn approve(&self, _: &ApprovalRequest) -> bool {
        false
    }
}

/// Events broadcast by the engine.
#[derive(Debug, Clone)]
pub enum EngineEvent {
    /// A workflow changed state.
    StateChanged {
        /// Workflow id.
        workflow_id: String,
        /// New state.
        state: WorkflowState,
    },
    /// A run finished (any status, including skipped).
    ExecutionFinished(ExecutionRecord),
}

#[derive(Debug)]
struct Slot {
    workflow: Arc<Workflow>,
    state_tx: watch::Sender<WorkflowState>,
    /// Cancels triggers *and* in-progress runs.
    cancel: CancellationToken,
    /// Child of `cancel`: stops triggers only (disable, shutdown).
    triggers: CancellationToken,
    run_lock: Arc<tokio::sync::Mutex<()>>,
}

/// What to do when a firing arrives while the workflow is already running.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Overlap {
    /// Wait for the current run (file events: each file must be handled).
    Queue,
    /// Record a `skipped` execution (schedule ticks: no backlog, no duplicates).
    Skip,
    /// Return an error (manual runs: tell the caller).
    Reject,
}

/// Outcome of [`Engine::shutdown`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShutdownReport {
    /// Runs that were still going when the grace period ended and were
    /// cancelled at their next action boundary.
    pub interrupted: usize,
}

impl Slot {
    fn state(&self) -> WorkflowState {
        *self.state_tx.borrow()
    }
}

#[derive(Debug)]
struct Inner {
    store: Store,
    notifier: Arc<dyn Notifier>,
    approver: Arc<dyn Approver>,
    config: EngineConfig,
    slots: Mutex<HashMap<String, Slot>>,
    events: broadcast::Sender<EngineEvent>,
    shutting_down: AtomicBool,
}

/// The automation engine. Cheap to clone; clones share state.
#[derive(Debug, Clone)]
pub struct Engine {
    inner: Arc<Inner>,
}

struct Snapshot {
    workflow: Arc<Workflow>,
    state_rx: watch::Receiver<WorkflowState>,
    cancel: CancellationToken,
    run_lock: Arc<tokio::sync::Mutex<()>>,
}

impl Engine {
    /// Creates an engine. Runs left `running` by a previous crash are marked failed.
    pub fn new(
        store: Store,
        notifier: Arc<dyn Notifier>,
        approver: Arc<dyn Approver>,
        config: EngineConfig,
    ) -> Result<Self> {
        let recovered = store.recover_interrupted()?;
        if recovered > 0 {
            tracing::warn!(count = recovered, "marked interrupted executions as failed");
        }
        let (events, _) = broadcast::channel(256);
        Ok(Self {
            inner: Arc::new(Inner {
                store,
                notifier,
                approver,
                config,
                slots: Mutex::new(HashMap::new()),
                events,
                shutting_down: AtomicBool::new(false),
            }),
        })
    }

    /// The history store.
    pub fn store(&self) -> &Store {
        &self.inner.store
    }

    /// Subscribes to engine events.
    pub fn subscribe(&self) -> broadcast::Receiver<EngineEvent> {
        self.inner.events.subscribe()
    }

    fn slots(&self) -> Result<MutexGuard<'_, HashMap<String, Slot>>> {
        self.inner
            .slots
            .lock()
            .map_err(|_| EngineError::State("engine state lock poisoned".into()))
    }

    fn emit(&self, event: EngineEvent) {
        let _ = self.inner.events.send(event);
    }

    /// Validates without registering.
    pub fn validate(&self, workflow: &Workflow) -> Result<()> {
        workflow
            .validate(&self.inner.config.validation)
            .map_err(EngineError::Invalid)
    }

    /// Validates and registers a workflow (state `Inactive`). Replacing a
    /// workflow that is active or paused is refused; cancel it first.
    pub fn register(&self, workflow: Workflow) -> Result<()> {
        self.validate(&workflow)?;
        let id = workflow.id.clone();
        let toml = workflow.to_toml()?;
        {
            let mut slots = self.slots()?;
            if let Some(existing) = slots.get(&id)
                && matches!(
                    existing.state(),
                    WorkflowState::Active | WorkflowState::Paused
                )
            {
                return Err(EngineError::State(format!(
                    "workflow `{id}` is {}; cancel it before replacing it",
                    existing.state()
                )));
            }
            // Keep the run lock of a replaced workflow, so a run that is still
            // finishing can never overlap with a run of the new definition.
            let run_lock = slots
                .get(&id)
                .map(|s| s.run_lock.clone())
                .unwrap_or_else(|| Arc::new(tokio::sync::Mutex::new(())));
            let (state_tx, _) = watch::channel(WorkflowState::Inactive);
            let cancel = CancellationToken::new();
            slots.insert(
                id.clone(),
                Slot {
                    workflow: Arc::new(workflow),
                    state_tx,
                    triggers: cancel.child_token(),
                    cancel,
                    run_lock,
                },
            );
        }
        self.inner
            .store
            .save_workflow(&id, &toml, WorkflowState::Inactive.as_str())?;
        self.inner.store.audit(&id, "registered", "")?;
        Ok(())
    }

    /// The workflow definition, if registered.
    pub fn workflow(&self, id: &str) -> Result<Arc<Workflow>> {
        self.slots()?
            .get(id)
            .map(|s| s.workflow.clone())
            .ok_or_else(|| EngineError::NotFound(id.into()))
    }

    /// Current state of a workflow.
    pub fn state(&self, id: &str) -> Result<WorkflowState> {
        self.slots()?
            .get(id)
            .map(Slot::state)
            .ok_or_else(|| EngineError::NotFound(id.into()))
    }

    /// Registered workflow ids with their states.
    pub fn list(&self) -> Result<Vec<(String, WorkflowState)>> {
        let mut out: Vec<_> = self
            .slots()?
            .iter()
            .map(|(id, s)| (id.clone(), s.state()))
            .collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(out)
    }

    /// Records the user's approval of a workflow's permission request.
    ///
    /// The caller is responsible for having shown the request to the user and
    /// obtained consent (the CLI does this interactively). `allow_sensitive`
    /// pre-approves trash/overwrite actions so they do not prompt each run.
    pub fn grant_permissions(&self, id: &str, allow_sensitive: bool) -> Result<Grant> {
        let workflow = self.workflow(id)?;
        let grant = Grant::from_request(id, &workflow.permissions, allow_sensitive)?;
        self.inner.store.save_grant(&grant)?;
        self.inner.store.audit(
            id,
            "granted",
            &format!(
                "read={:?} write={:?} notify={} allow_sensitive={}",
                grant.read, grant.write, grant.notify, grant.allow_sensitive
            ),
        )?;
        Ok(grant)
    }

    /// Withdraws a workflow's grant. Subsequent runs are denied.
    pub fn revoke_permissions(&self, id: &str) -> Result<bool> {
        let removed = self.inner.store.revoke_grant(id)?;
        self.inner.store.audit(id, "revoked", "")?;
        Ok(removed)
    }

    fn set_state(&self, id: &str, state: WorkflowState) -> Result<()> {
        {
            let slots = self.slots()?;
            let slot = slots
                .get(id)
                .ok_or_else(|| EngineError::NotFound(id.into()))?;
            slot.state_tx.send_replace(state);
        }
        self.inner.store.set_workflow_state(id, state.as_str())?;
        self.inner.store.audit(id, state.as_str(), "")?;
        self.emit(EngineEvent::StateChanged {
            workflow_id: id.into(),
            state,
        });
        Ok(())
    }

    /// Starts a workflow's trigger. Requires a grant covering its request.
    pub fn activate(&self, id: &str) -> Result<()> {
        let (workflow, cancel) = {
            let mut slots = self.slots()?;
            let slot = slots
                .get_mut(id)
                .ok_or_else(|| EngineError::NotFound(id.into()))?;
            match slot.state() {
                WorkflowState::Inactive | WorkflowState::Cancelled => {}
                s => {
                    return Err(EngineError::State(format!(
                        "workflow `{id}` is already {s}"
                    )));
                }
            }
            if self.inner.shutting_down.load(Ordering::SeqCst) {
                return Err(EngineError::State("the engine is shutting down".into()));
            }
            slot.cancel = CancellationToken::new();
            slot.triggers = slot.cancel.child_token();
            (slot.workflow.clone(), slot.triggers.clone())
        };

        let grant = self
            .inner
            .store
            .grant(id)?
            .ok_or_else(|| PermissionError::NotGranted(id.into()))?;
        if let Err(e) = grant.covers(&workflow.permissions) {
            self.inner.store.audit(id, "denied", &e.to_string())?;
            return Err(e.into());
        }

        let (tx, mut rx) = mpsc::channel::<Fire>(64);
        match &workflow.trigger {
            Trigger::Manual => {}
            Trigger::Schedule { every, cron } => {
                let schedule = parse_schedule(every.as_deref(), cron.as_deref())
                    .map_err(EngineError::Trigger)?;
                triggers::spawn_schedule(schedule, tx, cancel.clone());
            }
            Trigger::FolderWatch {
                path,
                events,
                pattern,
                recursive,
            } => {
                let folder = crate::permissions::lexical_path(path)?;
                // The watched folder itself must be readable under the grant.
                let folder = grant.check_path(crate::permissions::Access::Read, &folder)?;
                triggers::spawn_folder_watch(
                    WatchSpec {
                        folder,
                        events: events.clone(),
                        pattern: pattern.clone(),
                        recursive: *recursive,
                        debounce: self.inner.config.debounce,
                    },
                    tx,
                    cancel.clone(),
                )?;
            }
        }

        let engine = self.clone();
        let wf_id = id.to_string();
        let dispatch_cancel = cancel.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = dispatch_cancel.cancelled() => break,
                    fire = rx.recv() => {
                        let Some(fire) = fire else { break };
                        let engine = engine.clone();
                        let wf_id = wf_id.clone();
                        tokio::spawn(async move {
                            if let Err(e) = engine.run_fire(&wf_id, fire).await {
                                tracing::error!(workflow = %wf_id, error = %e, "execution error");
                            }
                        });
                    }
                }
            }
        });

        self.set_state(id, WorkflowState::Active)
    }

    /// Pauses an active workflow.
    pub fn pause(&self, id: &str) -> Result<()> {
        match self.state(id)? {
            WorkflowState::Active => self.set_state(id, WorkflowState::Paused),
            s => Err(EngineError::State(format!(
                "cannot pause `{id}`: it is {s}"
            ))),
        }
    }

    /// Resumes a paused workflow.
    pub fn resume(&self, id: &str) -> Result<()> {
        match self.state(id)? {
            WorkflowState::Paused => self.set_state(id, WorkflowState::Active),
            s => Err(EngineError::State(format!(
                "cannot resume `{id}`: it is {s}"
            ))),
        }
    }

    /// Cancels a workflow: stops triggers and any in-progress run.
    pub fn cancel(&self, id: &str) -> Result<()> {
        let state = self.state(id)?;
        if state == WorkflowState::Cancelled {
            return Err(EngineError::State(format!(
                "workflow `{id}` is already cancelled"
            )));
        }
        {
            let slots = self.slots()?;
            if let Some(slot) = slots.get(id) {
                slot.cancel.cancel();
            }
        }
        self.set_state(id, WorkflowState::Cancelled)
    }

    /// Disables a workflow: stops its triggers, but lets a run that is already
    /// in progress finish. State becomes `Inactive`; `activate` re-enables it.
    pub fn disable(&self, id: &str) -> Result<()> {
        match self.state(id)? {
            WorkflowState::Active | WorkflowState::Paused => {}
            s => {
                return Err(EngineError::State(format!(
                    "cannot disable `{id}`: it is {s}"
                )));
            }
        }
        {
            let slots = self.slots()?;
            if let Some(slot) = slots.get(id) {
                slot.triggers.cancel();
            }
        }
        self.set_state(id, WorkflowState::Inactive)
    }

    /// Whether a run of this workflow is in progress right now.
    pub fn is_running(&self, id: &str) -> Result<bool> {
        let slots = self.slots()?;
        let slot = slots
            .get(id)
            .ok_or_else(|| EngineError::NotFound(id.into()))?;
        Ok(slot.run_lock.try_lock().is_err())
    }

    /// Graceful shutdown. Stops all triggers and refuses new runs, then waits
    /// up to `grace` for in-progress runs to finish. Runs still going after
    /// that are cancelled at their next action boundary. Stored workflow
    /// states are left unchanged, so they can be restored on the next start.
    pub async fn shutdown(&self, grace: Duration) -> ShutdownReport {
        self.inner.shutting_down.store(true, Ordering::SeqCst);
        let (locks, cancels): (Vec<_>, Vec<_>) = match self.slots() {
            Ok(slots) => slots
                .values()
                .map(|s| {
                    s.triggers.cancel();
                    (s.run_lock.clone(), s.cancel.clone())
                })
                .unzip(),
            Err(_) => (Vec::new(), Vec::new()),
        };
        let wait_all = |locks: Vec<Arc<tokio::sync::Mutex<()>>>| async move {
            for l in locks {
                drop(l.lock().await);
            }
        };
        if tokio::time::timeout(grace, wait_all(locks.clone()))
            .await
            .is_ok()
        {
            return ShutdownReport { interrupted: 0 };
        }
        let interrupted = locks.iter().filter(|l| l.try_lock().is_err()).count();
        tracing::warn!(interrupted, "grace period over; cancelling runs");
        for c in &cancels {
            c.cancel();
        }
        // Actions are short filesystem operations; allow them to reach a boundary.
        let _ = tokio::time::timeout(Duration::from_secs(10), wait_all(locks)).await;
        ShutdownReport { interrupted }
    }

    /// Whether [`Engine::shutdown`] has been called.
    pub fn is_shutting_down(&self) -> bool {
        self.inner.shutting_down.load(Ordering::SeqCst)
    }

    /// Runs an active manual or scheduled workflow immediately and waits for it.
    pub async fn run_now(&self, id: &str) -> Result<ExecutionRecord> {
        self.run_now_as(id, "manual").await
    }

    /// Like [`Engine::run_now`], recording `trigger` (e.g. `agent:agent-1a2b`)
    /// as what started the run.
    pub async fn run_now_as(&self, id: &str, trigger: &str) -> Result<ExecutionRecord> {
        let workflow = self.workflow(id)?;
        if workflow.trigger.provides_file() {
            return Err(EngineError::State(format!(
                "`{id}` is triggered by files and cannot be run manually"
            )));
        }
        match self.state(id)? {
            WorkflowState::Active | WorkflowState::Paused => {}
            s => {
                return Err(EngineError::State(format!(
                    "activate `{id}` first (it is {s})"
                )));
            }
        }
        self.execute(id, trigger.into(), Vars::new(), None, Overlap::Reject)
            .await?
            .ok_or_else(|| EngineError::State(format!("`{id}` was cancelled before it ran")))
    }

    /// Recent executions for a workflow (or all), newest first.
    pub fn history(&self, id: Option<&str>, limit: usize) -> Result<Vec<ExecutionRecord>> {
        self.inner.store.executions(id, limit)
    }

    async fn run_fire(&self, id: &str, fire: Fire) -> Result<Option<ExecutionRecord>> {
        match fire {
            Fire::Schedule => {
                self.execute(id, "schedule".into(), Vars::new(), None, Overlap::Skip)
                    .await
            }
            Fire::File { path, kind } => {
                let mut vars = Vars::new();
                vars.insert("file", path.display().to_string());
                let name = path
                    .file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                vars.insert("file_name", name);
                vars.insert(
                    "file_stem",
                    path.file_stem()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                );
                vars.insert(
                    "file_ext",
                    path.extension()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                );
                vars.insert(
                    "file_dir",
                    path.parent()
                        .map(|p| p.display().to_string())
                        .unwrap_or_default(),
                );
                vars.insert("event", kind.as_str().into());
                let desc = format!("{}: {}", kind.as_str(), path.display());
                self.execute(id, desc, vars, Some(path), Overlap::Queue)
                    .await
            }
        }
    }

    fn snapshot(&self, id: &str) -> Result<Snapshot> {
        let slots = self.slots()?;
        let slot = slots
            .get(id)
            .ok_or_else(|| EngineError::NotFound(id.into()))?;
        Ok(Snapshot {
            workflow: slot.workflow.clone(),
            state_rx: slot.state_tx.subscribe(),
            cancel: slot.cancel.clone(),
            run_lock: slot.run_lock.clone(),
        })
    }

    /// Core run loop. Returns `None` when the firing was filtered out by
    /// conditions or arrived after cancellation (nothing is recorded then).
    async fn execute(
        &self,
        id: &str,
        trigger: String,
        mut vars: Vars,
        file: Option<PathBuf>,
        overlap: Overlap,
    ) -> Result<Option<ExecutionRecord>> {
        if self.is_shutting_down() {
            return Ok(None);
        }
        let Snapshot {
            workflow,
            mut state_rx,
            cancel,
            run_lock,
        } = self.snapshot(id)?;

        if let Some(path) = &file
            && !conditions_hold(&workflow.conditions, path)
        {
            tracing::debug!(workflow = %id, file = %path.display(), "conditions not met");
            return Ok(None);
        }

        let state = *state_rx.borrow_and_update();
        if state == WorkflowState::Cancelled || cancel.is_cancelled() {
            return Ok(None);
        }

        let execution_id = Uuid::new_v4().to_string();
        let mut record = ExecutionRecord {
            id: execution_id.clone(),
            workflow_id: id.into(),
            trigger,
            status: ExecutionStatus::Running,
            started_at: Utc::now(),
            finished_at: None,
            error: None,
            steps: Vec::new(),
        };

        let skip = |mut record: ExecutionRecord, why: &str| -> Result<Option<ExecutionRecord>> {
            record.status = ExecutionStatus::Skipped;
            record.error = Some(why.into());
            record.finished_at = Some(Utc::now());
            self.inner.store.begin_execution(&record)?;
            self.emit(EngineEvent::ExecutionFinished(record.clone()));
            Ok(Some(record))
        };

        if state == WorkflowState::Paused {
            return skip(record, "workflow was paused");
        }

        // One run at a time per workflow.
        let _guard = match overlap {
            Overlap::Queue => tokio::select! {
                g = run_lock.lock_owned() => g,
                _ = cancel.cancelled() => return Ok(None),
            },
            Overlap::Skip => match run_lock.try_lock_owned() {
                Ok(g) => g,
                Err(_) => return skip(record, "previous run still in progress"),
            },
            Overlap::Reject => run_lock
                .try_lock_owned()
                .map_err(|_| EngineError::State(format!("`{id}` is already running")))?,
        };
        if self.is_shutting_down() {
            return Ok(None);
        }

        self.inner.store.begin_execution(&record)?;

        // Permission grant is re-read for every run.
        let grant = match self.inner.store.grant(id)? {
            Some(g) => match g.covers(&workflow.permissions) {
                Ok(()) => g,
                Err(e) => return self.finish(record, ExecutionStatus::Denied, Some(e.to_string())),
            },
            None => {
                let msg = PermissionError::NotGranted(id.into()).to_string();
                return self.finish(record, ExecutionStatus::Denied, Some(msg));
            }
        };

        let now = chrono::Local::now();
        vars.insert("date", now.format("%Y-%m-%d").to_string());
        vars.insert("time", now.format("%H-%M-%S").to_string());
        vars.insert("datetime", now.format("%Y-%m-%dT%H-%M-%S").to_string());
        vars.insert("workflow_id", id.into());
        vars.insert("execution_id", execution_id.clone());
        let vars = Arc::new(vars);
        let grant = Arc::new(grant);

        for (index, action) in workflow.actions.iter().enumerate() {
            // Checkpoint: wait while paused, stop if cancelled.
            loop {
                if cancel.is_cancelled() {
                    let msg = format!("cancelled before action {index} ({})", action.kind());
                    return self.finish(record, ExecutionStatus::Cancelled, Some(msg));
                }
                let current = *state_rx.borrow_and_update();
                match current {
                    WorkflowState::Paused => {
                        tokio::select! {
                            changed = state_rx.changed() => {
                                if changed.is_err() {
                                    return self.finish(record, ExecutionStatus::Cancelled, Some("workflow was removed".into()));
                                }
                            }
                            _ = cancel.cancelled() => {}
                        }
                    }
                    WorkflowState::Cancelled => {
                        let msg = format!("cancelled before action {index} ({})", action.kind());
                        return self.finish(record, ExecutionStatus::Cancelled, Some(msg));
                    }
                    _ => break,
                }
            }

            let action = action.clone();
            let ctx_vars = vars.clone();
            let ctx_grant = grant.clone();
            let notifier = self.inner.notifier.clone();
            let approver = self.inner.approver.clone();
            let trash = self.inner.config.trash_dir.clone();
            let wf_id = id.to_string();
            let exec_id = execution_id.clone();

            let outcome = tokio::task::spawn_blocking(move || -> (StepStatus, String) {
                let ctx = ActionContext {
                    workflow_id: &wf_id,
                    execution_id: &exec_id,
                    vars: &ctx_vars,
                    grant: &ctx_grant,
                    trash_dir: &trash,
                    notifier: notifier.as_ref(),
                };
                match actions::needs_approval(&action, &ctx) {
                    Ok(true) if !ctx_grant.allow_sensitive => {
                        let request = ApprovalRequest {
                            workflow_id: wf_id.clone(),
                            execution_id: exec_id.clone(),
                            action_index: index,
                            action: action.kind().into(),
                            description: actions::describe(&action, &ctx),
                        };
                        if !approver.approve(&request) {
                            let e = PermissionError::NotApproved(request.description);
                            return (StepStatus::Denied, e.to_string());
                        }
                    }
                    Ok(_) => {}
                    Err(e) => return (StepStatus::Failed, e.to_string()),
                }
                match actions::execute(&action, &ctx) {
                    Ok(msg) => (StepStatus::Succeeded, msg),
                    Err(ActionError::Permission(e)) => (StepStatus::Denied, e.to_string()),
                    Err(ActionError::Failed(e)) => (StepStatus::Failed, e),
                }
            })
            .await
            .unwrap_or_else(|e| (StepStatus::Failed, format!("action panicked: {e}")));

            let step = StepRecord {
                index,
                action: workflow.actions[index].kind().into(),
                status: outcome.0,
                message: outcome.1,
                finished_at: Utc::now(),
            };
            self.inner.store.record_step(&execution_id, &step)?;
            record.steps.push(step.clone());

            match step.status {
                StepStatus::Succeeded => {}
                StepStatus::Denied => {
                    self.inner.store.audit(id, "denied", &step.message)?;
                    return self.finish(record, ExecutionStatus::Denied, Some(step.message));
                }
                StepStatus::Failed => {
                    let msg = format!("action {index} ({}) failed: {}", step.action, step.message);
                    return self.finish(record, ExecutionStatus::Failed, Some(msg));
                }
            }
        }

        self.finish(record, ExecutionStatus::Succeeded, None)
    }

    fn finish(
        &self,
        mut record: ExecutionRecord,
        status: ExecutionStatus,
        error: Option<String>,
    ) -> Result<Option<ExecutionRecord>> {
        self.inner
            .store
            .finish_execution(&record.id, status, error.as_deref())?;
        record.status = status;
        record.error = error;
        record.finished_at = Some(Utc::now());
        tracing::info!(
            workflow = %record.workflow_id,
            execution = %record.id,
            status = %status,
            steps = record.steps.len(),
            "execution finished"
        );
        self.emit(EngineEvent::ExecutionFinished(record.clone()));
        Ok(Some(record))
    }
}

fn conditions_hold(conditions: &[Condition], path: &std::path::Path) -> bool {
    conditions.iter().all(|c| match c {
        Condition::FileExtension { any_of } => {
            let ext = path
                .extension()
                .map(|e| e.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            any_of
                .iter()
                .any(|a| a.trim_start_matches('.').to_lowercase() == ext)
        }
        Condition::FileNameMatches { glob } => {
            let name = path.file_name().unwrap_or_default();
            Glob::new(glob)
                .map(|g| g.compile_matcher().is_match(name))
                .unwrap_or(false)
        }
        Condition::FileSize {
            min_bytes,
            max_bytes,
        } => match std::fs::metadata(path) {
            Ok(m) => {
                let len = m.len();
                min_bytes.is_none_or(|min| len >= min) && max_bytes.is_none_or(|max| len <= max)
            }
            Err(_) => false,
        },
    })
}
