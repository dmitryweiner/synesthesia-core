//! A counting global allocator for the tests in `tests/`: `Engine::render`
//! and `Player::render_into` must allocate (and free) nothing — AGENTS.md,
//! PLAN-SAMPLES-SCRIPTS.md decision 8. Counting is per thread, so the test
//! harness's other threads do not disturb it.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

pub struct Counting;

thread_local! {
    static ON: Cell<bool> = const { Cell::new(false) };
    static COUNT: Cell<usize> = const { Cell::new(0) };
}

fn note() {
    // `try_with`: the thread-locals may already be gone while a thread exits.
    let _ = ON.try_with(|on| {
        if on.get() {
            let _ = COUNT.try_with(|c| c.set(c.get() + 1));
        }
    });
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        note();
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        note();
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, new: usize) -> *mut u8 {
        note();
        unsafe { System.realloc(p, l, new) }
    }
}

/// How many allocations, frees and reallocations `f` made on this thread.
pub fn count<T>(f: impl FnOnce() -> T) -> (T, usize) {
    COUNT.with(|c| c.set(0));
    ON.with(|on| on.set(true));
    let out = f();
    ON.with(|on| on.set(false));
    (out, COUNT.with(Cell::get))
}
