//! Bounded root-local interval evidence and RSS capability reporting.
#![cfg(feature = "enabled")]

#[spanscope::trace(name = "evidence_leaf")]
fn evidence_leaf() {
    std::hint::black_box(1);
}

#[spanscope::trace(root, name = "evidence_root")]
fn evidence_root() {
    for _ in 0..4_100 {
        evidence_leaf();
    }
}

#[test]
fn root_evidence_is_bounded_and_reports_truncation() {
    evidence_root();
    let profile = spanscope::collection::snapshot();
    let root = profile
        .roots
        .iter()
        .find(|root| profile.spans[root.span as usize].name == "evidence_root")
        .unwrap();
    assert_eq!(root.invocations.len(), 4_096);
    assert!(root.evidence_truncated);
    assert_eq!(
        root.chains.iter().map(|chain| chain.calls).sum::<u64>(),
        4_101
    );
    assert!(root
        .invocations
        .iter()
        .all(|invocation| invocation.start_ns <= invocation.end_ns));
    assert_eq!(
        profile.rss_supported,
        cfg!(all(feature = "memory", target_os = "linux"))
    );
    if profile.rss_supported {
        assert!(root.rss_entry_kb > 0);
        assert!(root.rss_exit_kb > 0);
    } else {
        assert_eq!((root.rss_entry_kb, root.rss_exit_kb), (0, 0));
    }
}
