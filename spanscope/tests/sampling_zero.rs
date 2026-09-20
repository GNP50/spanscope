//! Full subtree rejection at a zero sampling probability.
#![cfg(feature = "enabled")]

#[spanscope::trace(root)]
fn sampled_root() {
    let _child = spanscope::span!("sampled-child");
}

#[test]
fn sample_rate_zero_rejects_the_entire_subtree() {
    spanscope::collection::set_sample_rate(0.0).unwrap();
    sampled_root();
    let snapshot = spanscope::collection::snapshot();
    assert!(snapshot.roots.is_empty());
    assert!(snapshot.chains.is_empty());
    assert_eq!(snapshot.sampled_out_roots, 1);
}
