//! Allocation counting for `prof.alloc` (§5): "counts allocations by system
//! so a Luau system that builds a table per entity is visible".
//!
//! `Counting` wraps the system allocator and keeps two thread-local
//! counters, allocations and bytes. The scheduler reads them before and
//! after each system and records the difference, so a system's row is
//! exactly what happened on the sim thread while it ran. Other threads (a
//! connection, the index scan) have their own counters and never show up.
//! The Luau VM allocates through the Rust global allocator (`mlua` passes
//! it to `lua_newstate`), so script systems are counted the same way.
//!
//! A binary opts in with
//!
//! ```ignore
//! #[global_allocator]
//! static ALLOC: argus_sim::alloc::Counting = argus_sim::alloc::Counting;
//! ```
//!
//! Without it every count is zero and `installed()` is false, which
//! `prof.alloc` reports as `counted: false` rather than pretending.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

thread_local! {
    static ALLOCS: Cell<u64> = const { Cell::new(0) };
    static BYTES: Cell<u64> = const { Cell::new(0) };
}

/// The counting allocator. Zero-sized; install it as the global allocator.
pub struct Counting;

#[inline]
fn count(bytes: usize) {
    // `try_with` so a thread in teardown never panics inside the allocator.
    let _ = ALLOCS.try_with(|c| c.set(c.get() + 1));
    let _ = BYTES.try_with(|c| c.set(c.get() + bytes as u64));
}

// SAFETY: every method forwards to `System` with the same layout; the
// counters are plain thread-local cells with const initialisers, so
// touching them never allocates or re-enters the allocator.
unsafe impl GlobalAlloc for Counting {
    #[inline]
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count(layout.size());
        System.alloc(layout)
    }
    #[inline]
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        count(layout.size());
        System.alloc_zeroed(layout)
    }
    #[inline]
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout)
    }
    #[inline]
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // A growing realloc is a new allocation of the extra bytes as far as
        // "how much did this system ask for" is concerned.
        count(new_size.saturating_sub(layout.size()));
        System.realloc(ptr, layout, new_size)
    }
}

/// Snapshot of this thread's counters: `(allocations, bytes)` since the
/// thread started. Take two and subtract.
#[inline]
pub fn counters() -> (u64, u64) {
    (
        ALLOCS.try_with(Cell::get).unwrap_or(0),
        BYTES.try_with(Cell::get).unwrap_or(0),
    )
}

/// True when `Counting` is the global allocator of this binary. Decided by
/// doing one small allocation and seeing whether the counter moved.
pub fn installed() -> bool {
    let before = counters().0;
    let b = Box::new(std::hint::black_box(0u64));
    std::hint::black_box(&b);
    let after = counters().0;
    drop(b);
    after > before
}
