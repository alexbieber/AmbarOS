//! Live approvals: a background run waits for the owner's decision.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use ambar_automation::{DenyAll, ExecutionStatus, MemoryNotifier, Store};
use ambar_service::protocol::{ErrorCode, GrantSpec};
use ambar_service::{Client, DaemonConfig, RunningDaemon, ServicePaths, start};

struct Env {
    _dir: tempfile::TempDir,
    root: PathBuf,
    paths: ServicePaths,
    notifier: MemoryNotifier,
}

fn env() -> Env {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    std::fs::create_dir_all(root.join("w")).unwrap();
    std::fs::write(root.join("w/old.log"), "x").unwrap();
    Env {
        paths: ServicePaths::new(root.join("d")),
        root,
        _dir: dir,
        notifier: MemoryNotifier::default(),
    }
}

async fn daemon(e: &Env, timeout: Duration) -> RunningDaemon {
    let mut c = DaemonConfig::new(e.paths.clone(), Arc::new(e.notifier.clone()), Arc::new(DenyAll));
    c.live_approvals = Some(timeout);
    c.grace = Duration::from_secs(5);
    start(c).await.unwrap()
}

async fn setup(e: &Env) -> Client {
    let mut c = Client::connect(&e.paths, "owner").await.unwrap();
    let def = format!(
        r#"
id = "cleanup"
name = "Cleanup"
[trigger]
type = "manual"
[[actions]]
type = "trash_file"
path = "{r}/w/old.log"
[permissions]
write = ["{r}/w"]
"#,
        r = e.root.display()
    );
    c.register(&def, Some(GrantSpec { allow_sensitive: false }), false).await.unwrap();
    c.enable("cleanup").await.unwrap();
    c
}

fn start_run(e: &Env) -> tokio::task::JoinHandle<ambar_automation::ExecutionRecord> {
    let paths = e.paths.clone();
    tokio::spawn(async move {
        Client::connect(&paths, "runner").await.unwrap().run_now("cleanup").await.unwrap()
    })
}

async fn wait_pending(c: &mut Client) -> String {
    for _ in 0..200 {
        let list = c.approvals().await.unwrap();
        if let Some(a) = list.first() {
            return a.id.clone();
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("no approval request appeared");
}

#[tokio::test]
async fn owner_allows_and_the_run_continues() {
    let e = env();
    let d = daemon(&e, Duration::from_secs(30)).await;
    let mut c = setup(&e).await;
    let run = start_run(&e);
    let id = wait_pending(&mut c).await;

    let pending = &c.approvals().await.unwrap()[0];
    assert_eq!(pending.workflow_id, "cleanup");
    assert_eq!(pending.action, "trash_file");
    assert!(pending.description.contains("old.log"), "{}", pending.description);
    assert_eq!(c.health().await.unwrap().pending_approvals, 1);
    assert!(e.root.join("w/old.log").exists(), "nothing happens before the decision");
    assert!(e.notifier.sent().iter().any(|n| n.title.contains("approval")));

    c.decide(&id, true).await.unwrap();
    let r = run.await.unwrap();
    assert_eq!(r.status, ExecutionStatus::Succeeded, "{r:?}");
    assert!(!e.root.join("w/old.log").exists());
    assert!(c.approvals().await.unwrap().is_empty());
    // Deciding twice is refused.
    assert_eq!(c.decide(&id, true).await.unwrap_err().code(), Some(ErrorCode::NotFound));

    let events: Vec<_> = Store::open(&e.paths.db()).unwrap().audit_entries(Some("cleanup")).unwrap()
        .into_iter().map(|a| a.event).collect();
    assert!(events.contains(&"approval.requested".to_string()));
    assert!(events.contains(&"approval.allowed".to_string()));
    d.stop().await;
}

#[tokio::test]
async fn owner_denies() {
    let e = env();
    let d = daemon(&e, Duration::from_secs(30)).await;
    let mut c = setup(&e).await;
    let run = start_run(&e);
    let id = wait_pending(&mut c).await;
    c.decide(&id, false).await.unwrap();
    assert_eq!(run.await.unwrap().status, ExecutionStatus::Denied);
    assert!(e.root.join("w/old.log").exists());
    d.stop().await;
}

#[tokio::test]
async fn no_answer_means_deny() {
    let e = env();
    let d = daemon(&e, Duration::from_millis(400)).await;
    let _c = setup(&e).await;
    let r = start_run(&e).await.unwrap();
    assert_eq!(r.status, ExecutionStatus::Denied);
    assert!(e.root.join("w/old.log").exists());
    let events: Vec<_> = Store::open(&e.paths.db()).unwrap().audit_entries(Some("cleanup")).unwrap()
        .into_iter().map(|a| a.event).collect();
    assert!(events.contains(&"approval.expired".to_string()), "{events:?}");
    d.stop().await;
}

#[tokio::test]
async fn agents_cannot_see_or_decide_approvals() {
    let e = env();
    let d = daemon(&e, Duration::from_secs(30)).await;
    let mut c = setup(&e).await;
    let (_, token) = c.agent_create("sneaky", "", &["read", "run:*", "control:*"]).await.unwrap();
    let run = start_run(&e);
    let id = wait_pending(&mut c).await;
    let mut a = Client::connect_with_token(&e.paths, &token, "agent").await.unwrap();
    assert_eq!(a.approvals().await.unwrap_err().code(), Some(ErrorCode::Forbidden));
    assert_eq!(a.decide(&id, true).await.unwrap_err().code(), Some(ErrorCode::Forbidden));
    assert!(e.root.join("w/old.log").exists());
    c.decide(&id, false).await.unwrap();
    run.await.unwrap();
    d.stop().await;
}

#[tokio::test]
async fn shutdown_denies_waiting_approvals_promptly() {
    let e = env();
    let d = daemon(&e, Duration::from_secs(300)).await;
    let mut c = setup(&e).await;
    let run = start_run(&e);
    wait_pending(&mut c).await;
    let t = std::time::Instant::now();
    let report = d.stop().await;
    assert!(t.elapsed() < Duration::from_secs(4), "shutdown waited {:?}", t.elapsed());
    assert_eq!(report.interrupted, 0);
    assert_eq!(run.await.unwrap().status, ExecutionStatus::Denied);
    assert!(e.root.join("w/old.log").exists());
}
