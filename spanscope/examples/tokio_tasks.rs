//! Poll-scoped ancestry in a migrating Tokio task and an explicitly spawned child.

#[spanscope::trace(name = "database")]
async fn database() -> u64 {
    tokio::task::yield_now().await;
    42
}

#[spanscope::trace(root, name = "request")]
async fn request() -> u64 {
    let parent = spanscope::context::propagate();
    tokio::spawn(parent.wrap(database()))
        .await
        .expect("worker completed")
}

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() {
    assert_eq!(request().await, 42);
    let snapshot = spanscope::collection::snapshot();
    assert_eq!(snapshot.roots.len(), 1);
    assert!(snapshot.roots[0]
        .chains
        .iter()
        .any(|chain| chain.path.len() == 2));
}
