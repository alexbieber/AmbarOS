#![allow(clippy::unwrap_used)]
use ambar_ai::generate::{extract_toml, parse_suggestions};
use proptest::prelude::*;

proptest! {
    #![proptest_config(ProptestConfig { cases: 3000, .. ProptestConfig::default() })]
    /// Model replies are untrusted: extraction and parsing must never panic.
    #[test]
    fn model_replies_never_panic(s in ".{0,300}", fence in prop::bool::ANY) {
        let reply = if fence { format!("```toml\n{s}\n```") } else { s };
        let _ = extract_toml(&reply);
        let _ = parse_suggestions(&reply);
    }
}
