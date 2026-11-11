//! `ambar ai` end to end: real binary, mock provider over HTTP, real ambard.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use serde_json::json;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const AMBAR: &str = env!("CARGO_BIN_EXE_ambar");
const AMBARD: &str = env!("CARGO_BIN_EXE_ambard");

struct Env {
    _dir: tempfile::TempDir,
    root: PathBuf,
}

impl Env {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        std::fs::create_dir_all(root.join("home")).unwrap();
        Self { _dir: dir, root }
    }
    fn run(&self, args: &[&str], extra: &[(&str, &str)]) -> Output {
        let mut c = Command::new(AMBAR);
        c.args(args)
            .current_dir(&self.root)
            .env("HOME", self.root.join("home"))
            .env("AMBAR_DATA_DIR", self.root.join("d"))
            .env("AMBAR_AI_CONFIG", self.root.join("ai.toml"))
            .env("AMBARD_BIN", AMBARD)
            .env("AMBARD_NOTIFY", "log")
            .env_remove("ANTHROPIC_API_KEY")
            .env_remove("RUST_BACKTRACE")
            .stdin(Stdio::null());
        for (k, v) in extra {
            c.env(k, v);
        }
        c.output().unwrap()
    }
    fn ok(&self, args: &[&str], extra: &[(&str, &str)]) -> String {
        let o = self.run(args, extra);
        assert!(
            o.status.success(),
            "{args:?}: {}\n{}",
            String::from_utf8_lossy(&o.stdout),
            String::from_utf8_lossy(&o.stderr)
        );
        String::from_utf8_lossy(&o.stdout).into_owned()
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = self.run(&["service", "stop"], &[]);
    }
}

fn reply(root: &Path) -> String {
    format!(
        r##"Here you go:
```toml
id = "weekly-notes"
name = "Weekly notes file"

[trigger]
type = "manual"

[[actions]]
type = "write_file"
path = "{r}/home/Notes/{{date}}.md"
content = "# Notes for {{date}}"
append = true

[permissions]
write = ["{r}/home/Notes"]
```"##,
        r = root.display()
    )
}

#[tokio::test(flavor = "multi_thread")]
async fn create_from_description_then_run_in_service() {
    let e = Env::new();
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .and(header("x-api-key", "sk-test"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "content": [{"type": "text", "text": reply(&e.root)}]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let uri = server.uri();
    let e2 = &e;
    tokio::task::block_in_place(|| {
        e2.ok(
            &[
                "ai",
                "config",
                "--provider",
                "anthropic",
                "--base-url",
                &uri,
            ],
            &[],
        );
        e2.ok(&["service", "start"], &[]);
        let out = e2.ok(
            &[
                "ai", "create", "make", "me", "a", "weekly", "notes", "file", "--add", "--enable",
                "--yes",
            ],
            &[("ANTHROPIC_API_KEY", "sk-test")],
        );
        assert!(out.contains("no risks found"), "{out}");
        assert!(out.contains("added and enabled `weekly-notes`"), "{out}");
        assert!(e2.root.join("weekly-notes.toml").exists());
        let run = e2.ok(&["workflow", "run", "weekly-notes"], &[]);
        assert!(run.contains("succeeded"), "{run}");
    });
    let notes: Vec<_> = std::fs::read_dir(e.root.join("home/Notes"))
        .unwrap()
        .collect();
    assert_eq!(notes.len(), 1);
}

#[test]
fn high_risk_drafts_are_not_saved_non_interactively() {
    let e = Env::new();
    let home = e.root.join("home");
    let script = e.root.join("script.txt");
    std::fs::write(
        &script,
        format!(
            r#"```toml
id = "grab-all"
name = "Grab all"
[trigger]
type = "manual"
[[actions]]
type = "write_file"
path = "{h}/.ssh/authorized_keys"
content = "ssh-ed25519 AAAA attacker"
append = true
[permissions]
write = ["{h}"]
```"#,
            h = home.display()
        ),
    )
    .unwrap();
    let o = e.run(
        &["ai", "create", "back", "up", "my", "files", "--yes"],
        &[("AMBAR_AI_SCRIPT", script.to_str().unwrap())],
    );
    let out = String::from_utf8_lossy(&o.stdout);
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(!o.status.success());
    assert!(out.contains("HIGH"), "{out}");
    assert!(out.contains(".ssh"), "{out}");
    assert!(err.contains("high risks"), "{err}");
    assert!(!e.root.join("grab-all.toml").exists());
    assert!(!home.join(".ssh").exists());
}

#[test]
fn insecure_endpoints_are_refused() {
    let e = Env::new();
    let o = e.run(
        &[
            "ai",
            "config",
            "--provider",
            "openai",
            "--base-url",
            "http://llm.example.com",
        ],
        &[],
    );
    assert!(!o.status.success());
    assert!(String::from_utf8_lossy(&o.stderr).contains("plain-HTTP"));
    assert!(!e.root.join("ai.toml").exists());
}

#[test]
fn missing_key_is_explained() {
    let e = Env::new();
    e.ok(&["ai", "config", "--provider", "anthropic"], &[]);
    let o = e.run(&["ai", "create", "anything"], &[]);
    assert!(!o.status.success());
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(
        err.contains("ANTHROPIC_API_KEY") && err.contains("set-key"),
        "{err}"
    );
}
