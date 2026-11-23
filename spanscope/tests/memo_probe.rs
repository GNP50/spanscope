//! `#[trace(memo_probe)]` fingerprints and memoization insights in exported profiles.
#![cfg(feature = "enabled")]

use spanscope::collection::snapshot;
use spanscope::config::{ConfigBuilder, OutputFormat};
use spanscope::export::write_snapshot;
use spanscope::profile::{Profile, Severity};
use std::sync::atomic::{AtomicU64, Ordering};

#[spanscope::trace(root, name = "probe.pure", memo_probe(result))]
fn pure(value: u32) -> u64 {
    std::hint::black_box(u64::from(value) * 3)
}

static COUNTER: AtomicU64 = AtomicU64::new(0);

#[spanscope::trace(root, name = "probe.impure", memo_probe(result))]
fn impure(value: u32) -> u64 {
    u64::from(value) + COUNTER.fetch_add(1, Ordering::Relaxed)
}

#[spanscope::trace(root, name = "probe.partial", memo_probe(key = (value)))]
fn partial(value: f64, log: &mut Vec<f64>) -> f64 {
    log.push(value);
    value * 2.0
}

#[spanscope::trace(root, name = "probe.unique", memo_probe)]
fn unique(value: u32) -> u32 {
    value
}

struct Grid(u32);

impl Grid {
    #[spanscope::trace(root, name = "probe.method", memo_probe(result))]
    fn cell(&self, row: u32, label: &str) -> Option<String> {
        (row < self.0).then(|| format!("{label}{row}"))
    }
}

fn export() -> Profile {
    let path = std::env::temp_dir().join(format!("spanscope-probe-{}.json", std::process::id()));
    let config = ConfigBuilder::new()
        .output(&path)
        .format(OutputFormat::Json)
        .auto_flush(false)
        .program("probe")
        .resolve()
        .unwrap();
    write_snapshot(&snapshot(), &config).unwrap();
    let profile = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    let _ = std::fs::remove_file(&path);
    profile
}

#[test]
fn probes_report_repetition_and_consistency() {
    for _ in 0..5 {
        for value in 0..4 {
            pure(value);
            impure(value);
        }
    }
    let mut log = Vec::new();
    for value in [1.0, 1.0, -0.0, 0.0] {
        partial(value, &mut log);
    }
    for value in 0..20 {
        unique(value);
    }
    let grid = Grid(2);
    for row in [0, 0, 1, 3, 3] {
        grid.cell(row, "r");
    }

    let profile = export();
    let memo = profile.memo.as_ref().expect("memo section");
    let span = |name: &str| {
        profile
            .spans
            .iter()
            .find(|span| span.name == name)
            .unwrap()
            .id
    };
    let probe = |name: &str| {
        memo.probes
            .iter()
            .find(|probe| probe.span == span(name))
            .unwrap()
    };

    let pure_probe = probe("probe.pure");
    assert_eq!(
        (
            pure_probe.calls,
            pure_probe.distinct_args,
            pure_probe.repeated_calls
        ),
        (20, 4, 16)
    );
    assert!(pure_probe.results_checked);
    assert_eq!(pure_probe.inconsistent_args, 0);
    assert!(pure_probe.repeated_ns <= pure_probe.total_ns);
    assert!(pure_probe.repeated_self_ns <= pure_probe.self_ns);

    let impure_probe = probe("probe.impure");
    assert_eq!(
        (
            impure_probe.inconsistent_args,
            impure_probe.inconsistent_calls
        ),
        (4, 16)
    );

    // Floats fingerprint by bit pattern: 0.0 and -0.0 are distinct.
    let partial_probe = probe("probe.partial");
    assert_eq!(
        (partial_probe.distinct_args, partial_probe.repeated_calls),
        (3, 1)
    );
    assert!(!partial_probe.results_checked);

    let method = probe("probe.method");
    assert_eq!((method.distinct_args, method.repeated_calls), (3, 2));
    assert_eq!(probe("probe.unique").repeated_calls, 0);

    let insights = |kind: &str| {
        profile
            .analysis
            .insights
            .iter()
            .filter(|insight| insight.kind == kind)
            .map(|insight| insight.evidence.spans[0])
            .collect::<Vec<_>>()
    };
    assert_eq!(insights("memo.candidate"), [span("probe.pure")]);
    assert_eq!(insights("memo.inconsistent"), [span("probe.impure")]);
    let candidate = profile
        .analysis
        .insights
        .iter()
        .find(|insight| insight.kind == "memo.candidate")
        .unwrap();
    assert!(matches!(
        candidate.severity,
        Severity::Info | Severity::Warning
    ));
    assert!(candidate.explanation.contains("20 calls, 4 distinct"));
    assert_eq!(candidate.data["repeated_fraction"], 0.8);
    assert!(!candidate.evidence.chains.is_empty());
}
