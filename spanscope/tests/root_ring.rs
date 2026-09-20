//! Bounded retention integration test in an isolated test process.
#![cfg(feature = "enabled")]

#[spanscope::trace(root)]
fn retained_root() {}

#[test]
fn bounded_root_ring_retains_latest_ten_thousand() {
    for _ in 0..10_001 {
        retained_root();
    }
    let profile = spanscope::collection::snapshot();
    assert_eq!(profile.roots.len(), 10_000);
    assert_eq!(profile.evicted_roots, 1);
    assert_ne!(profile.roots[0].uid, profile.roots[9999].uid);
}
