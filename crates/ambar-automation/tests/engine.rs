//! End-to-end tests for the eight Phase 2 capabilities:
//!
//! 1. Create and validate a workflow        -> `create_and_validate`
//! 2. Execute a scheduled workflow          -> `scheduled_workflow_runs`
//! 3. Monitor a selected folder             -> `folder_watch_files_matching_files`
//! 4. Perform approved file operations      -> `sensitive_actions_need_approval`
//! 5. Send a notification                   -> (asserted in 2 and 3)
//! 6. Record execution history              -> `history_is_persisted`
//! 7. Pause and cancel workflows            -> `pause_*`, `cancel_*`
//! 8. Enforce permissions                   -> `activation_requires_grant`,
//!    `runtime_scope_is_enforced`, `revoked_grant_denies_runs`

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc as std_mpsc};
use std::time::Duration;

use ambar_automation::{
    ApprovalRequest, Approver, DenyAll, Engine, EngineConfig, EngineError, EngineEvent,
    ExecutionRecord, ExecutionStatus, MemoryNotifier, PermissionError, Store, Workflow,
    WorkflowState,
};
use tokio::sync::broadcast;
use tokio::time::timeout;

const WAIT: Duration = Duration::from_secs(10);

struct Env {
    _dir: tempfile::TempDir,
    root: PathBuf,
    engine: Engine,
    notifier: MemoryNotifier,
}

fn env_with(approver: Arc<dyn Approver>) -> Env {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let mut config = EngineConfig::new(root.join("data"));
    config.validation.min_interval = Duration::from_millis(100);
    config.debounce = Duration::from_millis(200);
    let notifier = MemoryNotifier::default();
    let store = Store::open(&root.join("data/ambar.db")).unwrap();
    let engine = Engine::new(store, Arc::new(notifier.clone()), approver, config).unwrap();
    Env {
        _dir: dir,
        root,
        engine,
        notifier,
    }
}

fn env() -> Env {
    env_with(Arc::new(DenyAll))
}

fn wf(src: &str, root: &Path) -> Workflow {
    Workflow::from_toml(&src.replace("$ROOT", &root.display().to_string())).unwrap()
}

/// Waits for the next finished execution matching `pred`.
async fn next_execution(
    rx: &mut broadcast::Receiver<EngineEvent>,
    pred: impl Fn(&ExecutionRecord) -> bool,
) -> ExecutionRecord {
    timeout(WAIT, async {
        loop {
            match rx.recv().await {
                Ok(EngineEvent::ExecutionFinished(r)) if pred(&r) => return r,
                Ok(_) => {}
                Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(e) => panic!("event channel closed: {e}"),
            }
        }
    })
    .await
    .expect("timed out waiting for an execution")
}

// ---------------------------------------------------------------- 1

#[tokio::test]
async fn create_and_validate() {
    let e = env();
    let bad = wf(
        r#"
id = "bad"
name = "Bad"
[trigger]
type = "schedule"
every = "1h"
[[actions]]
type = "write_file"
path = "$ROOT/out/log.txt"
content = "{file_name}"
"#,
        &e.root,
    );
    match e.engine.register(bad) {
        Err(EngineError::Invalid(errs)) => {
            let text: Vec<_> = errs.iter().map(|e| e.to_string()).collect();
            assert!(
                text.iter().any(|t| t.contains("only available")),
                "{text:?}"
            );
            assert!(
                text.iter().any(|t| t.contains("permissions.write")),
                "{text:?}"
            );
        }
        other => panic!("expected Invalid, got {other:?}"),
    }
    assert!(e.engine.list().unwrap().is_empty());

    let good = wf(
        r#"
id = "good"
name = "Good"
[trigger]
type = "manual"
[[actions]]
type = "create_directory"
path = "$ROOT/out/{date}"
[permissions]
write = ["$ROOT/out"]
"#,
        &e.root,
    );
    e.engine.register(good).unwrap();
    assert_eq!(e.engine.state("good").unwrap(), WorkflowState::Inactive);
    let stored = e.engine.store().list_workflows().unwrap();
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].2, "inactive");
}

// ---------------------------------------------------------------- 8a

#[tokio::test]
async fn activation_requires_grant() {
    let e = env();
    e.engine
        .register(wf(
            r#"
id = "w"
name = "W"
[trigger]
type = "manual"
[[actions]]
type = "create_directory"
path = "$ROOT/out/x"
[permissions]
write = ["$ROOT/out"]
"#,
            &e.root,
        ))
        .unwrap();
    let err = e.engine.activate("w").unwrap_err();
    assert!(
        matches!(err, EngineError::Permission(PermissionError::NotGranted(_))),
        "{err:?}"
    );
    assert!(
        e.engine.run_now("w").await.is_err(),
        "inactive workflows must not run"
    );
    assert!(!e.root.join("out/x").exists());

    e.engine.grant_permissions("w", false).unwrap();
    e.engine.activate("w").unwrap();
    let rec = e.engine.run_now("w").await.unwrap();
    assert_eq!(rec.status, ExecutionStatus::Succeeded);
    assert!(e.root.join("out/x").is_dir());

    let events: Vec<_> = e
        .engine
        .store()
        .audit_entries(Some("w"))
        .unwrap()
        .into_iter()
        .map(|a| a.event)
        .collect();
    assert_eq!(events, ["registered", "granted", "active"]);
}

// ---------------------------------------------------------------- 2 + 5

#[tokio::test]
async fn scheduled_workflow_runs() {
    let e = env();
    e.engine
        .register(wf(
            r#"
id = "report"
name = "Report"
[trigger]
type = "schedule"
every = "300ms"
[[actions]]
type = "write_file"
path = "$ROOT/reports/log.txt"
content = "run {execution_id}"
append = true
[[actions]]
type = "notify"
title = "Report ready"
body = "{workflow_id}"
[permissions]
write = ["$ROOT/reports"]
notify = true
"#,
            &e.root,
        ))
        .unwrap();
    e.engine.grant_permissions("report", false).unwrap();
    let mut rx = e.engine.subscribe();
    e.engine.activate("report").unwrap();

    for _ in 0..2 {
        let r = next_execution(&mut rx, |r| r.workflow_id == "report").await;
        assert_eq!(r.status, ExecutionStatus::Succeeded, "{r:?}");
        assert_eq!(r.trigger, "schedule");
        assert_eq!(r.steps.len(), 2);
    }
    e.engine.cancel("report").unwrap();

    let log = std::fs::read_to_string(e.root.join("reports/log.txt")).unwrap();
    assert!(log.lines().count() >= 2, "{log}");
    let sent = e.notifier.sent();
    assert!(sent.len() >= 2);
    assert_eq!(sent[0].title, "Report ready");
    assert_eq!(sent[0].body, "report");
}

#[tokio::test]
async fn cron_schedules_are_accepted() {
    let e = env();
    e.engine
        .register(wf(
            r#"
id = "daily"
name = "Daily"
[trigger]
type = "schedule"
cron = "0 9 * * 1-5"
[[actions]]
type = "create_directory"
path = "$ROOT/out"
[permissions]
write = ["$ROOT/out"]
"#,
            &e.root,
        ))
        .unwrap();
    e.engine.grant_permissions("daily", false).unwrap();
    e.engine.activate("daily").unwrap();
    // A scheduled workflow can also be run on demand.
    let r = e.engine.run_now("daily").await.unwrap();
    assert_eq!(r.status, ExecutionStatus::Succeeded);
    assert_eq!(r.trigger, "manual");
    e.engine.cancel("daily").unwrap();
}

// ---------------------------------------------------------------- 3 + 5

#[tokio::test]
async fn folder_watch_files_matching_files() {
    let e = env();
    std::fs::create_dir_all(e.root.join("inbox")).unwrap();
    e.engine
        .register(wf(
            r#"
id = "pdfs"
name = "File PDFs"
[trigger]
type = "folder_watch"
path = "$ROOT/inbox"
pattern = "*.pdf"
[[conditions]]
type = "file_size"
max_bytes = 1000
[[actions]]
type = "move_file"
to_dir = "$ROOT/filed/{date}"
[[actions]]
type = "notify"
title = "Filed {file_name}"
body = "{event}"
[permissions]
write = ["$ROOT/inbox", "$ROOT/filed"]
notify = true
"#,
            &e.root,
        ))
        .unwrap();
    e.engine.grant_permissions("pdfs", false).unwrap();
    let mut rx = e.engine.subscribe();
    e.engine.activate("pdfs").unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await; // watcher settle

    std::fs::write(e.root.join("inbox/big.pdf"), vec![0u8; 5000]).unwrap(); // fails condition
    std::fs::write(e.root.join("inbox/notes.txt"), "x").unwrap(); // fails pattern
    std::fs::write(e.root.join("inbox/small.pdf"), "pdf").unwrap();

    let r = next_execution(&mut rx, |r| r.workflow_id == "pdfs").await;
    assert_eq!(r.status, ExecutionStatus::Succeeded, "{r:?}");
    assert!(r.trigger.starts_with("created: "), "{}", r.trigger);
    assert!(r.trigger.ends_with("small.pdf"));

    let date = chrono::Local::now().format("%Y-%m-%d").to_string();
    assert!(e.root.join("filed").join(&date).join("small.pdf").exists());
    assert!(!e.root.join("inbox/small.pdf").exists());

    // Give the other files time to settle; they must not have been touched.
    tokio::time::sleep(Duration::from_millis(800)).await;
    assert!(e.root.join("inbox/big.pdf").exists());
    assert!(e.root.join("inbox/notes.txt").exists());
    assert_eq!(e.engine.history(Some("pdfs"), 10).unwrap().len(), 1);

    let sent = e.notifier.sent();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].title, "Filed small.pdf");
    assert_eq!(sent[0].body, "created");
    e.engine.cancel("pdfs").unwrap();
}

// ---------------------------------------------------------------- 4

const TRASH_WF: &str = r#"
id = "cleanup"
name = "Cleanup"
[trigger]
type = "manual"
[[actions]]
type = "trash_file"
path = "$ROOT/work/old.log"
[permissions]
write = ["$ROOT/work"]
"#;

#[derive(Debug, Default)]
struct RecordingApprover {
    allow: bool,
    seen: Mutex<Vec<ApprovalRequest>>,
}

impl Approver for RecordingApprover {
    fn approve(&self, r: &ApprovalRequest) -> bool {
        self.seen.lock().unwrap().push(r.clone());
        self.allow
    }
}

async fn run_cleanup(e: &Env, allow_sensitive: bool) -> ExecutionRecord {
    std::fs::create_dir_all(e.root.join("work")).unwrap();
    std::fs::write(e.root.join("work/old.log"), "old").unwrap();
    e.engine.register(wf(TRASH_WF, &e.root)).unwrap();
    e.engine
        .grant_permissions("cleanup", allow_sensitive)
        .unwrap();
    e.engine.activate("cleanup").unwrap();
    e.engine.run_now("cleanup").await.unwrap()
}

#[tokio::test]
async fn sensitive_actions_need_approval() {
    // Refused by the approver: nothing happens.
    let refuse = Arc::new(RecordingApprover::default());
    let e = env_with(refuse.clone());
    let r = run_cleanup(&e, false).await;
    assert_eq!(r.status, ExecutionStatus::Denied, "{r:?}");
    assert!(e.root.join("work/old.log").exists());
    let seen = refuse.seen.lock().unwrap().clone();
    assert_eq!(seen.len(), 1);
    assert!(seen[0].description.contains("old.log"));
    assert!(seen[0].description.contains("trash"));

    // Approved: file goes to the recoverable trash, not deleted.
    let allow = Arc::new(RecordingApprover {
        allow: true,
        ..Default::default()
    });
    let e = env_with(allow.clone());
    let r = run_cleanup(&e, false).await;
    assert_eq!(r.status, ExecutionStatus::Succeeded, "{r:?}");
    assert!(!e.root.join("work/old.log").exists());
    let trashed: Vec<_> = std::fs::read_dir(e.root.join("data/trash"))
        .unwrap()
        .collect();
    assert_eq!(trashed.len(), 1);
    assert_eq!(allow.seen.lock().unwrap().len(), 1);

    // Pre-approved in the grant: no prompt needed, even with DenyAll.
    let e = env();
    let r = run_cleanup(&e, true).await;
    assert_eq!(r.status, ExecutionStatus::Succeeded, "{r:?}");
}

#[tokio::test]
async fn existing_files_are_not_overwritten_without_approval() {
    let e = env();
    std::fs::create_dir_all(e.root.join("out")).unwrap();
    std::fs::write(e.root.join("out/summary.txt"), "precious").unwrap();
    e.engine
        .register(wf(
            r#"
id = "summary"
name = "Summary"
[trigger]
type = "manual"
[[actions]]
type = "write_file"
path = "$ROOT/out/summary.txt"
content = "replaced"
[permissions]
write = ["$ROOT/out"]
"#,
            &e.root,
        ))
        .unwrap();
    e.engine.grant_permissions("summary", false).unwrap();
    e.engine.activate("summary").unwrap();
    let r = e.engine.run_now("summary").await.unwrap();
    assert_eq!(r.status, ExecutionStatus::Denied);
    assert_eq!(
        std::fs::read_to_string(e.root.join("out/summary.txt")).unwrap(),
        "precious"
    );
}

// ---------------------------------------------------------------- 8b

#[cfg(unix)]
#[tokio::test]
async fn runtime_scope_is_enforced() {
    let e = env();
    std::fs::create_dir_all(e.root.join("allowed")).unwrap();
    std::fs::create_dir_all(e.root.join("secret")).unwrap();
    std::os::unix::fs::symlink(e.root.join("secret"), e.root.join("allowed/link")).unwrap();
    e.engine
        .register(wf(
            r#"
id = "escape"
name = "Escape"
[trigger]
type = "manual"
[[actions]]
type = "write_file"
path = "$ROOT/allowed/link/pwned.txt"
content = "x"
[permissions]
write = ["$ROOT/allowed"]
"#,
            &e.root,
        ))
        .unwrap();
    e.engine.grant_permissions("escape", false).unwrap();
    e.engine.activate("escape").unwrap();
    let r = e.engine.run_now("escape").await.unwrap();
    assert_eq!(r.status, ExecutionStatus::Denied, "{r:?}");
    assert!(r.error.unwrap().contains("outside the granted scope"));
    assert!(!e.root.join("secret/pwned.txt").exists());
}

#[tokio::test]
async fn revoked_grant_denies_runs() {
    let e = env();
    e.engine
        .register(wf(
            r#"
id = "w"
name = "W"
[trigger]
type = "manual"
[[actions]]
type = "create_directory"
path = "$ROOT/out/a"
[permissions]
write = ["$ROOT/out"]
"#,
            &e.root,
        ))
        .unwrap();
    e.engine.grant_permissions("w", false).unwrap();
    e.engine.activate("w").unwrap();
    assert!(e.engine.revoke_permissions("w").unwrap());
    let r = e.engine.run_now("w").await.unwrap();
    assert_eq!(r.status, ExecutionStatus::Denied);
    assert!(r.steps.is_empty());
    assert!(!e.root.join("out/a").exists());
}

// ---------------------------------------------------------------- 7

#[tokio::test]
async fn pause_skips_firings_and_resume_continues() {
    let e = env();
    e.engine
        .register(wf(
            r#"
id = "tick"
name = "Tick"
[trigger]
type = "schedule"
every = "200ms"
[[actions]]
type = "write_file"
path = "$ROOT/out/ticks.txt"
content = "tick"
append = true
[permissions]
write = ["$ROOT/out"]
"#,
            &e.root,
        ))
        .unwrap();
    e.engine.grant_permissions("tick", false).unwrap();
    let mut rx = e.engine.subscribe();
    e.engine.activate("tick").unwrap();
    next_execution(&mut rx, |r| r.status == ExecutionStatus::Succeeded).await;

    e.engine.pause("tick").unwrap();
    assert_eq!(e.engine.state("tick").unwrap(), WorkflowState::Paused);
    let skipped = next_execution(&mut rx, |r| r.status == ExecutionStatus::Skipped).await;
    assert!(skipped.steps.is_empty());

    e.engine.resume("tick").unwrap();
    next_execution(&mut rx, |r| r.status == ExecutionStatus::Succeeded).await;

    e.engine.cancel("tick").unwrap();
    assert!(e.engine.pause("tick").is_err());
    // A tick dispatched just before the cancel may still record its run;
    // let that settle, then nothing new may appear.
    tokio::time::sleep(Duration::from_millis(300)).await;
    let before = e.engine.history(Some("tick"), 1000).unwrap().len();
    tokio::time::sleep(Duration::from_millis(700)).await;
    let after = e.engine.history(Some("tick"), 1000).unwrap().len();
    assert_eq!(before, after, "cancelled schedule must not fire");
}

/// Blocks the first approval until released, so tests can act mid-run.
#[derive(Debug)]
struct GateApprover {
    entered: AtomicBool,
    release: Mutex<std_mpsc::Receiver<()>>,
}

const TWO_STEP: &str = r#"
id = "two"
name = "Two steps"
[trigger]
type = "manual"
[[actions]]
type = "trash_file"
path = "$ROOT/work/first.txt"
[[actions]]
type = "write_file"
path = "$ROOT/work/second.txt"
content = "done"
append = true
[permissions]
write = ["$ROOT/work"]
"#;

impl Approver for GateApprover {
    fn approve(&self, _: &ApprovalRequest) -> bool {
        self.entered.store(true, Ordering::SeqCst);
        let _ = self.release.lock().unwrap().recv_timeout(WAIT);
        true
    }
}

async fn start_gated_run() -> (
    Env,
    Arc<GateApprover>,
    std_mpsc::Sender<()>,
    tokio::task::JoinHandle<ExecutionRecord>,
) {
    let (tx, rx) = std_mpsc::channel();
    let gate = Arc::new(GateApprover {
        entered: AtomicBool::new(false),
        release: Mutex::new(rx),
    });
    let e = env_with(gate.clone());
    std::fs::create_dir_all(e.root.join("work")).unwrap();
    std::fs::write(e.root.join("work/first.txt"), "1").unwrap();
    e.engine.register(wf(TWO_STEP, &e.root)).unwrap();
    e.engine.grant_permissions("two", false).unwrap();
    e.engine.activate("two").unwrap();
    let engine = e.engine.clone();
    let run = tokio::spawn(async move { engine.run_now("two").await.unwrap() });
    timeout(WAIT, async {
        while !gate.entered.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    (e, gate, tx, run)
}

#[tokio::test]
async fn pause_holds_an_in_progress_run() {
    let (e, _gate, release, run) = start_gated_run().await;
    e.engine.pause("two").unwrap();
    release.send(()).unwrap(); // step 0 completes, step 1 must wait
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert!(!run.is_finished());
    assert!(
        !e.root.join("work/first.txt").exists(),
        "step 0 should have run"
    );
    assert!(
        !e.root.join("work/second.txt").exists(),
        "step 1 must wait while paused"
    );

    e.engine.resume("two").unwrap();
    let r = timeout(WAIT, run).await.unwrap().unwrap();
    assert_eq!(r.status, ExecutionStatus::Succeeded, "{r:?}");
    assert!(e.root.join("work/second.txt").exists());
}

#[tokio::test]
async fn cancel_stops_an_in_progress_run() {
    let (e, _gate, release, run) = start_gated_run().await;
    e.engine.cancel("two").unwrap();
    release.send(()).unwrap();
    let r = timeout(WAIT, run).await.unwrap().unwrap();
    assert_eq!(r.status, ExecutionStatus::Cancelled, "{r:?}");
    assert_eq!(r.steps.len(), 1, "only the step already running completes");
    assert!(!e.root.join("work/second.txt").exists());
    assert_eq!(e.engine.state("two").unwrap(), WorkflowState::Cancelled);
    // Cancelled workflows can be re-activated deliberately.
    e.engine.activate("two").unwrap();
    assert_eq!(e.engine.state("two").unwrap(), WorkflowState::Active);
}

// ---------------------------------------------------------------- 6

#[tokio::test]
async fn history_is_persisted() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let db = root.join("data/ambar.db");
    let src = r#"
id = "h"
name = "History"
[trigger]
type = "manual"
[[actions]]
type = "create_directory"
path = "$ROOT/out/ok"
[[actions]]
type = "copy_file"
from = "$ROOT/out/missing.txt"
to_dir = "$ROOT/out/copies"
[permissions]
write = ["$ROOT/out"]
"#;
    {
        let engine = Engine::new(
            Store::open(&db).unwrap(),
            Arc::new(MemoryNotifier::default()),
            Arc::new(DenyAll),
            EngineConfig::new(root.join("data")),
        )
        .unwrap();
        engine.register(wf(src, &root)).unwrap();
        engine.grant_permissions("h", false).unwrap();
        engine.activate("h").unwrap();
        let r = engine.run_now("h").await.unwrap();
        assert_eq!(r.status, ExecutionStatus::Failed);
    }
    // Re-open the database as a fresh process would.
    let store = Store::open(&db).unwrap();
    let runs = store.executions(Some("h"), 10).unwrap();
    assert_eq!(runs.len(), 1);
    let run = &runs[0];
    assert_eq!(run.status, ExecutionStatus::Failed);
    assert!(run.finished_at.is_some());
    assert!(run.error.as_deref().unwrap().contains("copy_file"));
    assert_eq!(run.steps.len(), 2);
    assert_eq!(run.steps[0].action, "create_directory");
    assert_eq!(run.steps[1].action, "copy_file");
    assert!(store.grant("h").unwrap().is_some());
}

// ---------------------------------------------------------------- Phase 3 engine additions

#[tokio::test]
async fn disable_lets_the_current_run_finish() {
    let (e, _gate, release, run) = start_gated_run().await;
    e.engine.disable("two").unwrap();
    assert_eq!(e.engine.state("two").unwrap(), WorkflowState::Inactive);
    assert!(e.engine.is_running("two").unwrap());
    release.send(()).unwrap();
    let r = timeout(WAIT, run).await.unwrap().unwrap();
    assert_eq!(r.status, ExecutionStatus::Succeeded, "{r:?}");
    assert!(e.root.join("work/second.txt").exists());
    assert!(!e.engine.is_running("two").unwrap());
    // Disabled workflows cannot be run until re-enabled.
    assert!(e.engine.run_now("two").await.is_err());
    e.engine.activate("two").unwrap();
}

#[tokio::test]
async fn manual_run_is_rejected_while_running() {
    let (e, _gate, release, run) = start_gated_run().await;
    let err = e.engine.run_now("two").await.unwrap_err();
    assert!(err.to_string().contains("already running"), "{err}");
    release.send(()).unwrap();
    timeout(WAIT, run).await.unwrap().unwrap();
    assert_eq!(e.engine.history(Some("two"), 10).unwrap().len(), 1);
}

#[derive(Debug)]
struct SlowApprover(Duration);
impl Approver for SlowApprover {
    fn approve(&self, _: &ApprovalRequest) -> bool {
        std::thread::sleep(self.0);
        true
    }
}

#[tokio::test]
async fn overlapping_schedule_ticks_are_skipped_not_duplicated() {
    let e = env_with(Arc::new(SlowApprover(Duration::from_millis(700))));
    std::fs::create_dir_all(e.root.join("work")).unwrap();
    e.engine
        .register(wf(
            r#"
id = "slow"
name = "Slow"
[trigger]
type = "schedule"
every = "100ms"
[[actions]]
type = "write_file"
path = "$ROOT/work/log.txt"
content = "run"
append = true
[[actions]]
type = "trash_file"
path = "$ROOT/work/missing.txt"
[permissions]
write = ["$ROOT/work"]
"#,
            &e.root,
        ))
        .unwrap();
    e.engine.grant_permissions("slow", false).unwrap();
    e.engine.activate("slow").unwrap();
    tokio::time::sleep(Duration::from_millis(1600)).await;
    e.engine.cancel("slow").unwrap();
    tokio::time::sleep(Duration::from_millis(800)).await;

    let runs = e.engine.history(Some("slow"), 1000).unwrap();
    let skipped = runs
        .iter()
        .filter(|r| r.status == ExecutionStatus::Skipped)
        .count();
    let ran = runs.len() - skipped;
    assert!(skipped >= 3, "expected skipped ticks, got {runs:?}");
    // ~1.6s / 0.7s per run: at most 3 real runs, never concurrent.
    assert!((1..=3).contains(&ran), "ran {ran}");
    let lines = std::fs::read_to_string(e.root.join("work/log.txt")).unwrap();
    assert_eq!(lines.lines().count(), ran);
    for r in runs.iter().filter(|r| r.status == ExecutionStatus::Skipped) {
        assert_eq!(r.error.as_deref(), Some("previous run still in progress"));
    }
}

#[tokio::test]
async fn shutdown_waits_for_runs_within_grace() {
    let (e, _gate, release, run) = start_gated_run().await;
    let engine = e.engine.clone();
    let shut = tokio::spawn(async move { engine.shutdown(Duration::from_secs(5)).await });
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(!shut.is_finished(), "shutdown must wait for the run");
    release.send(()).unwrap();
    let report = timeout(WAIT, shut).await.unwrap().unwrap();
    assert_eq!(report.interrupted, 0);
    let r = timeout(WAIT, run).await.unwrap().unwrap();
    assert_eq!(r.status, ExecutionStatus::Succeeded);
    // State is preserved for restore; new runs are refused.
    assert_eq!(e.engine.state("two").unwrap(), WorkflowState::Active);
    assert!(e.engine.run_now("two").await.is_err());
}

#[tokio::test]
async fn shutdown_cancels_runs_after_grace() {
    let (e, _gate, release, run) = start_gated_run().await;
    let engine = e.engine.clone();
    let shut = tokio::spawn(async move { engine.shutdown(Duration::from_millis(200)).await });
    tokio::time::sleep(Duration::from_millis(400)).await;
    release.send(()).unwrap();
    let report = timeout(WAIT, shut).await.unwrap().unwrap();
    assert_eq!(report.interrupted, 1);
    let r = timeout(WAIT, run).await.unwrap().unwrap();
    assert_eq!(r.status, ExecutionStatus::Cancelled, "{r:?}");
    assert!(!e.root.join("work/second.txt").exists());
}

#[tokio::test]
async fn remove_deletes_definition_and_grant_but_keeps_history() {
    let (e, _gate, release, run) = start_gated_run().await;
    e.engine.remove("two").unwrap();
    release.send(()).unwrap();
    let r = timeout(WAIT, run).await.unwrap().unwrap();
    assert_eq!(r.status, ExecutionStatus::Cancelled, "in-flight run stops");
    assert!(e.engine.state("two").is_err());
    assert!(e.engine.store().list_workflows().unwrap().is_empty());
    assert!(e.engine.store().grant("two").unwrap().is_none());
    assert_eq!(e.engine.history(Some("two"), 10).unwrap().len(), 1);
    // The id can be registered again from scratch.
    e.engine.register(wf(TWO_STEP, &e.root)).unwrap();
    assert!(e.engine.activate("two").is_err(), "old grant is gone");
}
