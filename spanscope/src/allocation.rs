//! Opt-in heap accounting: allocations, deallocations, net balance and peak.
//! This is the only unsafe runtime module.
#![allow(unsafe_code)]

use std::alloc::{GlobalAlloc, Layout};
use std::cell::Cell;
use std::sync::atomic::{AtomicBool, Ordering};

static OBSERVED: AtomicBool = AtomicBool::new(false);

/// Whether an installed wrapper has attributed at least one heap operation.
pub(crate) fn observed() -> bool {
    OBSERVED.load(Ordering::Relaxed)
}

fn mark_observed() {
    if !OBSERVED.load(Ordering::Relaxed) {
        OBSERVED.store(true, Ordering::Relaxed);
    }
}

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

fn enabled() -> bool {
    ACTIVE.try_with(|active| active.get()).unwrap_or(false)
        && !PAUSED.try_with(|paused| paused.get()).unwrap_or(true)
}

fn charge(bytes: usize) {
    if !enabled() {
        return;
    }
    mark_observed();
    let _pause = Pause::new();
    crate::runtime::charge_allocation(bytes as u64);
}

fn credit(bytes: usize) {
    if !enabled() {
        return;
    }
    let _pause = Pause::new();
    crate::runtime::charge_deallocation(bytes as u64);
}

fn charge_realloc(old: usize, new: usize) {
    if !enabled() {
        return;
    }
    mark_observed();
    let _pause = Pause::new();
    // Allocate first: a moving reallocation holds both blocks at its peak.
    crate::runtime::charge_allocation(new as u64);
    crate::runtime::charge_deallocation(old as u64);
}

/// Application-installed allocator wrapper that measures the heap balance of spans.
///
/// Install it with `#[global_allocator]` around an allocator of your choice.
/// Allocations add their requested size to the innermost active span and
/// deallocations add theirs to the freed bytes of the span that frees them; the
/// difference is the span's net heap growth and its running maximum the peak.
/// A successful reallocation counts as freeing the old size and allocating the
/// new one, so gross allocation bytes keep their previous meaning. The hooks
/// never retain pointers or allocate.
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
        let pause = Pause::new();
        // SAFETY: The caller provides the pointer/layout pair accepted by A.
        unsafe { self.inner.dealloc(pointer, layout) };
        drop(pause);
        credit(layout.size());
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let pause = Pause::new();
        // SAFETY: The caller provides A's realloc pointer/layout preconditions.
        let result = unsafe { self.inner.realloc(pointer, layout, new_size) };
        drop(pause);
        if !result.is_null() {
            charge_realloc(layout.size(), new_size);
        }
        result
    }
}
