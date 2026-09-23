//! The process-wide allocator of the bibli binaries (ADR-011).
//!
//! glibc's malloc keeps what a thread freed in that thread's arena, and
//! after a large block is freed it raises the size it serves from the OS,
//! so the next ones come from the arenas too: the image buffers that
//! previews and thumbnails decode on several workers at once stayed in the
//! process as RSS after they were freed, 600 MB after a quick browse that
//! needed 240 MB at its peak. mimalloc, tuned, returns freed memory
//! at once and streams listings faster; ADR-011 has the measurements.
//!
//! A binary depends on this crate with the `mimalloc` feature (on by
//! default in `bibli` and `bibli-bench`, so the benchmarks measure what
//! ships) and calls [`tune`] first thing in `main`. Build a binary with
//! `--no-default-features` to profile the heap with heaptrack, which only
//! sees the system allocator.

#[cfg(feature = "mimalloc")]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

/// mimalloc's option numbers, from `mi_option_e` in the v3 header that
/// `libmimalloc-sys` compiles by default. The Rust bindings name neither
/// option, and the enum differs between v2 and v3; the crate is pinned
/// and a test checks that each number reads its option's default.
#[cfg(feature = "mimalloc")]
mod option {
    /// Whether an arena's memory is committed as soon as it is reserved
    /// (default 2: yes where the OS overcommits, as Linux does).
    pub const ARENA_EAGER_COMMIT: libmimalloc_sys::mi_option_t = 4;
    /// How long freed memory waits before it is given back (default 1000 ms).
    pub const PURGE_DELAY: libmimalloc_sys::mi_option_t = 15;
}

/// The settings, applied before `main` by [`EARLY`] where the platform
/// runs constructors, and again by [`tune`] (setting an option twice is
/// harmless). Freed memory goes back to the OS at once instead of a second
/// later, and arenas commit memory as it is used instead of when reserved.
#[cfg(feature = "mimalloc")]
fn settle() {
    // SAFETY: `mi_option_set` stores a number in mimalloc's option table;
    // it takes no pointers, allocates nothing, and may be called from any
    // thread at any time, before mimalloc has initialised too.
    #[allow(unsafe_code)]
    unsafe {
        libmimalloc_sys::mi_option_set(option::PURGE_DELAY, 0);
        libmimalloc_sys::mi_option_set(option::ARENA_EAGER_COMMIT, 0);
    }
}

/// Runs before `main`. The runtime allocates before `main` does, and the
/// first allocation reserves mimalloc's first arena with whatever eager
/// commit says then: set from `main`, the setting came too late for that
/// arena, which then kept 30 MB committed from startup on. An ELF
/// constructor in a numbered `.init_array` section runs before the
/// unnumbered ones, the runtime's among them. Not in this crate's own
/// tests, which read the untouched defaults.
#[cfg(all(feature = "mimalloc", target_os = "linux", not(test)))]
// SAFETY: the loader calls the function once, on the main thread, before
// any Rust code; `settle` is sound at that point (see there).
#[allow(unsafe_code)]
#[used]
#[unsafe(link_section = ".init_array.00099")]
static EARLY: extern "C" fn() = {
    extern "C" fn early() {
        settle();
    }
    early
};

/// Call first thing in `main`. It links [`EARLY`] into the binary (an
/// unreferenced static in a library can be left out) and applies the
/// settings where no constructor ran.
pub fn tune() {
    #[cfg(feature = "mimalloc")]
    {
        #[cfg(all(target_os = "linux", not(test)))]
        std::hint::black_box(&EARLY);
        settle();
    }
}

#[cfg(all(test, feature = "mimalloc"))]
mod tests {
    use super::option;

    #[test]
    fn the_option_numbers_name_the_options_they_mean() {
        // A different mimalloc (v2, or a v3 that inserted an option) would
        // shift the enum: the numbers would then read other defaults. Each
        // test runs in a process of its own (nextest), before any `tune`.
        if std::env::vars().any(|(key, _)| key.starts_with("MIMALLOC_")) {
            return;
        }
        #[allow(unsafe_code)]
        // SAFETY: reads mimalloc's option table; no pointers.
        let (purge, eager) = unsafe {
            (
                libmimalloc_sys::mi_option_get(option::PURGE_DELAY),
                libmimalloc_sys::mi_option_get(option::ARENA_EAGER_COMMIT),
            )
        };
        assert_eq!(purge, 1000, "purge_delay's default");
        assert_eq!(eager, 2, "arena_eager_commit's default");
    }
}
