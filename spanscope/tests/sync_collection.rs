//! End-to-end synchronous collection and snapshot behavior.
#![cfg(feature = "enabled")]

use spanscope::collection::{snapshot, Snapshot};

fn chain<'a>(profile: &'a Snapshot, name: &str) -> &'a spanscope::collection::ChainSnapshot {
    profile
        .chains
        .iter()
        .find(|chain| {
            chain.path.last().is_some_and(|id| {
                let actual = profile.spans[*id as usize].name;
                actual == name || actual.rsplit("::").next() == Some(name)
            })
        })
        .unwrap_or_else(|| panic!("missing chain {name}"))
}

#[spanscope::trace]
fn nested_child() {
    std::hint::black_box(1u32.wrapping_add(1));
}

#[spanscope::trace(root, name = "fixture::nested_root", tags("test", "root"))]
fn nested_root() {
    nested_child();
    nested_child();
    let _manual = spanscope::span!("manual-inner");
    spanscope::metric!("n_channels", 96);
    std::hint::black_box(3u32.wrapping_mul(7));
}

#[test]
fn nested_root_records_local_deltas_and_exclusive_time() {
    nested_root();
    let profile = snapshot();
    let parent = chain(&profile, "fixture::nested_root");
    let child = chain(&profile, "nested_child");
    let manual = chain(&profile, "manual-inner");
    assert!(parent.count >= 1);
    assert!(child.count >= 2);
    assert!(manual.count >= 1);
    assert!(parent.self_ns <= parent.total_ns);
    assert_eq!(parent.active_ns, parent.total_ns);
    let root = profile
        .roots
        .iter()
        .rev()
        .find(|root| profile.spans[root.span as usize].name == "fixture::nested_root")
        .unwrap();
    assert_eq!(root.metrics["n_channels"], 96.0);
    assert_eq!(root.chains.iter().map(|chain| chain.calls).sum::<u64>(), 4);
    assert!(root.duration_ns >= root.chains.iter().find(|c| c.calls == 2).unwrap().total_ns);
    assert_eq!(
        profile.spans[parent.path[0] as usize].tags,
        &["test", "root"]
    );
}

struct Calculator(u32);

impl Calculator {
    #[spanscope::trace(name = "fixture::generic_method")]
    fn calculate<T: Into<u32>>(&self, input: T) -> u32 {
        self.0 + input.into()
    }
}

#[spanscope::trace]
fn impl_trait_result() -> impl Iterator<Item = u32> {
    [1, 2, 3].into_iter()
}

#[test]
fn methods_generics_and_impl_trait_preserve_signatures() {
    assert_eq!(Calculator(10).calculate(5u8), 15);
    assert_eq!(impl_trait_result().sum::<u32>(), 6);
    let profile = snapshot();
    assert!(chain(&profile, "fixture::generic_method").count >= 1);
    assert!(chain(&profile, "impl_trait_result").count >= 1);
}

#[spanscope::trace]
fn unwind_span() {
    panic!("synthetic user panic");
}

#[test]
fn unwinding_records_cancellation_without_masking_host_panic() {
    assert!(std::panic::catch_unwind(unwind_span).is_err());
    let profile = snapshot();
    let span = chain(&profile, "unwind_span");
    assert!(span.cancelled >= 1);
    assert!(span.self_ns <= span.total_ns);
}

#[spanscope::trace]
fn worker_span() {
    std::hint::black_box(1u32.wrapping_add(1));
}

#[test]
fn worker_exit_keeps_exact_call_counts_across_repeated_snapshots() {
    let workers: Vec<_> = (0..8)
        .map(|_| {
            std::thread::spawn(|| {
                for _ in 0..1500 {
                    worker_span();
                }
            })
        })
        .collect();
    for worker in workers {
        worker.join().unwrap();
    }
    for _ in 0..2 {
        let profile = snapshot();
        assert_eq!(chain(&profile, "worker_span").count, 12_000);
    }
}

#[test]
fn rejected_sampling_subtrees_have_no_observed_calls() {
    // Sampling is process-wide, so use a dedicated binary test process in a later
    // feature-combination test. The unit test covers select() extremes.
    assert!(spanscope::collection::set_sample_rate(-0.1).is_err());
    assert!(spanscope::collection::set_sample_rate(f64::NAN).is_err());
}

#[spanscope::trace]
fn unpublished_worker_span() {
    std::hint::black_box(1);
}

#[test]
fn snapshot_marks_idle_worker_with_unpublished_observations_pending() {
    use std::sync::{Arc, Barrier};
    let started = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    let worker_started = Arc::clone(&started);
    let worker_release = Arc::clone(&release);
    let worker = std::thread::Builder::new()
        .name("publication-worker".to_owned())
        .spawn(move || {
            unpublished_worker_span();
            worker_started.wait();
            worker_release.wait();
        })
        .unwrap();
    started.wait();
    let partial = snapshot();
    let thread = partial
        .threads
        .iter()
        .find(|thread| thread.name.as_deref() == Some("publication-worker"))
        .unwrap();
    assert!(partial.pending_threads.contains(&thread.id));
    // Another test can request an earlier epoch concurrently, causing this
    // worker to publish before this snapshot. Pending is the reliable signal.
    release.wait();
    worker.join().unwrap();
    let complete = snapshot();
    assert_eq!(chain(&complete, "unpublished_worker_span").count, 1);
    assert!(!complete.pending_threads.contains(&thread.id));
}
