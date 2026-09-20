//! Opt-in, gross allocation accounting. This is the only unsafe runtime module.
#![allow(unsafe_code)]

use std::alloc::{GlobalAlloc, Layout};
use std::cell::Cell;

thread_local! {
    static PAUSED: Cell<bool> = const { Cell::new(false) };
    static ACTIVE: Cell<bool> = const { Cell::new(false) };
}

/// Stops attributing allocations made by the profiler itself.
pub(crate) struct Pause {
    previous: bool,
}

impl Pause {
    pub(crate) fn new() -> Self {
        let previous = PAUSED.try_with(|value| value.replace(true)).unwrap_or(true);
        Self { previous }
    }
}

impl Drop for Pause {
    fn drop(&mut self) {
        let _ = PAUSED.try_with(|value| value.set(self.previous));
    }
}

pub(crate) fn set_active(active: bool) {
    let _ = ACTIVE.try_with(|value| value.set(active));
}

fn charge(bytes: usize) {
    let enabled = ACTIVE.try_with(|active| active.get()).unwrap_or(false)
        && !PAUSED.try_with(|paused| paused.get()).unwrap_or(true);
    if !enabled {
        return;
    }
    let _pause = Pause::new();
    crate::runtime::charge_allocation(bytes as u64);
}

/// Application-installed allocator wrapper that counts successful gross allocations.
///
/// Install it with `#[global_allocator]` around an allocator of your choice.
/// Deallocation does not reduce gross bytes; successful reallocation charges the
/// requested new size. It does not retain pointers or allocate in its hooks.
pub struct TrackingAllocator<A: GlobalAlloc> {
    inner: A,
}

impl<A: GlobalAlloc> TrackingAllocator<A> {
    /// Creates an inert wrapper until instrumented code runs on a thread.
    pub const fn new(inner: A) -> Self {
        Self { inner }
    }
}

// SAFETY: Every operation delegates the exact pointer/layout contract to A.
// Bookkeeping only uses non-allocating thread-local cells and never retains a
// pointer, dereferences user memory, or changes success/failure behavior.
unsafe impl<A: GlobalAlloc> GlobalAlloc for TrackingAllocator<A> {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pause = Pause::new();
        // SAFETY: The caller provides GlobalAlloc's layout preconditions.
        let pointer = unsafe { self.inner.alloc(layout) };
        drop(pause);
        if !pointer.is_null() {
            charge(layout.size());
        }
        pointer
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let pause = Pause::new();
        // SAFETY: The caller provides GlobalAlloc's layout preconditions.
        let pointer = unsafe { self.inner.alloc_zeroed(layout) };
        drop(pause);
        if !pointer.is_null() {
            charge(layout.size());
        }
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        let _pause = Pause::new();
        // SAFETY: The caller provides the pointer/layout pair accepted by A.
        unsafe { self.inner.dealloc(pointer, layout) }
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let pause = Pause::new();
        // SAFETY: The caller provides A's realloc pointer/layout preconditions.
        let result = unsafe { self.inner.realloc(pointer, layout, new_size) };
        drop(pause);
        if !result.is_null() {
            charge(new_size);
        }
        result
    }
}
