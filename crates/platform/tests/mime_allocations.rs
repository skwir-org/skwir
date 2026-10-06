//! Typing a file by its name allocates for what matches, never once per
//! pattern of the database: a listing types every name it shows, and the
//! desktop's database holds well over a thousand patterns. Counted with a
//! global allocator of this test's own (its own process under nextest).
//! Linux's alone: the type database is read from the free desktops' folders
//! in tests (ADR-035).
#![cfg(target_os = "linux")]

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

struct Counting;

static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);

// SAFETY: forwards every call to the system allocator unchanged, counting
// the allocations; the counter is the only state it adds.
#[allow(unsafe_code)] // a global allocator is an unsafe trait
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        // SAFETY: the caller's contract is the system allocator's.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: `ptr` came from `alloc` above, which is the system's.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        // SAFETY: as for `dealloc`, with the caller's new size.
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static COUNTING: Counting = Counting;

/// The allocations one lookup of `name` makes, and its answer.
fn counted(name: &str) -> (usize, Option<String>) {
    let before = ALLOCATIONS.load(Ordering::Relaxed);
    let found = bibli_platform::mime::by_name(std::ffi::OsStr::new(name));
    let count = ALLOCATIONS.load(Ordering::Relaxed) - before;
    (count, found.map(|m| m.as_str().to_owned()))
}

#[test]
fn a_name_is_typed_without_an_allocation_per_pattern() {
    // The first lookup reads the database; what is measured is the next.
    let _ = counted("warm.txt");
    for (name, expected) in [
        ("notes.txt", "text/plain"),
        ("photo.JPG", "image/jpeg"),
        ("archive.tar.gz", "application/x-compressed-tar"),
        ("Makefile", "text/x-makefile"),
    ] {
        let (count, found) = counted(name);
        println!("{name}: {count} allocations, {found:?}");
        assert_eq!(found.as_deref(), Some(expected), "{name}");
        assert!(count < 64, "{name}: {count} allocations");
    }
}
