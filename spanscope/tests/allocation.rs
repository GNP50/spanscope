//! Opt-in allocator accounting of direct allocations and successful reallocations.
#![cfg(feature = "alloc-tracker")]
#![allow(unsafe_code)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

thread_local! { static REENTER_NEXT: Cell<bool> = const { Cell::new(false) }; }

struct Reentrant;

// SAFETY: All pointer/layout operations are delegated unchanged to System.
unsafe impl GlobalAlloc for Reentrant {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if REENTER_NEXT
            .try_with(|flag| flag.replace(false))
            .unwrap_or(false)
        {
            let nested = Box::new([7u8; 32]);
            std::hint::black_box(&nested);
            drop(nested);
        }
        // SAFETY: The caller supplies GlobalAlloc's layout preconditions.
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        // SAFETY: The caller supplies GlobalAlloc's layout preconditions.
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        // SAFETY: The pointer and layout are passed through unchanged.
        unsafe { System.dealloc(pointer, layout) }
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        // SAFETY: The pointer, layout, and new size are passed through unchanged.
        unsafe { System.realloc(pointer, layout, size) }
    }
}

#[global_allocator]
static ALLOCATOR: spanscope::TrackingAllocator<Reentrant> =
    spanscope::TrackingAllocator::new(Reentrant);

#[spanscope::trace(root, name = "allocation_root")]
fn allocation_root() {
    allocation_child();
}

#[spanscope::trace(name = "allocation_child")]
fn allocation_child() {
    REENTER_NEXT.with(|flag| flag.set(true));
    let mut bytes = Vec::<u8>::with_capacity(128);
    std::hint::black_box(&mut bytes);
    bytes.reserve_exact(256);
    std::hint::black_box(&mut bytes);
    drop(bytes);
}

#[spanscope::trace(name = "cross_thread_alloc")]
fn cross_thread_alloc() -> Vec<u8> {
    Vec::with_capacity(64)
}

#[test]
fn direct_gross_bytes_exclude_profiler_work_and_deallocation() {
    allocation_root();
    let first = spanscope::collection::snapshot();
    let child = first
        .chains
        .iter()
        .find(|chain| first.spans[*chain.path.last().unwrap() as usize].name == "allocation_child")
        .unwrap();
    assert_eq!(child.allocs, 2);
    assert_eq!(child.alloc_bytes, 384);
    let root = first
        .chains
        .iter()
        .find(|chain| first.spans[*chain.path.last().unwrap() as usize].name == "allocation_root")
        .unwrap();
    assert_eq!(root.allocs, 0);
    let second = spanscope::collection::snapshot();
    let again = second
        .chains
        .iter()
        .find(|chain| second.spans[*chain.path.last().unwrap() as usize].name == "allocation_child")
        .unwrap();
    assert_eq!(again.alloc_bytes, 384);
    assert_eq!(again.allocs, 2);

    let bytes = cross_thread_alloc();
    std::thread::spawn(move || drop(bytes)).join().unwrap();
    let after_drop = spanscope::collection::snapshot();
    let cross_thread = after_drop
        .chains
        .iter()
        .find(|chain| {
            after_drop.spans[*chain.path.last().unwrap() as usize].name == "cross_thread_alloc"
        })
        .unwrap();
    assert_eq!(cross_thread.alloc_bytes, 64);
    assert_eq!(cross_thread.allocs, 1);
}
