//! Heap balance per span: freed bytes, net change, peak, and inclusive totals.
#![cfg(feature = "alloc-tracker")]

use spanscope::collection::{ChainSnapshot, Snapshot};

#[global_allocator]
static ALLOCATOR: spanscope::TrackingAllocator<std::alloc::System> =
    spanscope::TrackingAllocator::new(std::alloc::System);

fn chain<'a>(snapshot: &'a Snapshot, name: &str) -> &'a ChainSnapshot {
    snapshot
        .chains
        .iter()
        .find(|chain| snapshot.spans[*chain.path.last().unwrap() as usize].name == name)
        .unwrap_or_else(|| panic!("missing chain {name}"))
}

#[spanscope::trace(root, name = "balance_root")]
fn balance_root() -> Vec<u8> {
    let kept = balance_retains();
    balance_transient();
    kept
}

/// Returns its allocation: net growth equals the allocation.
#[spanscope::trace(name = "balance_retains")]
fn balance_retains() -> Vec<u8> {
    let bytes = Vec::<u8>::with_capacity(1000);
    std::hint::black_box(bytes)
}

/// Allocates, grows, and frees: zero net change, peak while both blocks exist.
#[spanscope::trace(name = "balance_transient")]
fn balance_transient() {
    let mut bytes = Vec::<u8>::with_capacity(100);
    std::hint::black_box(&mut bytes);
    bytes.reserve_exact(300);
    std::hint::black_box(&mut bytes);
    drop(bytes);
}

#[spanscope::trace(root, name = "balance_releaser")]
fn balance_releaser(bytes: Vec<u8>) {
    drop(std::hint::black_box(bytes));
}

#[test]
fn spans_report_freed_net_peak_and_inclusive_bytes() {
    let kept = balance_root();
    balance_releaser(kept);
    let snapshot = spanscope::collection::snapshot();
    assert!(snapshot.heap_tracked);

    let retains = chain(&snapshot, "balance_retains");
    assert_eq!(retains.alloc_bytes, 1000);
    assert_eq!(retains.freed_bytes, 0);
    assert_eq!(retains.net_bytes_max, 1000);
    assert_eq!(retains.net_bytes_min, 1000);
    assert_eq!(retains.peak_bytes_max, 1000);
    assert_eq!(retains.retaining_calls, 1);

    let transient = chain(&snapshot, "balance_transient");
    assert_eq!(transient.allocs, 2);
    assert_eq!(transient.alloc_bytes, 400);
    assert_eq!(transient.frees, 2);
    assert_eq!(transient.freed_bytes, 400);
    assert_eq!(transient.net_bytes_max, 0);
    assert_eq!(transient.peak_bytes_max, 400);
    assert_eq!(transient.retaining_calls, 0);
    assert_eq!(transient.peak_histogram.iter().map(|b| b.1).sum::<u64>(), 1);

    let root = chain(&snapshot, "balance_root");
    assert_eq!(root.alloc_bytes, 0);
    assert_eq!(root.incl_alloc_bytes, 1400);
    assert_eq!(root.incl_freed_bytes, 400);
    assert_eq!(root.net_bytes_max, 1000);
    // The retained 1000 bytes stay live while the transient child peaks at 400.
    assert_eq!(root.peak_bytes_max, 1400);

    let releaser = chain(&snapshot, "balance_releaser");
    assert_eq!(releaser.freed_bytes, 1000);
    assert_eq!(releaser.net_bytes_min, -1000);
    assert_eq!(releaser.peak_bytes_max, 0);

    let root_record = snapshot
        .roots
        .iter()
        .find(|root| snapshot.spans[root.span as usize].name == "balance_root")
        .unwrap();
    assert_eq!(root_record.heap_alloc_bytes, 1400);
    assert_eq!(root_record.heap_freed_bytes, 400);
    assert_eq!(root_record.heap_peak_bytes, 1400);
}

#[spanscope::trace(root, name = "balance_async")]
async fn balance_async() -> usize {
    let first = Vec::<u8>::with_capacity(512);
    Yield(false).await;
    let second = Vec::<u8>::with_capacity(256);
    drop(first);
    std::hint::black_box(&second);
    second.capacity()
}

struct Yield(bool);

impl std::future::Future for Yield {
    type Output = ();
    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<()> {
        if self.0 {
            std::task::Poll::Ready(())
        } else {
            self.0 = true;
            cx.waker().wake_by_ref();
            std::task::Poll::Pending
        }
    }
}

#[test]
fn async_spans_accumulate_balance_across_polls() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    runtime.block_on(balance_async());
    let snapshot = spanscope::collection::snapshot();
    let chain = chain(&snapshot, "balance_async");
    assert_eq!(chain.poll_count, 2);
    assert_eq!(chain.alloc_bytes, 768);
    assert_eq!(chain.freed_bytes, 768);
    assert_eq!(chain.net_bytes_max, 0);
    assert_eq!(chain.peak_bytes_max, 768);
}

#[spanscope::trace(root, name = "balance_cache_fill")]
fn balance_cache_fill(cache: &mut Vec<Vec<u8>>) {
    cache.push(vec![1u8; 80 * 1024]);
}

#[spanscope::trace(root, name = "balance_churn")]
fn balance_churn() -> usize {
    let mut total = 0;
    for index in 0..64 {
        let piece = std::hint::black_box(vec![index as u8; 24]);
        total += piece.len();
    }
    total
}

#[spanscope::trace(root, name = "balance_spike")]
fn balance_spike() -> usize {
    let buffer = std::hint::black_box(vec![0u8; 2 * 1024 * 1024]);
    buffer.len()
}

#[test]
fn export_carries_memory_sections_and_insights() {
    use spanscope::config::{ConfigBuilder, OutputFormat};
    use spanscope::profile::Profile;

    let mut cache = Vec::new();
    for _ in 0..4 {
        balance_cache_fill(&mut cache);
    }
    for _ in 0..40 {
        balance_churn();
    }
    balance_spike();
    let path = std::env::temp_dir().join(format!("spanscope-memory-{}.json", std::process::id()));
    let config = ConfigBuilder::new()
        .output(&path)
        .format(OutputFormat::Json)
        .auto_flush(false)
        .program("memory")
        .resolve()
        .unwrap();
    spanscope::export::write_snapshot(&spanscope::collection::snapshot(), &config).unwrap();
    let profile: Profile = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    let _ = std::fs::remove_file(&path);
    drop(cache);

    assert!(profile.meta.capture.heap_tracked);
    assert!(profile.chains.iter().all(|chain| chain.memory.is_some()));
    assert!(profile.roots.iter().all(|root| root.memory.is_some()));
    let kinds = profile
        .analysis
        .insights
        .iter()
        .map(|insight| insight.kind.as_str())
        .collect::<Vec<_>>();
    for expected in ["memory.retention", "memory.churn", "memory.peak"] {
        assert!(kinds.contains(&expected), "missing {expected} in {kinds:?}");
    }
}
