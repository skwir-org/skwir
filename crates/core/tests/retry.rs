//! A copy a failure cut short goes on from where it stopped when retried:
//! what it had written stays in its staging folder, locked, and the retry
//! reads back its last part before it goes on. The failure here is the
//! file size limit the test sets on its own process (nextest runs each
//! test in one of its own): a write past it fails, as one to a server
//! gone does.
#![cfg(target_os = "linux")]

mod common;

use std::os::unix::fs::FileExt;
use std::path::{Path, PathBuf};

use common::*;
use skwir_core::cancel::CancelToken;
use skwir_core::op::{Copy, EntryRef, Executor, Operation, Plan};
use skwir_core::scheduler::Scheduler;

const MIB: u64 = 1 << 20;

/// Writes past `bytes` fail ("File too large") instead of ending the
/// process; `None` lifts the limit.
fn file_size_limit(bytes: Option<u64>) {
    static IGNORED: std::sync::Once = std::sync::Once::new();
    IGNORED.call_once(|| {
        // SAFETY: SIG_IGN installs no handler, so nothing runs in signal
        // context: a write past the limit then fails with EFBIG rather
        // than the signal ending the test's process.
        #[allow(unsafe_code)]
        unsafe {
            libc::signal(libc::SIGXFSZ, libc::SIG_IGN);
        }
    });
    rustix::process::setrlimit(
        rustix::process::Resource::Fsize,
        rustix::process::Rlimit {
            current: bytes,
            maximum: None,
        },
    )
    .expect("file size limit");
}

/// 24 MiB that differ along their length, on tmpfs: a source the kernel
/// cannot clone onto the destination's volume, so it is copied byte by
/// byte.
fn source() -> (tempfile::TempDir, PathBuf, Vec<u8>) {
    let dir = tempfile::tempdir_in("/dev/shm").expect("tmpfs");
    let path = dir.path().join("it's big.bin");
    let mut x = 0x9e37_79b9_7f4a_7c15_u64;
    let bytes: Vec<u8> = (0..24 * MIB)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x as u8
        })
        .collect();
    std::fs::write(&path, &bytes).expect("source");
    (dir, path, bytes)
}

fn copy_plan(src: &Path, dest: &Path) -> Plan {
    Plan::with_steps(
        "copy",
        vec![Box::new(Copy {
            entry: EntryRef::from_path(src).expect("source"),
            dest_dir: dest.to_path_buf(),
            new_name: None,
            preserve_attributes: true,
        }) as Box<dyn Operation>],
    )
}

/// The file a staging folder in `dest` holds, the copy cut short.
fn partial_in(dest: &Path) -> PathBuf {
    let staging = std::fs::read_dir(dest)
        .expect("destination")
        .flatten()
        .find(|e| e.file_name().to_string_lossy().starts_with(".skwir-copy-"))
        .expect("a staging folder kept")
        .path();
    std::fs::read_dir(&staging)
        .expect("staging")
        .flatten()
        .find(|e| e.file_name() != "lock")
        .expect("the copy cut short")
        .path()
}

/// 24 MiB as `source` has them, with a hole of 4 MiB from 10 MiB: a file
/// copied stretch by stretch, its holes kept.
fn source_with_a_hole() -> (tempfile::TempDir, PathBuf, Vec<u8>) {
    let (dir, path, mut bytes) = source();
    let hole = (10 * MIB) as usize..(14 * MIB) as usize;
    bytes[hole.clone()].fill(0);
    let file = std::fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .expect("source");
    rustix::fs::fallocate(
        &file,
        rustix::fs::FallocateFlags::PUNCH_HOLE | rustix::fs::FallocateFlags::KEEP_SIZE,
        hole.start as u64,
        hole.len() as u64,
    )
    .expect("a hole");
    (dir, path, bytes)
}

/// Whether the file at `path` has holes.
fn has_holes(path: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    let meta = std::fs::metadata(path).expect("there");
    meta.blocks() * 512 < meta.len()
}

/// A copy cut short at 12 MiB, and what it left.
fn cut_short(executor: &Executor, src: &Path, dest: &Path) -> (Plan, PathBuf) {
    let plan = copy_plan(src, dest);
    file_size_limit(Some(12 * MIB));
    let r = executor.execute_blocking(&plan, &CancelToken::new());
    file_size_limit(None);
    assert_eq!(r.applied, 0);
    assert!(r.error.is_some(), "it failed partway");
    assert!(executor.can_retry(plan.id));
    let partial = partial_in(dest);
    let len = std::fs::metadata(&partial).expect("kept").len();
    assert!((8 * MIB..=12 * MIB).contains(&len), "{len} bytes kept");
    (plan, partial)
}

/// Retried, it goes on from what it had written: a byte changed at its
/// start since is still there (no one reads it back but its last part).
/// The staging folder stays locked meanwhile: no sweep takes it.
#[test]
fn a_large_copy_cut_short_goes_on_from_where_it_stopped() {
    let (_src_dir, src, bytes) = source();
    let tmp = home_tempdir();
    let dest = tmp.path().join("dest");
    std::fs::create_dir(&dest).expect("dest");
    let executor = Executor::new(Scheduler::new(2));
    let (plan, partial) = cut_short(&executor, &src, &dest);

    skwir_core::op::staging::sweep(&dest);
    assert!(partial.exists(), "held for Retry");
    std::fs::OpenOptions::new()
        .write(true)
        .open(&partial)
        .expect("partial")
        .write_all_at(b"X", 0)
        .expect("marked");

    let r = executor
        .retry_blocking(plan.id, &CancelToken::new())
        .expect("retried");
    assert_eq!((r.applied, r.error), (1, None));
    let copied = std::fs::read(dest.join("it's big.bin")).expect("copied");
    assert_eq!(copied.len(), bytes.len());
    assert_eq!(copied[0], b'X', "gone on from what it had");
    assert!(copied[1..] == bytes[1..], "the rest as the source");
    assert_eq!(
        std::fs::read_dir(&dest).expect("dest").count(),
        1,
        "nothing staged left"
    );
    // One action: Undo takes the copy back.
    executor.undo_blocking(&CancelToken::new()).expect("undone");
    assert!(!dest.join("it's big.bin").exists());
}

/// What a copy cut short holds is gone on from only while one of its
/// last four MiB reads as the source's, and the source is as it was: else
/// it starts again.
#[test]
fn a_copy_cut_short_that_does_not_match_starts_again() {
    let (_src_dir, src, bytes) = source();
    let tmp = home_tempdir();
    let executor = Executor::new(Scheduler::new(2));

    // None of its last four MiB is the source's.
    let dest = tmp.path().join("tail");
    std::fs::create_dir(&dest).expect("dest");
    let (plan, partial) = cut_short(&executor, &src, &dest);
    let file = std::fs::OpenOptions::new()
        .write(true)
        .open(&partial)
        .expect("partial");
    let len = file.metadata().expect("len").len();
    file.write_all_at(b"X", 0).expect("marked");
    for back in 0..4 {
        file.write_all_at(b"?", len - 1 - back * MIB)
            .expect("changed");
    }
    drop(file);
    let r = executor
        .retry_blocking(plan.id, &CancelToken::new())
        .expect("retried");
    assert_eq!((r.applied, r.error), (1, None));
    assert_eq!(
        std::fs::read(dest.join("it's big.bin")).expect("copied"),
        bytes
    );

    // The source changed since.
    let dest = tmp.path().join("touched");
    std::fs::create_dir(&dest).expect("dest");
    let (plan, partial) = cut_short(&executor, &src, &dest);
    std::fs::OpenOptions::new()
        .write(true)
        .open(&partial)
        .expect("partial")
        .write_all_at(b"X", 0)
        .expect("marked");
    let later = std::time::SystemTime::now() + std::time::Duration::from_secs(60);
    std::fs::File::options()
        .write(true)
        .open(&src)
        .expect("source")
        .set_modified(later)
        .expect("touched");
    let r = executor
        .retry_blocking(plan.id, &CancelToken::new())
        .expect("retried");
    assert_eq!((r.applied, r.error), (1, None));
    assert_eq!(
        std::fs::read(dest.join("it's big.bin")).expect("copied"),
        bytes
    );
}

/// A file with holes goes on as any: written in order, what it holds is
/// the source's start, and its holes stay holes.
#[test]
fn a_copy_with_holes_cut_short_goes_on_from_where_it_stopped() {
    let (_src_dir, src, bytes) = source_with_a_hole();
    assert!(has_holes(&src));
    let tmp = home_tempdir();
    let dest = tmp.path().join("dest");
    std::fs::create_dir(&dest).expect("dest");
    let executor = Executor::new(Scheduler::new(2));
    let (plan, partial) = cut_short(&executor, &src, &dest);
    std::fs::OpenOptions::new()
        .write(true)
        .open(&partial)
        .expect("partial")
        .write_all_at(b"X", 0)
        .expect("marked");

    let r = executor
        .retry_blocking(plan.id, &CancelToken::new())
        .expect("retried");
    assert_eq!((r.applied, r.error), (1, None));
    let copy = dest.join("it's big.bin");
    let copied = std::fs::read(&copy).expect("copied");
    assert_eq!(copied[0], b'X', "gone on from what it had");
    assert!(copied[1..] == bytes[1..], "the rest as the source");
    // Where the destination's volume keeps holes.
    let probe = tmp.path().join("probe");
    let file = std::fs::File::create(&probe).expect("probe");
    file.set_len(MIB).expect("a hole");
    if has_holes(&probe) {
        assert!(has_holes(&copy), "its hole kept");
    }
}

/// A folder copied whole, cut short in its last file: Retry keeps what
/// that run finished (a byte changed since, its time given back, stays:
/// nothing reads it again), goes on from the file it was writing, and
/// makes the rest.
#[test]
fn a_folder_cut_short_goes_on_from_where_it_stopped() {
    use std::os::unix::fs::MetadataExt;
    let (_src_dir, big, bytes) = source();
    // tmpfs lists the newest first: the large file, made first, is
    // copied last.
    let tree = big.parent().unwrap().join("it's a folder");
    std::fs::create_dir(&tree).expect("tree");
    std::fs::rename(&big, tree.join("z big.bin")).expect("big");
    let first: Vec<u8> = bytes[..(5 * MIB) as usize].to_vec();
    std::fs::write(tree.join("a.bin"), &first).expect("a");
    std::fs::create_dir(tree.join("sub")).expect("sub");
    std::fs::write(tree.join("sub/b.bin"), &first[1..]).expect("b");
    std::fs::write(tree.join("sub/c.txt"), "c").expect("c");
    std::os::unix::fs::symlink("a.bin", tree.join("to a")).expect("link");
    let tmp = home_tempdir();
    let dest = tmp.path().join("dest");
    std::fs::create_dir(&dest).expect("dest");
    let executor = Executor::new(Scheduler::new(2));

    let plan = copy_plan(&tree, &dest);
    file_size_limit(Some(12 * MIB));
    let r = executor.execute_blocking(&plan, &CancelToken::new());
    file_size_limit(None);
    assert_eq!(r.applied, 0);
    assert!(r.error.is_some(), "it failed partway");
    assert!(executor.can_retry(plan.id));
    let staged = partial_in(&dest);
    assert!(staged.is_dir(), "the tree kept");
    let finished = staged.join("a.bin");
    let source_a = std::fs::metadata(tree.join("a.bin")).expect("a");
    assert_eq!(
        std::fs::metadata(&finished).expect("finished").len(),
        source_a.len(),
        "finished before the cut"
    );
    let cut = staged.join("z big.bin");
    let kept = std::fs::metadata(&cut).expect("cut short").len();
    assert!((8 * MIB..=12 * MIB).contains(&kept), "{kept} bytes kept");
    for path in [&finished, &cut] {
        let file = std::fs::OpenOptions::new()
            .write(true)
            .open(path)
            .expect("there");
        file.write_all_at(b"X", 0).expect("marked");
        if path == &finished {
            file.set_modified(source_a.modified().unwrap())
                .expect("its time given back");
        }
    }

    let r = executor
        .retry_blocking(plan.id, &CancelToken::new())
        .expect("retried");
    assert_eq!((r.applied, r.error), (1, None));
    let copy = dest.join("it's a folder");
    let a = std::fs::read(copy.join("a.bin")).expect("a");
    assert!(a[0] == b'X' && a[1..] == first[1..], "finished, kept");
    let z = std::fs::read(copy.join("z big.bin")).expect("z");
    assert!(z[0] == b'X' && z[1..] == bytes[1..], "gone on from");
    assert_eq!(
        std::fs::read(copy.join("sub/b.bin")).expect("b"),
        first[1..]
    );
    assert_eq!(std::fs::read(copy.join("sub/c.txt")).expect("c"), b"c");
    assert_eq!(
        std::fs::read_link(copy.join("to a")).expect("link"),
        Path::new("a.bin")
    );
    assert_eq!(
        std::fs::metadata(copy.join("sub/b.bin")).unwrap().mtime(),
        std::fs::metadata(tree.join("sub/b.bin")).unwrap().mtime()
    );
    assert_eq!(
        std::fs::read_dir(&dest).expect("dest").count(),
        1,
        "nothing staged left"
    );
    executor.undo_blocking(&CancelToken::new()).expect("undone");
    assert!(!copy.exists());
}

/// A copy cut short whose last bytes are not the source's (a mount cut
/// short may leave what it was asked for last zeroed) goes on from the
/// end of the last MiB before them that is.
#[test]
fn a_copy_cut_short_with_a_spoiled_end_goes_on_from_before_it() {
    let (_src_dir, src, bytes) = source();
    let tmp = home_tempdir();
    let dest = tmp.path().join("dest");
    std::fs::create_dir(&dest).expect("dest");
    let executor = Executor::new(Scheduler::new(2));
    let (plan, partial) = cut_short(&executor, &src, &dest);
    let file = std::fs::OpenOptions::new()
        .write(true)
        .open(&partial)
        .expect("partial");
    let len = file.metadata().expect("len").len();
    file.write_all_at(b"X", 0).expect("marked");
    file.write_all_at(&[0; 100], len - 100).expect("zeroed");
    drop(file);

    let r = executor
        .retry_blocking(plan.id, &CancelToken::new())
        .expect("retried");
    assert_eq!((r.applied, r.error), (1, None));
    let copied = std::fs::read(dest.join("it's big.bin")).expect("copied");
    assert_eq!(copied[0], b'X', "gone on from what it had");
    assert!(copied[1..] == bytes[1..], "the rest as the source");
}
