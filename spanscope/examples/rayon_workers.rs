//! Explicit root propagation into Rayon workers.

#[spanscope::trace(name = "work")]
fn work(input: u64) -> u64 {
    input * input
}

#[spanscope::trace(root, name = "parallel_sum")]
fn parallel_sum() -> u64 {
    let parent = spanscope::context::propagate();
    (0..100_u64)
        .into_par_iter()
        .map(|value| {
            let _attached = parent.attach();
            work(value)
        })
        .sum()
}

use rayon::prelude::*;

fn main() {
    assert_eq!(parallel_sum(), 328_350);
    let snapshot = spanscope::collection::snapshot();
    assert_eq!(
        snapshot.roots[0]
            .chains
            .iter()
            .map(|chain| chain.calls)
            .sum::<u64>(),
        101
    );
}
