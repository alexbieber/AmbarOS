//! Property tests: hostile input to every parser must never panic, and
//! nothing that validates may escape its declared permissions.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use ambar_automation::permissions::lexical_path;
use ambar_automation::template::{Vars, referenced_vars, render};
use ambar_automation::{ValidationOptions, Workflow};
use proptest::prelude::*;

const BASE: &str = r#"
id = "t"
name = "T"
[trigger]
type = "folder_watch"
path = "/data/in"
pattern = "*.pdf"
[[conditions]]
type = "file_size"
max_bytes = 100
[[actions]]
type = "move_file"
to_dir = "/data/out/{date}"
[[actions]]
type = "notify"
title = "{file_name}"
[permissions]
write = ["/data/in", "/data/out"]
notify = true
"#;

proptest! {
    #![proptest_config(ProptestConfig { cases: 2000, .. ProptestConfig::default() })]

    #[test]
    fn arbitrary_text_never_panics(s in ".{0,400}") {
        if let Ok(wf) = Workflow::from_toml(&s) {
            let _ = wf.validate(&ValidationOptions::default());
        }
    }

    #[test]
    fn mutated_workflows_never_panic(pos in 0usize..BASE.len(), byte in any::<u8>(), cut in 0usize..BASE.len()) {
        let mut b = BASE.as_bytes().to_vec();
        b[pos] = byte;
        b.truncate(cut.max(pos + 1));
        if let Ok(s) = String::from_utf8(b)
            && let Ok(wf) = Workflow::from_toml(&s) {
                let _ = wf.validate(&ValidationOptions::default());
            }
    }

    #[test]
    fn templates_never_panic(t in ".{0,120}", v in "[^{}]{0,40}") {
        let _ = referenced_vars(&t);
        let mut vars = Vars::new();
        vars.insert("file_name", v.clone());
        vars.insert("date", "2026-01-01".into());
        if let Ok(out) = render(&t, &vars) {
            // Rendering only substitutes known variables; never invents braces.
            let brace = |x: &str| x.chars().filter(|c| *c == '{').count();
            let ok = brace(&out) <= brace(&t);
            prop_assert!(ok, "render added braces: {:?}", out);
        }
    }

    #[test]
    fn paths_never_panic_and_never_contain_parent(p in ".{0,80}") {
        if let Ok(path) = lexical_path(&p) {
            prop_assert!(path.is_absolute());
            prop_assert!(!path.components().any(|c| matches!(c, std::path::Component::ParentDir)));
        }
    }

    /// Any workflow that validates with a write-to path must have that path
    /// covered by its declared write permissions (statically checked).
    #[test]
    fn validated_write_targets_are_declared(dir in "[a-z]{1,8}", sub in "[a-z]{0,8}") {
        let src = format!(r#"
id = "w"
name = "W"
[trigger]
type = "manual"
[[actions]]
type = "write_file"
path = "/{dir}/{sub}/x.txt"
content = "x"
[permissions]
write = ["/data"]
"#);
        let wf = Workflow::from_toml(&src).unwrap();
        let ok = wf.validate(&ValidationOptions::default()).is_ok();
        prop_assert_eq!(ok, dir == "data", "dir={} sub={}", dir, sub);
    }
}
