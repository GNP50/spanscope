//! Poll-scoped ancestry and explicit propagation contracts.
#![cfg(feature = "enabled")]

use spanscope::collection::{snapshot, Snapshot};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Barrier};
use std::task::{Context, Poll, Wake, Waker};

struct Noop;
impl Wake for Noop {
    fn wake(self: Arc<Self>) {}
}

fn poll<F: Future + Unpin>(future: &mut F) -> Poll<F::Output> {
    let waker = Waker::from(Arc::new(Noop));
    Pin::new(future).poll(&mut Context::from_waker(&waker))
}

struct YieldOnce(bool);
impl Future for YieldOnce {
    type Output = ();
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        if self.0 {
            Poll::Ready(())
        } else {
            self.0 = true;
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    }
}

#[spanscope::trace(name = "async_child")]
async fn async_child() {
    YieldOnce(false).await;
    sync_child();
}

#[spanscope::trace(name = "sync_child")]
fn sync_child() {
    std::hint::black_box(1);
}

#[spanscope::trace(root, name = "async_root")]
async fn async_root() {
    async_child().await;
}

fn named<'a>(snapshot: &'a Snapshot, name: &str) -> &'a spanscope::collection::ChainSnapshot {
    snapshot
        .chains
        .iter()
        .find(|chain| snapshot.spans[*chain.path.last().unwrap() as usize].name == name)
        .unwrap()
}

#[test]
fn interleaving_migration_and_cancellation_preserve_paths_and_send() {
    fn require_send<T: Send>(_: &T) {}
    let mut a = Box::pin(async_root());
    let mut b = Box::pin(async_root());
    require_send(&a);
    assert!(poll(&mut a).is_pending());
    assert!(poll(&mut b).is_pending());
    std::thread::spawn(move || assert!(poll(&mut a).is_ready()))
        .join()
        .unwrap();
    drop(b);
    let profile = snapshot();
    let root = named(&profile, "async_root");
    let child = profile
        .chains
        .iter()
        .find(|chain| {
            chain.path.len() == 2
                && profile.spans[chain.path[0] as usize].name == "async_root"
                && profile.spans[chain.path[1] as usize].name == "async_child"
        })
        .unwrap();
    let sync = profile
        .chains
        .iter()
        .find(|chain| {
            chain.path.len() == 3
                && profile.spans[chain.path[0] as usize].name == "async_root"
                && profile.spans[chain.path[2] as usize].name == "sync_child"
        })
        .unwrap();
    assert_eq!(root.count, 2);
    assert_eq!(root.cancelled, 1);
    assert_eq!(root.poll_count, 3);
    assert!(root.self_ns <= root.active_ns);
    assert!(root.active_ns <= root.total_ns);
    assert_eq!(child.count, 2);
    assert_eq!(child.cancelled, 1);
    assert_eq!(child.poll_count, 3);
    assert!(child.self_ns <= child.active_ns);
    assert_eq!(sync.count, 1);
    assert_eq!(sync.path, vec![root.path[0], child.path[1], sync.path[2]]);
    assert_eq!(
        profile
            .roots
            .iter()
            .filter(|root| profile.spans[root.span as usize].name == "async_root")
            .count(),
        2
    );
    let cancelled = profile
        .roots
        .iter()
        .find(|root| profile.spans[root.span as usize].name == "async_root" && root.cancelled)
        .unwrap();
    assert_eq!(
        cancelled
            .chains
            .iter()
            .map(|chain| chain.calls)
            .sum::<u64>(),
        2
    );
}

#[spanscope::trace(root, name = "propagation_root")]
fn propagation_root() {
    let parent = spanscope::context::propagate();
    std::thread::spawn(move || {
        let _attached = parent.attach();
        sync_child();
        spanscope::metric!("workers", 1);
    })
    .join()
    .unwrap();
}

#[test]
fn propagated_thread_updates_only_its_root() {
    propagation_root();
    let profile = snapshot();
    let root = profile
        .roots
        .iter()
        .find(|root| profile.spans[root.span as usize].name == "propagation_root")
        .unwrap();
    assert_eq!(root.metrics["workers"], 1.0);
    assert_eq!(root.chains.iter().map(|chain| chain.calls).sum::<u64>(), 2);
    assert!(!root.incomplete);
    assert!(profile.chains.iter().any(|chain| {
        profile.spans[chain.path[0] as usize].name == "propagation_root"
            && profile.spans[*chain.path.last().unwrap() as usize].name == "sync_child"
    }));
}

#[test]
fn late_attached_work_marks_root_incomplete() {
    let started = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    #[spanscope::trace(root, name = "early_root")]
    fn start_child(started: Arc<Barrier>, release: Arc<Barrier>) -> std::thread::JoinHandle<()> {
        let parent = spanscope::context::propagate();
        let worker_started = Arc::clone(&started);
        let worker = std::thread::spawn(move || {
            let _attached = parent.attach();
            worker_started.wait();
            release.wait();
            sync_child();
        });
        started.wait();
        worker
    }
    let worker = start_child(Arc::clone(&started), Arc::clone(&release));
    release.wait();
    worker.join().unwrap();
    let profile = snapshot();
    let root = profile
        .roots
        .iter()
        .find(|root| profile.spans[root.span as usize].name == "early_root")
        .unwrap();
    assert!(root.incomplete);
    assert_eq!(root.chains.iter().map(|chain| chain.calls).sum::<u64>(), 1);
}

#[spanscope::trace(root, name = "wrapped_root")]
fn wrapped_root() {
    let parent = spanscope::context::propagate();
    std::thread::spawn(move || {
        let mut future = Box::pin(parent.wrap(async_child()));
        assert!(poll(&mut future).is_pending());
        assert!(poll(&mut future).is_ready());
    })
    .join()
    .unwrap();
}

#[test]
fn future_attachment_survives_pending_and_migration_boundary() {
    wrapped_root();
    let profile = snapshot();
    let root = profile
        .roots
        .iter()
        .find(|root| profile.spans[root.span as usize].name == "wrapped_root")
        .unwrap();
    let paths: Vec<_> = root
        .chains
        .iter()
        .map(|chain| {
            chain
                .path
                .iter()
                .map(|id| profile.spans[*id as usize].name)
                .collect::<Vec<_>>()
        })
        .collect();
    assert!(paths.contains(&vec!["wrapped_root", "async_child"]));
    assert!(paths.contains(&vec!["wrapped_root", "async_child", "sync_child"]));
    assert!(!root.incomplete);
}

#[spanscope::trace(root, name = "parallel_root")]
fn parallel_root(index: u32, barrier: Arc<Barrier>) {
    spanscope::metric!("index", index);
    barrier.wait();
    sync_child();
}

#[test]
fn concurrent_roots_keep_local_metrics() {
    let barrier = Arc::new(Barrier::new(2));
    let a = {
        let barrier = Arc::clone(&barrier);
        std::thread::spawn(move || parallel_root(1, barrier))
    };
    let b = {
        let barrier = Arc::clone(&barrier);
        std::thread::spawn(move || parallel_root(2, barrier))
    };
    a.join().unwrap();
    b.join().unwrap();
    let profile = snapshot();
    let roots: Vec<_> = profile
        .roots
        .iter()
        .filter(|root| profile.spans[root.span as usize].name == "parallel_root")
        .collect();
    assert_eq!(roots.len(), 2);
    let mut values: Vec<_> = roots
        .iter()
        .map(|root| root.metrics["index"] as u32)
        .collect();
    values.sort_unstable();
    assert_eq!(values, vec![1, 2]);
    assert!(roots
        .iter()
        .all(|root| root.chains.iter().map(|chain| chain.calls).sum::<u64>() == 2));
}

#[spanscope::trace(root, name = "panic_root")]
async fn panic_root() {
    YieldOnce(false).await;
    panic!("synthetic poll panic");
}

#[test]
fn panic_restores_context_and_records_cancellation() {
    let mut future = Box::pin(panic_root());
    assert!(poll(&mut future).is_pending());
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| poll(&mut future))).is_err());
    drop(future);
    let profile = snapshot();
    let chain = named(&profile, "panic_root");
    assert_eq!(chain.cancelled, 1);
    assert_eq!(chain.path.len(), 1);
    assert!(profile
        .roots
        .iter()
        .any(|root| profile.spans[root.span as usize].name == "panic_root" && root.cancelled));
}
