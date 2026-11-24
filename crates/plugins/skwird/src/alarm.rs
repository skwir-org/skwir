//! Skwir's own timer on a connect to skwird (`crate::connect`): an engine
//! that takes the connection and says no hello within a few seconds is
//! stopped through the connect's `Stopper`, so a stalled engine (stopped,
//! not gone: the system still takes its connections) costs a search a few
//! seconds, not the client's whole silence. One thread rings every alarm
//! armed, made when one is and ended once none is: nothing runs between
//! connects.

use std::collections::BTreeMap;
use std::sync::LazyLock;
use std::time::{Duration, Instant};

use parking_lot::{Condvar, Mutex, MutexGuard};
use skwird_client::Stopper;

/// Every alarm armed, rung in the order of their times.
#[derive(Default)]
struct Alarms {
    state: Mutex<State>,
    /// Woken when an alarm is armed sooner than the first.
    sooner: Condvar,
}

#[derive(Default)]
struct State {
    armed: BTreeMap<(Instant, u64), Stopper>,
    next: u64,
    /// Whether the thread that rings them runs.
    ringing: bool,
}

static ALARMS: LazyLock<Alarms> = LazyLock::new(Alarms::default);

/// `ask`'s answer, unless it takes longer than `time`: then `stopper`
/// is stopped, and whatever `ask` answers once stopped is let go of (a
/// connection made as the alarm rang is closed with it). `ask` is to
/// stop when `stopper` does, as a connect given it does.
pub(crate) fn within<T>(time: Duration, stopper: &Stopper, ask: impl FnOnce() -> T) -> Option<T> {
    let key = arm(Instant::now() + time, stopper);
    let answer = ask();
    // Taken back under the lock the thread rings it under: either it is
    // still armed here, or it rang.
    let mut state = ALARMS.state.lock();
    let rang = state.armed.remove(&key).is_none();
    // The last one taken back: the thread ends now, not at its time.
    if state.armed.is_empty() {
        ALARMS.sooner.notify_one();
    }
    drop(state);
    (!rang).then_some(answer)
}

fn arm(at: Instant, stopper: &Stopper) -> (Instant, u64) {
    let mut state = ALARMS.state.lock();
    let key = (at, state.next);
    state.next += 1;
    let sooner = state
        .armed
        .first_key_value()
        .is_none_or(|(first, _)| key < *first);
    state.armed.insert(key, stopper.clone());
    if !state.ringing {
        let started = std::thread::Builder::new()
            .name("skwird hello".into())
            .spawn(ring);
        // None started: nothing rings, and the client's own silence ends
        // what waits.
        state.ringing = started.is_ok();
    } else if sooner {
        ALARMS.sooner.notify_one();
    }
    key
}

/// Ring each alarm at its time, until none is armed.
fn ring() {
    let mut state = ALARMS.state.lock();
    loop {
        let Some((&key, _)) = state.armed.first_key_value() else {
            state.ringing = false;
            return;
        };
        if key.0 > Instant::now() {
            ALARMS.sooner.wait_until(&mut state, key.0);
            continue;
        }
        if let Some(stopper) = state.armed.remove(&key) {
            // A stop shuts a socket and wakes a wait: it blocks on nothing,
            // but it is no reason to hold every arming meanwhile.
            MutexGuard::unlocked(&mut state, || stopper.stop());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An answer in time is taken; one that takes longer stops what asked
    /// it, at its time, and is let go of; the thread ends once none waits.
    #[test]
    fn an_answer_late_is_stopped_at_its_time() {
        let stopper = Stopper::new();
        assert_eq!(within(Duration::from_secs(30), &stopper, || 1), Some(1));
        // Taken back, the thread ends then, not at its time.
        ends_within(Duration::from_secs(5));

        let late = Stopper::new();
        let started = Instant::now();
        let answer = within(Duration::from_millis(100), &late, || {
            // A stopper's stop is seen by what it was given to: a probe
            // client, connected to nothing, says so once stopped.
            while started.elapsed() < Duration::from_secs(10) {
                if stopped(&late) {
                    return 2;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            3
        });
        assert_eq!(answer, None, "stopped, then let go of");
        let took = started.elapsed();
        assert!(
            took >= Duration::from_millis(100) && took < Duration::from_secs(5),
            "at its time: {took:?}"
        );
        // Every alarm taken back or rung, the thread ends.
        ends_within(Duration::from_secs(10));
    }

    /// The thread that rings ends within `time`.
    fn ends_within(time: Duration) {
        let deadline = Instant::now() + time;
        while ALARMS.state.lock().ringing {
            assert!(Instant::now() < deadline, "the thread ended");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// Whether `stopper` was stopped: a connect given it says so at once.
    fn stopped(stopper: &Stopper) -> bool {
        let mut find = skwird_client::Find::new("skwir-test");
        find.start = None;
        find.socket = Some(std::path::PathBuf::from("/nonexistent/skwird-alarm"));
        matches!(
            skwird_client::Client::connect_stopped(&find, stopper),
            Err(skwird_client::Error::Stopped)
        )
    }
}
