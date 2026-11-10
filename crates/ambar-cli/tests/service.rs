//! Process-level tests: the real `ambar` and `ambard` binaries, separate
//! processes, a real socket, SIGKILL crash recovery.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

const AMBAR: &str = env!("CARGO_BIN_EXE_ambar");
const AMBARD: &str = env!("CARGO_BIN_EXE_ambard");

struct Env {
    _dir: tempfile::TempDir,
    root: PathBuf,
    data: PathBuf,
}

impl Env {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        std::fs::create_dir_all(root.join("home")).unwrap();
        Self {
            data: root.join("d"),
            root,
            _dir: dir,
        }
    }

    fn cmd(&self, program: &str) -> Command {
        let mut c = Command::new(program);
        c.env("HOME", self.root.join("home"))
            .env("AMBAR_DATA_DIR", &self.data)
            .env("AMBARD_BIN", AMBARD)
            .env("AMBARD_NOTIFY", "log")
            .env("AMBARD_MIN_INTERVAL", "100ms")
            .env_remove("RUST_BACKTRACE")
            .env_remove("XDG_DATA_HOME");
        c
    }

    fn ambar(&self, args: &[&str]) -> Output {
        self.cmd(AMBAR)
            .args(args)
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    fn ok(&self, args: &[&str]) -> String {
        let out = self.ambar(args);
        assert!(
            out.status.success(),
            "ambar {args:?} failed ({}):\n{}\n{}",
            out.status,
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    fn workflow_file(&self, name: &str, body: &str) -> PathBuf {
        let p = self.root.join(name);
        std::fs::write(&p, body.replace("$ROOT", &self.root.display().to_string())).unwrap();
        p
    }

    fn list(&self) -> Vec<(String, String)> {
        let json = self.ok(&["workflow", "list", "--json"]);
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        v.as_array()
            .unwrap()
            .iter()
            .map(|w| {
                (
                    w["id"].as_str().unwrap().to_string(),
                    w["state"].as_str().unwrap().to_string(),
                )
            })
            .collect()
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        // Never leave a daemon behind, even when a test fails.
        let _ = self.ambar(&["service", "stop"]);
    }
}

fn wait_for(mut cond: impl FnMut() -> bool, what: &str) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        if cond() {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("timed out waiting for {what}");
}

fn lines(p: &Path) -> usize {
    std::fs::read_to_string(p)
        .map(|s| s.lines().count())
        .unwrap_or(0)
}

const MANUAL: &str = r#"
id = "hello"
name = "Hello"
[trigger]
type = "manual"
[[actions]]
type = "write_file"
path = "$ROOT/out/hello.txt"
content = "hi"
append = true
[permissions]
write = ["$ROOT/out"]
"#;

const TICK: &str = r#"
id = "tick"
name = "Tick"
[trigger]
type = "schedule"
every = "200ms"
[[actions]]
type = "write_file"
path = "$ROOT/out/tick.log"
content = "tick"
append = true
[permissions]
write = ["$ROOT/out"]
"#;

#[test]
fn service_start_status_stop() {
    let e = Env::new();
    let out = e.ambar(&["service", "status"]);
    assert_eq!(out.status.code(), Some(3), "not running → exit 3");

    let started = e.ok(&["service", "start"]);
    assert!(started.contains("ambard started"), "{started}");
    let again = e.ok(&["service", "start"]);
    assert!(again.contains("already running"), "{again}");

    let status = e.ok(&["service", "status"]);
    assert!(status.contains("ambard is running (ok)"), "{status}");
    let json: serde_json::Value =
        serde_json::from_str(&e.ok(&["service", "status", "--json"])).unwrap();
    assert_eq!(json["status"], "ok");
    let pid = json["pid"].as_u64().unwrap();

    // A second ambard for the same data dir is refused (exit 2).
    let dup = e.cmd(AMBARD).output().unwrap();
    assert_eq!(
        dup.status.code(),
        Some(2),
        "{}",
        String::from_utf8_lossy(&dup.stderr)
    );

    assert!(e.ok(&["service", "stop"]).contains("ambard stopped"));
    assert_eq!(e.ambar(&["service", "status"]).status.code(), Some(3));
    // The process has really exited (gone, or a zombie awaiting reaping by
    // init — some container init processes never reap orphans).
    if cfg!(target_os = "linux") {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap_or_default();
        let state = stat
            .rsplit(')')
            .next()
            .unwrap_or("")
            .split_whitespace()
            .next();
        assert!(
            matches!(state, None | Some("Z") | Some("X")),
            "ambard still alive: {stat}"
        );
    }
    assert!(e.ok(&["service", "stop"]).contains("not running"));
}

#[test]
fn workflows_survive_service_restart() {
    let e = Env::new();
    e.ok(&["service", "start"]);
    let manual = e.workflow_file("m.toml", MANUAL);
    let tick = e.workflow_file("t.toml", TICK);
    e.ok(&[
        "workflow",
        "add",
        manual.to_str().unwrap(),
        "--yes",
        "--enable",
    ]);
    e.ok(&[
        "workflow",
        "add",
        tick.to_str().unwrap(),
        "--yes",
        "--enable",
    ]);

    // Without consent nothing is added.
    let declined = e.ambar(&["workflow", "add", manual.to_str().unwrap(), "--replace"]);
    assert!(!declined.status.success());

    let run = e.ok(&["workflow", "run", "hello"]);
    assert!(run.contains("succeeded"), "{run}");
    assert_eq!(lines(&e.root.join("out/hello.txt")), 1);

    let log = e.root.join("out/tick.log");
    wait_for(|| lines(&log) >= 2, "ticks");
    e.ok(&["workflow", "pause", "tick"]);
    e.ok(&["workflow", "disable", "hello"]);
    assert_eq!(
        e.list(),
        [
            ("hello".into(), "inactive".into()),
            ("tick".into(), "paused".into())
        ]
    );

    // Restart: states come back exactly.
    e.ok(&["service", "stop"]);
    e.ok(&["service", "start"]);
    assert_eq!(
        e.list(),
        [
            ("hello".into(), "inactive".into()),
            ("tick".into(), "paused".into())
        ]
    );
    let paused_at = lines(&log);
    std::thread::sleep(Duration::from_millis(600));
    assert_eq!(lines(&log), paused_at, "still paused after restart");
    e.ok(&["workflow", "resume", "tick"]);
    wait_for(|| lines(&log) > paused_at, "ticks after resume");
    e.ok(&["workflow", "cancel", "tick"]);

    // Errors are reported with a non-zero exit.
    let bad = e.ambar(&["workflow", "resume", "nope"]);
    assert!(!bad.status.success());
    assert!(String::from_utf8_lossy(&bad.stderr).contains("not registered"));

    // History and audit are shared with the plain CLI commands.
    let hist = e.ok(&["history", "--workflow", "hello"]);
    assert!(hist.contains("succeeded"), "{hist}");
    let audit = e.ok(&["audit"]);
    for ev in [
        "service.started",
        "ipc.register",
        "ipc.pause",
        "ipc.disable",
        "service.stopped",
    ] {
        assert!(audit.contains(ev), "missing {ev} in audit:\n{audit}");
    }
}

#[test]
fn foreground_run_is_refused_while_service_owns_the_data_dir() {
    let e = Env::new();
    e.ok(&["service", "start"]);
    let f = e.workflow_file("m.toml", MANUAL);
    let out = e.ambar(&["run", f.to_str().unwrap(), "--yes"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("is running for this data directory"));
    assert!(!e.root.join("out/hello.txt").exists());
}

#[test]
fn recovers_after_sigkill() {
    let e = Env::new();
    let mut child: Child = e
        .cmd(AMBARD)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    wait_for(
        || e.ambar(&["service", "status"]).status.success(),
        "ambard up",
    );
    let tick = e.workflow_file("t.toml", TICK);
    e.ok(&[
        "workflow",
        "add",
        tick.to_str().unwrap(),
        "--yes",
        "--enable",
    ]);
    let log = e.root.join("out/tick.log");
    wait_for(|| lines(&log) >= 2, "ticks");

    // Crash: no cleanup, socket and token left behind.
    child.kill().unwrap();
    child.wait().unwrap();
    assert!(e.data.join("run/ambard.sock").exists(), "stale socket left");
    assert_ne!(e.ambar(&["service", "status"]).status.code(), Some(0));

    // Restart recovers: stale socket replaced, workflow re-enabled, ticking.
    e.ok(&["service", "start"]);
    assert_eq!(e.list(), [("tick".into(), "active".into())]);
    let after = lines(&log);
    wait_for(|| lines(&log) > after, "ticks after recovery");
    let status = e.ok(&["service", "status"]);
    assert!(status.contains("(ok)"), "{status}");
}

#[test]
fn sigterm_is_a_graceful_shutdown() {
    let e = Env::new();
    let child = e
        .cmd(AMBARD)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    wait_for(
        || e.ambar(&["service", "status"]).status.success(),
        "ambard up",
    );
    let status = Command::new("kill")
        .args(["-TERM", &child.id().to_string()])
        .status()
        .unwrap();
    assert!(status.success());
    let mut child = child;
    let exit = child.wait().unwrap();
    assert_eq!(exit.code(), Some(0), "graceful exit code");
    assert!(
        !e.data.join("run/ambard.sock").exists(),
        "socket cleaned up"
    );
    let audit = e.ok(&["audit"]);
    assert!(audit.contains("service.stopped"), "{audit}");
}
