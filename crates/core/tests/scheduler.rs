//! A panicking job must not take a worker thread with it.
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use bibli_core::cancel::CancelToken;
use bibli_core::scheduler::{Priority, Scheduler};

#[test]
fn a_panicking_job_leaves_the_pool_able_to_run_later_jobs() {
    let scheduler = Scheduler::new(1);
    for _ in 0..3 {
        scheduler.spawn(Priority::Visible, "test.panic", CancelToken::new(), || {
            panic!("malformed input in a third-party parser");
        });
    }
    let done = Arc::new(AtomicUsize::new(0));
    for _ in 0..4 {
        let done = Arc::clone(&done);
        scheduler.spawn(Priority::Normal, "test.ok", CancelToken::new(), move || {
            done.fetch_add(1, Ordering::SeqCst);
        });
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    while done.load(Ordering::SeqCst) < 4 {
        assert!(Instant::now() < deadline, "later jobs never ran");
        std::thread::sleep(Duration::from_millis(5));
    }
    let stats = scheduler.stats();
    let panicked = stats
        .iter()
        .find(|(l, _)| *l == "test.panic")
        .unwrap()
        .1
        .clone();
    assert_eq!(panicked.panics, 3);
    assert_eq!(panicked.runs, 3);
}
