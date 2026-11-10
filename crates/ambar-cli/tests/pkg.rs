//! `ambar pkg` end to end: the repo's marketplace, a real ambard.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const AMBAR: &str = env!("CARGO_BIN_EXE_ambar");
const AMBARD: &str = env!("CARGO_BIN_EXE_ambard");

fn marketplace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../marketplace")
        .canonicalize()
        .unwrap()
}

struct Env {
    _dir: tempfile::TempDir,
    root: PathBuf,
}

impl Env {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        for d in ["home/Downloads", "home/Desktop"] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        Self { _dir: dir, root }
    }
    fn run(&self, args: &[&str]) -> Output {
        Command::new(AMBAR)
            .args(args)
            .current_dir(&self.root)
            .env("HOME", self.root.join("home"))
            .env("AMBAR_DATA_DIR", self.root.join("d"))
            .env("AMBAR_CONFIG_DIR", self.root.join("cfg"))
            .env("AMBARD_BIN", AMBARD)
            .env("AMBARD_NOTIFY", "log")
            .env_remove("RUST_BACKTRACE")
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }
    fn ok(&self, args: &[&str]) -> String {
        let o = self.run(args);
        assert!(
            o.status.success(),
            "{args:?}\n{}\n{}",
            String::from_utf8_lossy(&o.stdout),
            String::from_utf8_lossy(&o.stderr)
        );
        String::from_utf8_lossy(&o.stdout).into_owned()
    }
    fn err(&self, args: &[&str]) -> String {
        let o = self.run(args);
        assert!(!o.status.success(), "{args:?} should fail");
        String::from_utf8_lossy(&o.stderr).into_owned()
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = self.run(&["service", "stop"]);
    }
}

#[test]
fn install_from_marketplace_then_remove() {
    let e = Env::new();
    let index = marketplace().join("index.json");
    let index = index.to_str().unwrap();
    e.ok(&["service", "start"]);

    let found = e.ok(&["pkg", "search", "screenshots", "--index", index]);
    assert!(found.contains("dev.ambar.screenshot-sorter"), "{found}");
    assert!(found.contains("untrusted"), "{found}");

    // Unknown publisher: refused without --trust (no terminal to ask).
    let err = e.err(&[
        "pkg",
        "install",
        "dev.ambar.tidy-downloads",
        "--index",
        index,
        "--yes",
    ]);
    assert!(err.contains("not trusted"), "{err}");

    // Trust the published key explicitly, then install.
    let pubkey = marketplace().join("ambar-examples.pub");
    e.ok(&["pkg", "trust", pubkey.to_str().unwrap()]);
    let out = e.ok(&[
        "pkg",
        "install",
        "dev.ambar.tidy-downloads",
        "--index",
        index,
        "--yes",
        "--enable",
    ]);
    assert!(out.contains("signature  valid"), "{out}");
    assert!(
        out.contains("installed dev.ambar.tidy-downloads 1.0.0 (enabled)"),
        "{out}"
    );
    let list = e.ok(&["workflow", "list"]);
    assert!(
        list.contains("tidy-downloads-documents") && list.contains("tidy-downloads-images"),
        "{list}"
    );
    assert!(
        e.ok(&["pkg", "list"])
            .contains("dev.ambar.tidy-downloads 1.0.0")
    );

    // The installed workflow really runs: drop a PDF into Downloads.
    std::fs::write(e.root.join("home/Downloads/report.pdf"), "pdf").unwrap();
    let date = chrono::Local::now().format("%Y-%m-%d").to_string();
    let dest = e
        .root
        .join(format!("home/Documents/Downloads/pdf/{date}/report.pdf"));
    for _ in 0..100 {
        if dest.exists() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    assert!(dest.exists(), "package workflow did not file the PDF");

    // Re-installing the same version is an upgrade-in-place, not a conflict.
    e.ok(&[
        "pkg",
        "install",
        "dev.ambar.tidy-downloads",
        "--index",
        index,
        "--yes",
    ]);

    e.ok(&["pkg", "remove", "dev.ambar.tidy-downloads"]);
    let list = e.ok(&["workflow", "list"]);
    assert!(!list.contains("tidy-downloads"), "{list}");
    assert!(e.ok(&["pkg", "list"]).contains("No packages"));
}

#[test]
fn tampered_and_mismatched_packages_are_refused() {
    let e = Env::new();
    e.ok(&["service", "start"]);
    let src = marketplace().join("packages/dev.ambar.daily-notes-1.0.0.ambarpkg");
    let text = std::fs::read_to_string(&src).unwrap();
    let tampered = e.root.join("evil.ambarpkg");
    std::fs::write(&tampered, text.replace("Documents/Notes", ".ssh")).unwrap();
    let err = e.err(&[
        "pkg",
        "install",
        tampered.to_str().unwrap(),
        "--trust",
        "--yes",
    ]);
    assert!(err.contains("signature does not match"), "{err}");

    // An index whose hash doesn't match the file it points to.
    let idx_dir = e.root.join("market");
    std::fs::create_dir_all(idx_dir.join("packages")).unwrap();
    std::fs::copy(
        &src,
        idx_dir.join("packages/dev.ambar.daily-notes-1.0.0.ambarpkg"),
    )
    .unwrap();
    let mut idx: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(marketplace().join("index.json")).unwrap())
            .unwrap();
    for p in idx["packages"].as_array_mut().unwrap() {
        p["sha256"] = "0".repeat(64).into();
    }
    let idx_path = idx_dir.join("index.json");
    std::fs::write(&idx_path, idx.to_string()).unwrap();
    let err = e.err(&[
        "pkg",
        "install",
        "dev.ambar.daily-notes",
        "--index",
        idx_path.to_str().unwrap(),
        "--trust",
        "--yes",
    ]);
    assert!(err.contains("does not match the index"), "{err}");
    assert!(!e.ok(&["workflow", "list"]).contains("daily-note"));
}

#[test]
fn author_workflow_keygen_build_publish_install() {
    let e = Env::new();
    e.ok(&["service", "start"]);
    let src = e.root.join("mypkg");
    std::fs::create_dir_all(src.join("wf")).unwrap();
    std::fs::write(
        src.join("ambar-package.toml"),
        "name = \"dev.me.hello\"\nversion = \"0.1.0\"\ndescription = \"hi\"\nauthor = \"Me\"\nworkflows = [\"wf/hello.toml\"]\n",
    )
    .unwrap();
    let out_dir = e.root.join("home/out");
    std::fs::write(
        src.join("wf/hello.toml"),
        format!(
            "id = \"hello-pkg\"\nname = \"Hello\"\n[trigger]\ntype = \"manual\"\n[[actions]]\ntype = \"write_file\"\npath = \"{o}/hi.txt\"\ncontent = \"hi\"\n[permissions]\nwrite = [\"{o}\"]\n",
            o = out_dir.display()
        ),
    )
    .unwrap();
    assert!(e.ok(&["pkg", "keygen", "Me"]).contains("fingerprint"));
    e.ok(&[
        "pkg",
        "build",
        src.to_str().unwrap(),
        "--key",
        "me",
        "-o",
        "hello.ambarpkg",
    ]);
    let v = e.ok(&["pkg", "verify", "hello.ambarpkg"]);
    assert!(
        v.contains("dev.me.hello 0.1.0") && v.contains("NOT trusted"),
        "{v}"
    );
    let idx = e.root.join("market/index.json");
    e.ok(&[
        "pkg",
        "publish",
        "hello.ambarpkg",
        "--index",
        idx.to_str().unwrap(),
    ]);
    e.ok(&[
        "pkg",
        "install",
        "dev.me.hello",
        "--index",
        idx.to_str().unwrap(),
        "--trust",
        "--yes",
        "--enable",
    ]);
    assert!(
        e.ok(&["workflow", "run", "hello-pkg"])
            .contains("succeeded")
    );
    assert!(out_dir.join("hi.txt").exists());
}
