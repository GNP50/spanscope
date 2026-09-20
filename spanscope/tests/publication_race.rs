//! Simultaneous flushes and worker exits retain observations exactly once.
#![cfg(feature = "enabled")]

use std::sync::{Arc, Barrier};

#[spanscope::trace(name = "racing_worker")]
fn racing_worker() {
    std::hint::black_box(1);
}

#[spanscope::trace(root, name = "racing_root")]
fn racing_root() {
    racing_worker();
}

#[test]
fn concurrent_snapshots_and_thread_exits_do_not_lose_or_duplicate_batches() {
    const WORKERS: usize = 6;
    const FLUSHERS: usize = 2;
    let start = Arc::new(Barrier::new(WORKERS + FLUSHERS + 1));
    let workers: Vec<_> = (0..WORKERS)
        .map(|_| {
            let start = Arc::clone(&start);
            std::thread::spawn(move || {
                start.wait();
                for _ in 0..1_200 {
                    racing_worker();
                }
                racing_root();
            })
        })
        .collect();
    let flushers: Vec<_> = (0..FLUSHERS)
        .map(|_| {
            let start = Arc::clone(&start);
            std::thread::spawn(move || {
                start.wait();
                for _ in 0..100 {
                    let profile = spanscope::collection::snapshot();
                    if profile
                        .roots
                        .iter()
                        .any(|root| profile.spans[root.span as usize].name == "racing_root")
                    {
                        assert!(profile.chains.iter().any(|chain| {
                            profile.spans[*chain.path.last().unwrap() as usize].name
                                == "racing_root"
                        }));
                    }
                }
            })
        })
        .collect();
    start.wait();
    for worker in workers {
        worker.join().unwrap();
    }
    for flusher in flushers {
        flusher.join().unwrap();
    }
    for _ in 0..3 {
        let profile = spanscope::collection::snapshot();
        let count: u64 = profile
            .chains
            .iter()
            .filter(|chain| {
                profile.spans[*chain.path.last().unwrap() as usize].name == "racing_worker"
            })
            .map(|chain| chain.count)
            .sum();
        assert_eq!(count, (WORKERS * 1_201) as u64);
        assert_eq!(
            profile
                .roots
                .iter()
                .filter(|root| profile.spans[root.span as usize].name == "racing_root")
                .count(),
            WORKERS
        );
        assert!(
            profile
                .threads
                .iter()
                .filter(|thread| thread.exited)
                .count()
                >= WORKERS
        );
    }
}
