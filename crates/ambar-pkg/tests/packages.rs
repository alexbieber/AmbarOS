#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;

use ambar_pkg::{
    Index, Package, PkgError, Publisher, Registry, TrustStore, fingerprint, sha256_hex,
};

fn source(dir: &Path, extra_manifest: &str) {
    std::fs::create_dir_all(dir.join("workflows")).unwrap();
    std::fs::write(
        dir.join("ambar-package.toml"),
        format!(
            r#"
name = "dev.test.notes"
version = "1.2.0"
description = "Daily notes helpers"
author = "Test Author"
license = "Apache-2.0"
workflows = ["workflows/daily.toml"]
{extra_manifest}
"#
        ),
    )
    .unwrap();
    std::fs::write(
        dir.join("workflows/daily.toml"),
        r##"
id = "daily-note"
name = "Daily note"
[trigger]
type = "schedule"
cron = "0 9 * * *"
[[actions]]
type = "write_file"
path = "~/Notes/{date}.md"
content = "# {date}"
append = true
[permissions]
write = ["~/Notes"]
"##,
    )
    .unwrap();
}

fn built() -> (tempfile::TempDir, Publisher, Vec<u8>) {
    let dir = tempfile::tempdir().unwrap();
    source(dir.path(), "");
    let p = Publisher::generate("Test Author").unwrap();
    let bytes = Package::build(dir.path(), &p).unwrap().to_bytes().unwrap();
    (dir, p, bytes)
}

#[test]
fn build_and_verify_round_trip() {
    let (_d, p, bytes) = built();
    let v = Package::verify(&bytes).unwrap();
    assert_eq!(v.package.manifest.name, "dev.test.notes");
    assert_eq!(v.workflows[0].id, "daily-note");
    assert_eq!(v.fingerprint, fingerprint(&p.public().public));
    assert!(v.risks().is_empty());
}

#[test]
fn any_tampering_breaks_the_signature() {
    let (_d, _p, bytes) = built();
    let text = String::from_utf8(bytes).unwrap();
    for (from, to) in [
        ("~/Notes", "~/.ssh"),                   // widen permissions
        ("\\\"1.2.0\\\"", "\\\"9.9.9\\\""),      // unlikely escape form
        ("\"1.2.0\"", "\"1.2.1\""),              // bump version
        ("Daily notes helpers", "Totally safe"), // change description
    ] {
        if !text.contains(from) {
            continue;
        }
        let tampered = text.replacen(from, to, 1);
        let err = Package::verify(tampered.as_bytes()).unwrap_err();
        assert!(matches!(err, PkgError::Verification(_)), "{from}: {err}");
    }
}

#[test]
fn re_signing_with_another_key_changes_the_fingerprint() {
    let (dir, p, bytes) = built();
    let other = Publisher::generate("Test Author").unwrap(); // same claimed name
    let forged = Package::build(dir.path(), &other)
        .unwrap()
        .to_bytes()
        .unwrap();
    let a = Package::verify(&bytes).unwrap();
    let b = Package::verify(&forged).unwrap();
    assert_ne!(a.fingerprint, b.fingerprint, "trust is by key, not by name");

    let trust_dir = tempfile::tempdir().unwrap();
    let trust = TrustStore::open(trust_dir.path());
    assert!(matches!(trust.require(&a), Err(PkgError::Untrusted { .. })));
    trust.trust(&p.public()).unwrap();
    trust.require(&a).unwrap();
    assert!(matches!(trust.require(&b), Err(PkgError::Untrusted { .. })));
    assert!(trust.untrust(&a.fingerprint));
    assert!(trust.require(&a).is_err());
}

#[test]
fn invalid_contents_are_refused_at_build_time() {
    let p = Publisher::generate("x").unwrap();
    for (manifest, expect) in [
        (r#"name = "Bad Name""#, "package name"),
        (r#"version = "1.0""#, "MAJOR.MINOR.PATCH"),
        (r#"workflows = ["../outside.toml"]"#, "relative path"),
        (r#"workflows = ["/etc/passwd.toml"]"#, "relative path"),
    ] {
        let dir = tempfile::tempdir().unwrap();
        source(dir.path(), "");
        let m = dir.path().join("ambar-package.toml");
        let text = std::fs::read_to_string(&m).unwrap();
        let key = manifest.split(" =").next().unwrap();
        let replaced: String = text
            .lines()
            .map(|l| {
                if l.starts_with(key) {
                    manifest.to_string()
                } else {
                    l.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(&m, replaced).unwrap();
        let err = Package::build(dir.path(), &p).unwrap_err().to_string();
        assert!(err.contains(expect), "{manifest}: {err}");
    }
    // Invalid workflow inside.
    let dir = tempfile::tempdir().unwrap();
    source(dir.path(), "");
    std::fs::write(
        dir.path().join("workflows/daily.toml"),
        "id = 'x'\n[trigger]\ntype = 'manual'\n[[actions]]\ntype = 'run_shell'\n",
    )
    .unwrap();
    assert!(Package::build(dir.path(), &p).is_err());
}

#[test]
fn smuggled_files_fail_verification() {
    let (_d, _p, bytes) = built();
    let mut v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    v["files"]["workflows/extra.toml"] = "id = 'x'".into();
    let err = Package::verify(&serde_json::to_vec(&v).unwrap()).unwrap_err();
    assert!(matches!(err, PkgError::Verification(_)), "{err}");
}

#[test]
fn keys_save_and_load() {
    let dir = tempfile::tempdir().unwrap();
    let p = Publisher::generate("Me").unwrap();
    let (k, _pubf) = p.save(dir.path(), "me").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&k).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    let loaded = Publisher::load(&k).unwrap();
    assert_eq!(loaded.public(), p.public());
}

#[test]
fn index_entries_check_hash_and_signer() {
    let (_d, _p, bytes) = built();
    let v = Package::verify(&bytes).unwrap();
    let idx = Index {
        format: 1,
        name: "Test".into(),
        packages: vec![Index::entry_for(&v, "notes.ambarpkg")],
    };
    let json = serde_json::to_vec(&idx).unwrap();
    let idx = Index::parse(&json).unwrap();
    assert_eq!(idx.search("notes").len(), 1);
    assert_eq!(idx.search("NOTES").len(), 1);
    assert!(idx.search("nope").is_empty());
    let e = idx.find("dev.test.notes").unwrap();
    e.check(&bytes).unwrap();
    // A different (validly signed) file under the same index entry is refused.
    let (_d2, _p2, other) = built();
    assert!(matches!(e.check(&other), Err(PkgError::Verification(_))));
    assert_eq!(e.sha256, sha256_hex(&bytes));
}

#[test]
fn registry_records_installs() {
    let dir = tempfile::tempdir().unwrap();
    let r = Registry::open(dir.path());
    r.record(&ambar_pkg::Installed {
        name: "dev.test.notes".into(),
        version: "1.2.0".into(),
        signer: "abc".into(),
        publisher: "T".into(),
        workflows: vec!["daily-note".into()],
        installed_at: "now".into(),
    })
    .unwrap();
    assert_eq!(r.list().len(), 1);
    assert!(r.get("dev.test.notes").is_some());
    assert!(r.get("../../etc/passwd").is_none());
    assert!(r.forget("dev.test.notes"));
    assert!(r.list().is_empty());
}
