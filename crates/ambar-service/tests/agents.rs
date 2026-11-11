//! Agent runtime: identities, scoped access, proposals, memory, monitoring.
//! A real daemon over a real socket; agents connect with their own tokens.

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
}

fn env() -> Env {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    Env {
        paths: ServicePaths::new(root.join("d")),
        root,
        _dir: dir,
    }
}

async fn daemon(e: &Env) -> RunningDaemon {
    let mut c = DaemonConfig::new(
        e.paths.clone(),
        Arc::new(MemoryNotifier::default()),
        Arc::new(DenyAll),
    );
    c.engine.validation.min_interval = Duration::from_millis(100);
    start(c).await.unwrap()
}

async fn owner(e: &Env) -> Client {
    Client::connect(&e.paths, "owner").await.unwrap()
}

async fn as_agent(e: &Env, token: &str) -> Client {
    Client::connect_with_token(&e.paths, token, "agent-sdk")
        .await
        .unwrap()
}

fn manual(e: &Env, id: &str) -> String {
    format!(
        r#"
id = "{id}"
name = "{id}"
[trigger]
type = "manual"
[[actions]]
type = "write_file"
path = "{r}/out/{id}.txt"
content = "ran"
append = true
[permissions]
write = ["{r}/out"]
"#,
        r = e.root.display()
    )
}

const GRANT: Option<GrantSpec> = Some(GrantSpec {
    allow_sensitive: false,
});

#[tokio::test]
async fn agent_scopes_are_enforced() {
    let e = env();
    let d = daemon(&e).await;
    let mut o = owner(&e).await;
    for id in ["report", "other"] {
        o.register(&manual(&e, id), GRANT, false).await.unwrap();
        o.enable(id).await.unwrap();
    }
    let (agent, token) = o
        .agent_create("reporter", "runs the report", &["read", "run:report"])
        .await
        .unwrap();
    assert!(agent.id.starts_with("agent-"));

    let mut a = as_agent(&e, &token).await;
    let me = a.whoami().await.unwrap();
    assert_eq!(me["name"], "reporter");
    assert_eq!(me["scopes"], serde_json::json!(["read", "run:report"]));

    // Allowed.
    assert_eq!(a.list().await.unwrap().len(), 2);
    let r = a.run_now("report").await.unwrap();
    assert_eq!(r.status, ExecutionStatus::Succeeded);
    assert_eq!(
        r.trigger,
        format!("agent:{}", agent.id),
        "runs are attributed"
    );

    // Not allowed: other workflows, owner operations, memory without the scope.
    let forbidden = |r: Result<_, ambar_service::ClientError>| {
        assert_eq!(r.err().and_then(|e| e.code()), Some(ErrorCode::Forbidden))
    };
    forbidden(a.run_now("other").await.map(|_| ()));
    forbidden(a.enable("other").await);
    forbidden(a.pause("report").await);
    forbidden(a.revoke("report").await);
    forbidden(
        a.register(&manual(&e, "sneaky"), GRANT, false)
            .await
            .map(|_| ()),
    );
    forbidden(a.agent_create("child", "", &["run:*"]).await.map(|_| ()));
    forbidden(a.memory_put("k", "v").await);
    forbidden(a.shutdown().await);
    assert!(!e.root.join("out/other.txt").exists());
    assert!(!e.root.join("out/sneaky.txt").exists());

    // The service is still up (the agent's shutdown did nothing).
    o.health().await.unwrap();

    // Monitoring: counters and audit trail.
    let listed = o.agent_list().await.unwrap();
    assert_eq!(listed[0].denied, 8);
    assert!(listed[0].requests >= 11);
    assert!(listed[0].last_seen.is_some());
    let store = Store::open(&e.paths.db()).unwrap();
    let forbidden_entries = store
        .audit_entries(Some("-"))
        .unwrap()
        .into_iter()
        .filter(|a| a.event == "ipc.forbidden")
        .count();
    assert_eq!(forbidden_entries, 8);
    let run_audit = store
        .audit_entries(Some("report"))
        .unwrap()
        .into_iter()
        .find(|x| x.event == "ipc.run_now")
        .unwrap();
    assert!(
        run_audit
            .detail
            .contains(&format!("actor=agent:{}", agent.id))
    );
    d.stop().await;
}

#[tokio::test]
async fn revoked_agents_are_locked_out_immediately() {
    let e = env();
    let d = daemon(&e).await;
    let mut o = owner(&e).await;
    let (_, token) = o.agent_create("temp", "", &["read"]).await.unwrap();
    let mut open = as_agent(&e, &token).await;
    open.health().await.unwrap();
    o.agent_revoke("temp").await.unwrap();
    // Even an already-open connection is cut off.
    assert_eq!(
        open.health().await.unwrap_err().code(),
        Some(ErrorCode::Unauthenticated)
    );
    let r = Client::connect_with_token(&e.paths, &token, "x").await;
    assert_eq!(r.unwrap_err().code(), Some(ErrorCode::Unauthenticated));
    d.stop().await;
}

#[tokio::test]
async fn agent_tokens_survive_restart_but_owner_tokens_rotate() {
    let e = env();
    let d = daemon(&e).await;
    let (_, token) = owner(&e)
        .await
        .agent_create("keeper", "", &["read"])
        .await
        .unwrap();
    let owner_token = std::fs::read_to_string(e.paths.token()).unwrap();
    d.stop().await;
    let d = daemon(&e).await;
    as_agent(&e, &token).await.health().await.unwrap();
    assert_ne!(
        std::fs::read_to_string(e.paths.token()).unwrap(),
        owner_token
    );
    d.stop().await;
}

#[tokio::test]
async fn proposals_need_human_approval() {
    let e = env();
    let d = daemon(&e).await;
    let mut o = owner(&e).await;
    let (_, token) = o
        .agent_create("planner", "", &["propose", "read"])
        .await
        .unwrap();
    let mut a = as_agent(&e, &token).await;

    // Invalid proposals are rejected up front.
    let bad = a.propose("id = 'x'", "").await.unwrap_err();
    assert_eq!(bad.code(), Some(ErrorCode::Invalid));

    let p = a
        .propose(&manual(&e, "tidy"), "keeps notes tidy")
        .await
        .unwrap();
    assert_eq!(p.status, "pending");
    // Proposing registers nothing.
    assert!(o.list().await.unwrap().is_empty());
    assert_eq!(a.proposals().await.unwrap().len(), 1);

    let approved = o.approve(&p.id, false, true).await.unwrap();
    assert_eq!(approved.status, "approved");
    let wf = &o.list().await.unwrap()[0];
    assert_eq!(
        (wf.id.as_str(), wf.state.as_str(), wf.granted),
        ("tidy", "active", true)
    );
    // Decisions are final.
    assert_eq!(
        o.approve(&p.id, false, true).await.unwrap_err().code(),
        Some(ErrorCode::State)
    );

    let p2 = a.propose(&manual(&e, "spam"), "").await.unwrap();
    assert_eq!(o.reject(&p2.id).await.unwrap().status, "rejected");
    assert_eq!(o.list().await.unwrap().len(), 1);

    // Risky proposals carry the review.
    let home_wide = format!(
        r#"
id = "wide"
name = "Wide"
[trigger]
type = "manual"
[[actions]]
type = "create_directory"
path = "{h}/x"
[permissions]
write = ["{h}"]
"#,
        h = std::env::var("HOME").unwrap()
    );
    let p3 = a.propose(&home_wide, "").await.unwrap();
    assert!(
        p3.risks.iter().any(|r| r.starts_with("high")),
        "{:?}",
        p3.risks
    );
    d.stop().await;
}

#[tokio::test]
async fn memory_is_private_per_agent() {
    let e = env();
    let d = daemon(&e).await;
    let mut o = owner(&e).await;
    let (_, t1) = o.agent_create("one", "", &["memory"]).await.unwrap();
    let (_, t2) = o.agent_create("two", "", &["memory"]).await.unwrap();
    let mut a1 = as_agent(&e, &t1).await;
    let mut a2 = as_agent(&e, &t2).await;
    a1.memory_put("plan", "file pdfs on fridays").await.unwrap();
    assert_eq!(
        a1.memory_get("plan").await.unwrap().as_deref(),
        Some("file pdfs on fridays")
    );
    assert_eq!(a2.memory_get("plan").await.unwrap(), None);
    assert_eq!(a1.memory_list().await.unwrap(), ["plan"]);
    let big = "x".repeat(70_000);
    assert_eq!(
        a1.memory_put("big", &big).await.unwrap_err().code(),
        Some(ErrorCode::BadRequest)
    );
    // Memory survives restarts.
    drop((a1, a2));
    d.stop().await;
    let d = daemon(&e).await;
    let mut a1 = as_agent(&e, &t1).await;
    assert!(a1.memory_get("plan").await.unwrap().is_some());
    d.stop().await;
}

#[tokio::test]
async fn agents_are_rate_limited() {
    let e = env();
    let d = daemon(&e).await;
    let (_, token) = owner(&e)
        .await
        .agent_create("chatty", "", &["read"])
        .await
        .unwrap();
    let mut a = as_agent(&e, &token).await;
    let mut limited = None;
    for i in 0..200 {
        if let Err(err) = a.health().await {
            limited = Some((i, err.code()));
            break;
        }
    }
    let (at, code) = limited.expect("rate limit never triggered");
    assert_eq!(code, Some(ErrorCode::RateLimited));
    // hello counts as the first request of the minute
    assert!((115..=121).contains(&at), "limited at {at}");
    // The owner is unaffected.
    owner(&e).await.health().await.unwrap();
    d.stop().await;
}
