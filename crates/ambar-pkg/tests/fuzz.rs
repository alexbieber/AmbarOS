//! Property tests: no byte-level change to a signed package can verify as
//! different content, and arbitrary input never panics the verifier.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::OnceLock;

use ambar_pkg::{Package, Publisher};
use proptest::prelude::*;

fn signed() -> &'static (Vec<u8>, Package) {
    static P: OnceLock<(Vec<u8>, Package)> = OnceLock::new();
    P.get_or_init(|| {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("w")).unwrap();
        std::fs::write(dir.path().join("ambar-package.toml"),
            "name = \"dev.t.p\"\nversion = \"1.0.0\"\ndescription = \"d\"\nauthor = \"a\"\nworkflows = [\"w/a.toml\"]\n").unwrap();
        std::fs::write(dir.path().join("w/a.toml"),
            "id = \"a\"\nname = \"A\"\n[trigger]\ntype = \"manual\"\n[[actions]]\ntype = \"create_directory\"\npath = \"/data/x\"\n[permissions]\nwrite = [\"/data\"]\n").unwrap();
        let p = Package::build(dir.path(), &Publisher::generate("t").unwrap()).unwrap();
        (p.to_bytes().unwrap(), p)
    })
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 3000, .. ProptestConfig::default() })]

    #[test]
    fn single_byte_changes_never_verify_as_different_content(pos in any::<prop::sample::Index>(), byte in any::<u8>()) {
        let (bytes, original) = signed();
        let mut b = bytes.clone();
        let i = pos.index(b.len());
        b[i] = byte;
        if let Ok(v) = Package::verify(&b) {
            // Only formatting-level changes (e.g. whitespace) may still verify,
            // and then the content must be exactly the signed content.
            prop_assert_eq!(&v.package.manifest, &original.manifest);
            prop_assert_eq!(&v.package.files, &original.files);
        }
    }

    #[test]
    fn arbitrary_bytes_never_panic(b in proptest::collection::vec(any::<u8>(), 0..2048)) {
        let _ = Package::verify(&b);
    }
}
