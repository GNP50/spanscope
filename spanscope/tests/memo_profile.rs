//! `#[memo]` counters in exported profiles and hit/miss attributes on traced roots.
#![cfg(all(feature = "memo", feature = "enabled"))]

use spanscope::collection::snapshot;
use spanscope::config::{ConfigBuilder, OutputFormat};
use spanscope::export::write_snapshot;
use spanscope::profile::Profile;

#[spanscope::memo(name = "profile.memo_outer")]
#[spanscope::trace(root, name = "memo_outer")]
fn memo_outer(value: u32) -> u32 {
    value + 1
}

#[spanscope::trace(root, name = "trace_outer")]
#[spanscope::memo(name = "profile.trace_outer")]
fn trace_outer(value: u32) -> u32 {
    value + 2
}

fn root_outcomes(profile: &Profile, span: &str, attribute: &str) -> Vec<String> {
    let id = profile
        .spans
        .iter()
        .find(|candidate| candidate.name == span)
        .expect("span exported")
        .id;
    profile
        .roots
        .iter()
        .filter(|root| root.span == id)
        .map(|root| root.attributes[attribute].clone())
        .collect()
}

#[test]
fn exported_profile_contains_memo_counters() {
    for _ in 0..3 {
        assert_eq!(memo_outer(1), 2);
        assert_eq!(trace_outer(1), 3);
    }
    let path = std::env::temp_dir().join(format!("spanscope-memo-{}.json", std::process::id()));
    let config = ConfigBuilder::new()
        .output(&path)
        .format(OutputFormat::Json)
        .auto_flush(false)
        .program("memo")
        .resolve()
        .unwrap();
    write_snapshot(&snapshot(), &config).unwrap();
    let profile: Profile = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    let _ = std::fs::remove_file(&path);

    assert!(profile
        .meta
        .features
        .iter()
        .any(|feature| feature == "memo"));
    // Hits stay inside the traced root whichever attribute comes first.
    for (span, attribute) in [
        ("memo_outer", "memo.profile.memo_outer"),
        ("trace_outer", "memo.profile.trace_outer"),
    ] {
        assert_eq!(
            root_outcomes(&profile, span, attribute),
            ["miss", "hit", "hit"],
            "{span}"
        );
    }

    let functions = &profile.memo.as_ref().expect("memo section").functions;
    assert!(!profile.analysis.sections.contains_key("memo"));
    for name in ["profile.memo_outer", "profile.trace_outer"] {
        let function = functions
            .iter()
            .find(|function| function.name == name)
            .expect("function exported");
        assert_eq!(
            (
                function.hits,
                function.misses,
                function.inserts,
                function.entries
            ),
            (2, 1, 1, 1)
        );
        assert_eq!(
            (
                function.evictions,
                function.bypasses,
                function.verify_failures
            ),
            (0, 0, 0)
        );
        assert!(function.bytes > 0);
    }
}
