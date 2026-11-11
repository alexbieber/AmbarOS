//! In-process tests of `ambard`: a real daemon on a real Unix socket, driven
//! through the real client. Process-level tests (spawning the binaries,
//! SIGKILL recovery) live in `crates/ambar-cli/tests/service.rs`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc as std_mpsc};
use std::time::Duration;

use ambar_automation::{
    ApprovalRequest, Approver, DenyAll, ExecutionStatus, MemoryNotifier, Store,
};
use ambar_service::protocol::{ErrorCode, GrantSpec, Request};
use ambar_service::{Client, ClientError, DaemonConfig, RunningDaemon, ServicePaths, start};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::time::timeout;

const WAIT: Duration = Duration::from_secs(10);
const GRANT: Option<GrantSpec> = Some(GrantSpec {
    allow_sensitive: false,
});

struct Env {
    _dir: tempfile::TempDir,
    root: PathBuf,
    paths: ServicePaths,
}

fn env() -> Env {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let paths = ServicePaths::new(root.join("d"));
    Env {
        _dir: dir,
        root,
        paths,
    }
}

fn config(e: &Env, approver: Arc<dyn Approver>) -> DaemonConfig {
    let mut c = DaemonConfig::new(
        e.paths.clone(),
        Arc::new(MemoryNotifier::default()),
        approver,
    );
    c.engine.validation.min_interval = Duration::from_millis(100);
    c.engine.debounce = Duration::from_millis(200);
    c.grace = Duration::from_secs(5);
    c
}

async fn daemon(e: &Env) -> RunningDaemon {
    start(config(e, Arc::new(DenyAll))).await.unwrap()
}

async fn client(e: &Env) -> Client {
    Client::connect(&e.paths, "test").await.unwrap()
}

fn toml(src: &str, root: &Path) -> String {
    src.replace("$ROOT", &root.display().to_string())
}

fn schedule_wf(id: &str, every: &str, root: &Path) -> String {
    toml(
        &format!(
            r#"
id = "{id}"
name = "{id}"
[trigger]
type = "schedule"
every = "{every}"
[[actions]]
type = "write_file"
path = "$ROOT/out/{id}.log"
content = "tick"
append = true
[permissions]
write = ["$ROOT/out"]
"#
        ),
        root,
    )
}

fn lines(path: &Path) -> usize {
    std::fs::read_to_string(path)
        .map(|s| s.lines().count())
        .unwrap_or(0)
}

async fn wait_until(mut cond: impl FnMut() -> bool) {
    timeout(WAIT, async {
        while !cond() {
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("condition not reached in time");
}

// ------------------------------------------------------------ startup / shutdown

#[tokio::test]
async fn startup_health_and_graceful_shutdown() {
    let e = env();
    let d = daemon(&e).await;
    let sock = e.paths.socket();
    let token = e.paths.token();
    for (p, mode) in [(&sock, 0o600), (&token, 0o600), (&e.paths.run_dir(), 0o700)] {
        let m = std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(m, mode, "{}", p.display());
    }

    let mut c = client(&e).await;
    assert_eq!(c.welcome().pid, std::process::id());
    let h = c.health().await.unwrap();
    assert_eq!(h.status, "ok", "{h:?}");
    assert!(h.database_ok);
    assert!(!h.shutting_down);

    c.shutdown().await.unwrap();
    let report = timeout(WAIT, d.wait()).await.unwrap();
    assert_eq!(report.interrupted, 0);
    assert!(!sock.exists(), "socket removed");
    assert!(!token.exists(), "token removed");
    assert!(!e.paths.is_locked(), "lock released");
    assert!(matches!(
        Client::connect(&e.paths, "t").await,
        Err(ClientError::NotRunning(_))
    ));

    let store = Store::open(&e.paths.db()).unwrap();
    let events: Vec<_> = store
        .audit_entries(Some("-"))
        .unwrap()
        .into_iter()
        .map(|a| a.event)
        .collect();
    assert_eq!(
        events,
        ["service.started", "ipc.shutdown", "service.stopped"]
    );
}

#[tokio::test]
async fn second_instance_is_refused() {
    let e = env();
    let d = daemon(&e).await;
    let err = start(config(&e, Arc::new(DenyAll))).await.unwrap_err();
    assert!(err.to_string().contains("already running"), "{err}");
    // The first daemon is unaffected.
    client(&e).await.health().await.unwrap();
    d.stop().await;
    // After it stops, a new one can start.
    daemon(&e).await.stop().await;
}

// ------------------------------------------------------------ auth

#[tokio::test]
async fn ipc_requires_valid_token_and_hello_first() {
    let e = env();
    let d = daemon(&e).await;

    let bad = Client::connect_with_token(&e.paths, &"0".repeat(64), "intruder").await;
    assert_eq!(bad.unwrap_err().code(), Some(ErrorCode::Unauthenticated));

    // Skipping hello: the request is refused and the connection closed.
    let mut raw = UnixStream::connect(e.paths.socket()).await.unwrap();
    raw.write_all(b"{\"op\":\"shutdown\"}\n").await.unwrap();
    let mut reply = String::new();
    BufReader::new(&mut raw)
        .read_line(&mut reply)
        .await
        .unwrap();
    assert!(reply.contains("unauthenticated"), "{reply}");

    // Garbage / oversized frames.
    let mut raw = UnixStream::connect(e.paths.socket()).await.unwrap();
    raw.write_all(&vec![b'x'; 2 * 1024 * 1024]).await.ok();
    let mut reply = String::new();
    let _ = BufReader::new(&mut raw).read_line(&mut reply).await;
    assert!(reply.contains("unauthenticated"), "{reply}");

    // An old token is useless after restart.
    let old = std::fs::read_to_string(e.paths.token()).unwrap();
    d.stop().await;
    let d = daemon(&e).await;
    let stale = Client::connect_with_token(&e.paths, &old, "old").await;
    assert_eq!(stale.unwrap_err().code(), Some(ErrorCode::Unauthenticated));

    // Service still healthy; every denial was audited.
    client(&e).await.health().await.unwrap();
    let store = Store::open(&e.paths.db()).unwrap();
    let denied = store
        .audit_entries(Some("-"))
        .unwrap()
        .into_iter()
        .filter(|a| a.event == "ipc.denied")
        .count();
    assert_eq!(denied, 4);
    d.stop().await;
}

// ------------------------------------------------------------ control + permissions

#[tokio::test]
async fn workflow_control_over_ipc() {
    let e = env();
    let d = daemon(&e).await;
    let mut c = client(&e).await;
    let def = schedule_wf("tick", "150ms", &e.root);

    // Registered without a grant: enabling is refused.
    let info = c.register(&def, None, false).await.unwrap();
    assert_eq!(info.state, "inactive");
    assert!(!info.granted);
    assert_eq!(
        c.enable("tick").await.unwrap_err().code(),
        Some(ErrorCode::Permission)
    );

    // Duplicate registration needs --replace.
    let dup = c.register(&def, GRANT, false).await.unwrap_err();
    assert_eq!(dup.code(), Some(ErrorCode::State));
    let info = c.register(&def, GRANT, true).await.unwrap();
    assert!(info.granted);

    c.enable("tick").await.unwrap();
    let log = e.root.join("out/tick.log");
    wait_until(|| lines(&log) >= 2).await;

    c.pause("tick").await.unwrap();
    assert_eq!(c.list().await.unwrap()[0].state, "paused");
    tokio::time::sleep(Duration::from_millis(300)).await; // let an in-flight tick land
    let paused_at = lines(&log);
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(lines(&log), paused_at, "no runs while paused");

    c.resume("tick").await.unwrap();
    wait_until(|| lines(&log) > paused_at).await;

    c.disable("tick").await.unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await; // let an in-flight tick land
    let disabled_at = lines(&log);
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(lines(&log), disabled_at, "no runs while disabled");
    assert_eq!(c.list().await.unwrap()[0].state, "inactive");

    c.enable("tick").await.unwrap();
    c.cancel("tick").await.unwrap();
    assert_eq!(c.list().await.unwrap()[0].state, "cancelled");

    // Errors map to codes.
    assert_eq!(
        c.pause("nope").await.unwrap_err().code(),
        Some(ErrorCode::NotFound)
    );
    assert_eq!(
        c.resume("tick").await.unwrap_err().code(),
        Some(ErrorCode::State)
    );
    let invalid = c.register("id = 'x'", GRANT, false).await.unwrap_err();
    assert_eq!(invalid.code(), Some(ErrorCode::Invalid));

    // Revoking takes effect immediately for manual runs.
    c.enable("tick").await.unwrap();
    c.revoke("tick").await.unwrap();
    let r = c.run_now("tick").await.unwrap();
    assert_eq!(r.status, ExecutionStatus::Denied);

    // Every control operation was audited with its outcome.
    let store = Store::open(&e.paths.db()).unwrap();
    let ipc: Vec<_> = store
        .audit_entries(Some("tick"))
        .unwrap()
        .into_iter()
        .filter(|a| a.event.starts_with("ipc."))
        .map(|a| (a.event, a.detail.contains("result=ok")))
        .collect();
    let expected = [
        ("ipc.register", true),
        ("ipc.enable", false),
        ("ipc.register", false),
        ("ipc.register", true),
        ("ipc.enable", true),
        ("ipc.pause", true),
        ("ipc.resume", true),
        ("ipc.disable", true),
        ("ipc.enable", true),
        ("ipc.cancel", true),
        ("ipc.resume", false),
        ("ipc.enable", true),
        ("ipc.revoke", true),
        ("ipc.run_now", true),
    ];
    let expected: Vec<_> = expected.iter().map(|(a, b)| (a.to_string(), *b)).collect();
    assert_eq!(ipc, expected);
    d.stop().await;
}

#[tokio::test]
async fn runtime_permissions_still_enforced_through_the_service() {
    let e = env();
    std::fs::create_dir_all(e.root.join("allowed")).unwrap();
    std::fs::create_dir_all(e.root.join("secret")).unwrap();
    std::os::unix::fs::symlink(e.root.join("secret"), e.root.join("allowed/link")).unwrap();
    let d = daemon(&e).await;
    let mut c = client(&e).await;
    let def = toml(
        r#"
id = "escape"
name = "Escape"
[trigger]
type = "manual"
[[actions]]
type = "write_file"
path = "$ROOT/allowed/link/pwned.txt"
content = "x"
[[actions]]
type = "trash_file"
path = "$ROOT/allowed/x"
[permissions]
write = ["$ROOT/allowed"]
"#,
        &e.root,
    );
    c.register(&def, GRANT, false).await.unwrap();
    c.enable("escape").await.unwrap();
    let r = c.run_now("escape").await.unwrap();
    assert_eq!(r.status, ExecutionStatus::Denied, "{r:?}");
    assert!(!e.root.join("secret/pwned.txt").exists());
    d.stop().await;
}

#[tokio::test]
async fn sensitive_actions_are_refused_in_background_unless_preapproved() {
    let e = env();
    std::fs::create_dir_all(e.root.join("w")).unwrap();
    std::fs::write(e.root.join("w/old.txt"), "x").unwrap();
    let def = toml(
        r#"
id = "trash"
name = "Trash"
[trigger]
type = "manual"
[[actions]]
type = "trash_file"
path = "$ROOT/w/old.txt"
[permissions]
write = ["$ROOT/w"]
"#,
        &e.root,
    );
    let d = daemon(&e).await;
    let mut c = client(&e).await;
    c.register(&def, GRANT, false).await.unwrap();
    c.enable("trash").await.unwrap();
    assert_eq!(
        c.run_now("trash").await.unwrap().status,
        ExecutionStatus::Denied
    );
    assert!(e.root.join("w/old.txt").exists());

    c.register(
        &def,
        Some(GrantSpec {
            allow_sensitive: true,
        }),
        true,
    )
    .await
    .unwrap();
    c.enable("trash").await.unwrap();
    assert_eq!(
        c.run_now("trash").await.unwrap().status,
        ExecutionStatus::Succeeded
    );
    assert!(!e.root.join("w/old.txt").exists());
    d.stop().await;
}

// ------------------------------------------------------------ persistence + recovery

#[tokio::test]
async fn enabled_workflows_are_restored_after_restart() {
    let e = env();
    let d = daemon(&e).await;
    let mut c = client(&e).await;
    for (id, then) in [
        ("a", "enable"),
        ("p", "pause"),
        ("x", "cancel"),
        ("i", "none"),
    ] {
        c.register(&schedule_wf(id, "150ms", &e.root), GRANT, false)
            .await
            .unwrap();
        match then {
            "enable" => c.enable(id).await.unwrap(),
            "pause" => {
                c.enable(id).await.unwrap();
                c.pause(id).await.unwrap();
            }
            "cancel" => {
                c.enable(id).await.unwrap();
                c.cancel(id).await.unwrap();
            }
            _ => {}
        }
    }
    drop(c);
    d.stop().await;

    // Nothing runs while the service is down.
    let a = e.root.join("out/a.log");
    let before = lines(&a);
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert_eq!(lines(&a), before);

    let d = daemon(&e).await;
    let mut c = client(&e).await;
    let states: Vec<_> = c
        .list()
        .await
        .unwrap()
        .into_iter()
        .map(|w| (w.id, w.state))
        .collect();
    assert_eq!(
        states,
        [
            ("a".to_string(), "active".to_string()),
            ("i".into(), "inactive".into()),
            ("p".into(), "paused".into()),
            ("x".into(), "inactive".into()),
        ]
    );
    wait_until(|| lines(&a) > before).await;
    assert_eq!(lines(&e.root.join("out/i.log")), 0);
    assert_eq!(c.health().await.unwrap().status, "ok");
    d.stop().await;
}

#[tokio::test]
async fn failed_restores_are_reported_and_retried() {
    let e = env();
    let watched = e.root.join("usb-drive");
    std::fs::create_dir_all(&watched).unwrap();
    let def = toml(
        r#"
id = "usb"
name = "USB"
[trigger]
type = "folder_watch"
path = "$ROOT/usb-drive"
[[actions]]
type = "notify"
title = "{file_name}"
[permissions]
read = ["$ROOT/usb-drive"]
notify = true
"#,
        &e.root,
    );
    let d = daemon(&e).await;
    let mut c = client(&e).await;
    c.register(&def, GRANT, false).await.unwrap();
    c.enable("usb").await.unwrap();
    drop(c);
    d.stop().await;

    // The "drive" is unplugged while the service is down.
    std::fs::remove_dir(&watched).unwrap();
    let d = daemon(&e).await;
    let h = client(&e).await.health().await.unwrap();
    assert_eq!(h.status, "degraded");
    assert_eq!(h.restore_failures.len(), 1);
    assert_eq!(h.restore_failures[0].id, "usb");
    d.stop().await;

    // Plugged back in: restored on the next start.
    std::fs::create_dir_all(&watched).unwrap();
    let d = daemon(&e).await;
    let mut c = client(&e).await;
    assert_eq!(c.health().await.unwrap().status, "ok");
    assert_eq!(c.list().await.unwrap()[0].state, "active");
    d.stop().await;
}

#[tokio::test]
async fn crash_leftovers_are_recovered() {
    let e = env();
    {
        let d = daemon(&e).await;
        let mut c = client(&e).await;
        c.register(&schedule_wf("a", "1h", &e.root), GRANT, false)
            .await
            .unwrap();
        c.enable("a").await.unwrap();
        drop(c);
        d.stop().await;
    }
    // Simulate a crash mid-run: a `running` execution, a stale socket and token.
    let store = Store::open(&e.paths.db()).unwrap();
    store
        .begin_execution(&ambar_automation::ExecutionRecord {
            id: "crashed".into(),
            workflow_id: "a".into(),
            trigger: "schedule".into(),
            status: ExecutionStatus::Running,
            started_at: chrono::Utc::now(),
            finished_at: None,
            error: None,
            steps: vec![],
        })
        .unwrap();
    std::fs::write(e.paths.socket(), "stale").unwrap();
    std::fs::write(e.paths.token(), "stale").unwrap();

    let d = daemon(&e).await;
    let mut c = client(&e).await;
    let h = c.health().await.unwrap();
    assert_eq!(h.recovered_interrupted_runs, 1);
    assert_eq!(c.list().await.unwrap()[0].state, "active");
    let run = &store.executions(Some("a"), 1).unwrap()[0];
    assert_eq!(run.id, "crashed");
    assert_eq!(run.status, ExecutionStatus::Failed);
    assert!(run.error.as_deref().unwrap().contains("interrupted"));
    d.stop().await;
}

// ------------------------------------------------------------ duplicates + concurrency

#[derive(Debug)]
struct Gate {
    entered: AtomicBool,
    release: Mutex<std_mpsc::Receiver<()>>,
}
impl Approver for Gate {
    fn approve(&self, _: &ApprovalRequest) -> bool {
        self.entered.store(true, Ordering::SeqCst);
        let _ = self.release.lock().unwrap().recv_timeout(WAIT);
        true
    }
}

#[tokio::test]
async fn duplicate_execution_is_prevented() {
    let e = env();
    let (tx, rx) = std_mpsc::channel();
    let gate = Arc::new(Gate {
        entered: AtomicBool::new(false),
        release: Mutex::new(rx),
    });
    std::fs::create_dir_all(e.root.join("w")).unwrap();
    std::fs::write(e.root.join("w/f.txt"), "x").unwrap();
    let d = start(config(&e, gate.clone())).await.unwrap();
    let def = toml(
        r#"
id = "dup"
name = "Dup"
[trigger]
type = "schedule"
every = "100ms"
[[actions]]
type = "trash_file"
path = "$ROOT/w/f.txt"
[permissions]
write = ["$ROOT/w"]
"#,
        &e.root,
    );
    let mut c = client(&e).await;
    c.register(&def, GRANT, false).await.unwrap();
    c.enable("dup").await.unwrap();
    wait_until(|| gate.entered.load(Ordering::SeqCst)).await;

    // While the first run is held open: manual runs are refused and schedule
    // ticks are recorded as skipped instead of piling up.
    let err = c.run_now("dup").await.unwrap_err();
    assert!(err.to_string().contains("already running"), "{err}");
    assert!(c.list().await.unwrap()[0].running);
    tokio::time::sleep(Duration::from_millis(600)).await;
    tx.send(()).unwrap();
    c.cancel("dup").await.unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;

    let runs = d.engine().history(Some("dup"), 100).unwrap();
    let real: Vec<_> = runs
        .iter()
        .filter(|r| r.status != ExecutionStatus::Skipped)
        .collect();
    let skipped = runs.len() - real.len();
    assert_eq!(real.len(), 1, "{runs:?}");
    assert!(skipped >= 3, "{runs:?}");
    d.stop().await;
}

#[tokio::test]
async fn concurrent_workflows_and_clients() {
    let e = env();
    let d = daemon(&e).await;
    let mut c = client(&e).await;
    let ids: Vec<String> = (0..6).map(|i| format!("wf-{i}")).collect();
    for id in &ids {
        c.register(&schedule_wf(id, "100ms", &e.root), GRANT, false)
            .await
            .unwrap();
        c.enable(id).await.unwrap();
    }
    // 20 clients hammering the socket while workflows run.
    let mut tasks = Vec::new();
    for i in 0..20 {
        let paths = e.paths.clone();
        tasks.push(tokio::spawn(async move {
            let mut c = Client::connect(&paths, &format!("c{i}")).await.unwrap();
            for _ in 0..10 {
                assert_eq!(c.list().await.unwrap().len(), 6);
                c.health().await.unwrap();
            }
        }));
    }
    for t in tasks {
        timeout(WAIT, t).await.unwrap().unwrap();
    }
    for id in &ids {
        let log = e.root.join(format!("out/{id}.log"));
        wait_until(|| lines(&log) >= 3).await;
    }
    let h = c.health().await.unwrap();
    assert_eq!(h.workflows.active, 6);
    let report = d.stop().await;
    assert_eq!(report.interrupted, 0);
    // Every run either succeeded or was skipped; none failed or overlapped.
    let store = Store::open(&e.paths.db()).unwrap();
    for r in store.executions(None, 10_000).unwrap() {
        assert!(
            matches!(
                r.status,
                ExecutionStatus::Succeeded | ExecutionStatus::Skipped
            ),
            "{r:?}"
        );
    }
}

#[tokio::test]
async fn shutdown_lets_in_flight_runs_finish() {
    let e = env();
    let (tx, rx) = std_mpsc::channel();
    let gate = Arc::new(Gate {
        entered: AtomicBool::new(false),
        release: Mutex::new(rx),
    });
    std::fs::create_dir_all(e.root.join("w")).unwrap();
    std::fs::write(e.root.join("w/f.txt"), "x").unwrap();
    let d = start(config(&e, gate.clone())).await.unwrap();
    let def = toml(
        r#"
id = "long"
name = "Long"
[trigger]
type = "manual"
[[actions]]
type = "trash_file"
path = "$ROOT/w/f.txt"
[[actions]]
type = "write_file"
path = "$ROOT/w/done.txt"
content = "done"
[permissions]
write = ["$ROOT/w"]
"#,
        &e.root,
    );
    let mut c = client(&e).await;
    c.register(&def, GRANT, false).await.unwrap();
    c.enable("long").await.unwrap();
    let paths = e.paths.clone();
    let run = tokio::spawn(async move {
        Client::connect(&paths, "runner")
            .await
            .unwrap()
            .run_now("long")
            .await
    });
    wait_until(|| gate.entered.load(Ordering::SeqCst)).await;

    c.shutdown().await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    tx.send(()).unwrap();
    let report = timeout(WAIT, d.wait()).await.unwrap();
    assert_eq!(report.interrupted, 0);
    let r = run.await.unwrap().unwrap();
    assert_eq!(r.status, ExecutionStatus::Succeeded);
    assert!(e.root.join("w/done.txt").exists());

    // And the workflow is still enabled for the next start.
    let d = daemon(&e).await;
    assert_eq!(client(&e).await.list().await.unwrap()[0].state, "active");
    d.stop().await;
}

#[tokio::test]
async fn second_hello_is_rejected() {
    let e = env();
    let d = daemon(&e).await;
    let mut c = client(&e).await;
    let err = c
        .call::<serde_json::Value>(&Request::Hello {
            token: "x".into(),
            protocol: 1,
            client: "x".into(),
        })
        .await
        .unwrap_err();
    assert_eq!(err.code(), Some(ErrorCode::BadRequest));
    d.stop().await;
}
