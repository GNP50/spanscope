//! Recursive chains are distinct even when each frame uses the same descriptor.

#[spanscope::trace(name = "factorial")]
fn factorial(n: u64) -> u64 {
    if n == 0 {
        1
    } else {
        n * factorial(n - 1)
    }
}

#[spanscope::trace(root, name = "factorial_root")]
fn factorial_root(n: u64) -> u64 {
    factorial(n)
}

fn main() {
    assert_eq!(factorial_root(5), 120);
    let snapshot = spanscope::collection::snapshot();
    assert_eq!(snapshot.chains.len(), 7);
    assert_eq!(snapshot.roots[0].chains.len(), 7);
}
