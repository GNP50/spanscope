//! Isolated async sampling decisions and never-polled futures.
#![cfg(feature = "enabled")]

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};

struct Noop;
impl Wake for Noop {
    fn wake(self: Arc<Self>) {}
}

#[spanscope::trace(name = "sampled_async_child")]
async fn child() -> u32 {
    7
}

#[spanscope::trace(root, name = "sampled_async_root")]
async fn root() -> u32 {
    child().await
}

#[test]
fn never_polled_is_invisible_and_zero_rate_suppresses_whole_async_subtree() {
    spanscope::collection::set_sample_rate(0.0).unwrap();
    drop(root());
    let before = spanscope::collection::snapshot();
    assert_eq!(before.sampled_out_roots, 0);
    assert!(before.chains.is_empty());
    let mut future = Box::pin(root());
    let waker = Waker::from(Arc::new(Noop));
    assert!(matches!(
        Pin::new(&mut future).poll(&mut Context::from_waker(&waker)),
        Poll::Ready(7)
    ));
    let after = spanscope::collection::snapshot();
    assert_eq!(after.sampled_out_roots, 1);
    assert!(after.chains.is_empty());
    assert!(after.roots.is_empty());
}
