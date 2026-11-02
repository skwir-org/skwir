//! How fast a running plan goes and how long it has left (ADR-113): what
//! its meter counted, sampled as it goes, read over the last seconds.
//! Every instant is given, so a test says when things happen.
//!
//! The speed is the bytes moved over the last [`SPEED_OVER`]; the time
//! left, what is left over the speed of the last [`LEFT_OVER`], steadier,
//! so that it counts down rather than jumps. Nothing is said before there
//! is enough to say it ([`SPEED_AFTER`], [`LEFT_AFTER`]), nor while
//! nothing moved of late: a plan walking a tree, or waiting on a server,
//! has no speed, and no time left anyone could believe.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// A sample at most this often: often enough for a window of seconds,
/// rarely enough that a fast copy's chunks add no work.
pub(super) const SAMPLE_EVERY: Duration = Duration::from_millis(250);

/// Samples kept: the longest window, and the one sample before it.
const KEPT: Duration = Duration::from_secs(30);

/// The speed shown is over this: recent enough to follow a change (a
/// server slowing down), long enough not to flicker with each chunk.
pub(super) const SPEED_OVER: Duration = Duration::from_secs(5);

/// The time left is reckoned over this: steadier than the speed shown.
pub(super) const LEFT_OVER: Duration = Duration::from_secs(20);

/// No speed is said before the work went this long (from its start, or
/// from where it went on after a pause), nor a time left before
/// [`LEFT_AFTER`].
pub(super) const SPEED_AFTER: Duration = Duration::from_secs(2);
pub(super) const LEFT_AFTER: Duration = Duration::from_secs(4);

/// What a plan had counted at an instant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Sample {
    pub(super) at: Instant,
    /// Bytes moved (the meter's `moved`: what went through now, a clone's
    /// bytes and a copy's bytes gone on from left out; while a mount
    /// sends, what it sent).
    pub(super) bytes: u64,
    /// Steps ended.
    pub(super) steps: usize,
}

/// The samples of the work under way, oldest first. Nothing is noted
/// before it began, nor while it is paused.
#[derive(Debug, Default)]
pub(super) struct Samples {
    kept: VecDeque<Sample>,
    armed: bool,
}

impl Samples {
    /// The work begins, or goes on after a pause or into another phase
    /// (a mount sending what was written): what came before says nothing
    /// of what comes now, and what is noted next is where it is measured
    /// from. Not from now: the wait for its first byte (a server's first
    /// answer, a tree sized) says nothing of how fast bytes go.
    pub(super) fn arm(&mut self) {
        self.kept.clear();
        self.armed = true;
    }

    /// As [`Self::arm`], measured from `now`.
    #[cfg(test)]
    pub(super) fn restart(&mut self, now: Sample) {
        self.arm();
        self.kept.push_back(now);
    }

    /// The work stops being measured (paused): nothing is noted nor said
    /// until it is armed again.
    pub(super) fn clear(&mut self) {
        self.kept.clear();
        self.armed = false;
    }

    /// Note `now`, at most every [`SAMPLE_EVERY`], letting go of what the
    /// longest window no longer reaches; the first since armed is where the
    /// work is measured from. Nothing while not begun or paused.
    pub(super) fn note(&mut self, now: Sample) {
        if !self.armed {
            return;
        }
        let Some(last) = self.kept.back() else {
            self.kept.push_back(now);
            return;
        };
        if now.at.saturating_duration_since(last.at) < SAMPLE_EVERY {
            return;
        }
        self.kept.push_back(now);
        while self.kept.len() > 2 && self.kept[1].at + KEPT <= now.at {
            self.kept.pop_front();
        }
    }

    /// How much of `of` went by each second over the last `over`, ending
    /// at `now`: from the latest sample at least `over` old, else from the
    /// first. `None` before any.
    fn per_second(&self, now: Sample, over: Duration, of: fn(&Sample) -> u64) -> Option<f64> {
        let first = self.kept.front()?;
        let from = self
            .kept
            .iter()
            .rev()
            .find(|s| s.at + over <= now.at)
            .unwrap_or(first);
        let span = now.at.saturating_duration_since(from.at).as_secs_f64();
        #[allow(clippy::cast_precision_loss)] // A rate: bytes far past 2^52 are rare.
        let moved = of(&now).saturating_sub(of(from)) as f64;
        (span > 0.0).then(|| moved / span)
    }

    /// How fast the work goes at `now`, and how long it has left: `bytes`
    /// left to move, else `steps` left to end (a plan that moves no byte,
    /// counted by its steps). See the module's documentation for when
    /// nothing is said.
    pub(super) fn reading(&self, now: Sample, left: Left) -> Reading {
        let Some(first) = self.kept.front() else {
            return Reading::default();
        };
        let went = now.at.saturating_duration_since(first.at);
        let bytes = |s: &Sample| s.bytes;
        let steps = |s: &Sample| s.steps as u64;
        let of: fn(&Sample) -> u64 = match left {
            Left::Steps(_) => steps,
            Left::Bytes(_) | Left::Unknown => bytes,
        };
        // Nothing moved of late: no speed, no time left.
        let lately = self.per_second(now, SPEED_OVER, of).unwrap_or(0.0);
        if went < SPEED_AFTER || lately <= 0.0 {
            return Reading::default();
        }
        let speed = matches!(left, Left::Bytes(_) | Left::Unknown).then_some(lately);
        let steady = self.per_second(now, LEFT_OVER, of).unwrap_or(0.0);
        let left = match left {
            _ if went < LEFT_AFTER || steady <= 0.0 => None,
            Left::Bytes(0) | Left::Steps(0) | Left::Unknown => None,
            #[allow(clippy::cast_precision_loss)]
            Left::Bytes(n) => Some(Duration::from_secs_f64(n as f64 / steady)),
            #[allow(clippy::cast_precision_loss)]
            Left::Steps(n) => Some(Duration::from_secs_f64(n as f64 / steady)),
        };
        Reading { speed, left }
    }
}

/// What a plan has left, as far as it can be told.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Left {
    /// Bytes, the plan's total being known.
    Bytes(u64),
    /// Steps, for a plan that moves no byte.
    Steps(usize),
    /// Not known: a speed may be said, no time left.
    Unknown,
}

/// How fast a plan goes, and how long it has left, when that can be said.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct Reading {
    /// Bytes a second, of late; `None` for a plan counted by its steps.
    pub(super) speed: Option<f64>,
    pub(super) left: Option<Duration>,
}

/// How long a piece of work ran, its pauses left out.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Ran {
    before: Duration,
    since: Option<Instant>,
}

impl Ran {
    /// It runs from `now` (it began, or went on).
    pub(super) fn go(&mut self, now: Instant) {
        if self.since.is_none() {
            self.since = Some(now);
        }
    }

    /// It stops running at `now` (paused).
    pub(super) fn stop(&mut self, now: Instant) {
        if let Some(since) = self.since.take() {
            self.before += now.saturating_duration_since(since);
        }
    }

    /// How long it ran, until `now`.
    pub(super) fn until(&self, now: Instant) -> Duration {
        self.before
            + self
                .since
                .map_or(Duration::ZERO, |since| now.saturating_duration_since(since))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(start: Instant, millis: u64, bytes: u64) -> Sample {
        Sample {
            at: start + Duration::from_millis(millis),
            bytes,
            steps: 0,
        }
    }

    /// A steady copy: a speed once two seconds went by, a time left once
    /// four did, what is left over the speed.
    #[test]
    fn a_steady_copy_says_its_speed_then_its_time_left() {
        let start = Instant::now();
        let mut samples = Samples::default();
        samples.restart(at(start, 0, 0));
        // 1 MB a second, sampled every 100 ms (most dropped: every 250).
        for t in (100..=10_000).step_by(100) {
            samples.note(at(start, t, t * 1_000));
        }
        let early = samples.reading(at(start, 1_000, 1_000_000), Left::Bytes(9_000_000));
        assert_eq!(early, Reading::default(), "nothing before two seconds");
        let soon = samples.reading(at(start, 3_000, 3_000_000), Left::Bytes(7_000_000));
        assert!(
            soon.speed.is_some_and(|s| (s - 1e6).abs() < 1e3),
            "{soon:?}"
        );
        assert_eq!(soon.left, None, "no time left before four seconds");
        let later = samples.reading(at(start, 10_000, 10_000_000), Left::Bytes(30_000_000));
        let left = later.left.unwrap().as_secs_f64();
        assert!((left - 30.0).abs() < 0.1, "{left}");
    }

    /// A copy that stops moving (a server not answering) says nothing of
    /// its speed, nor of its time left, rather than a speed falling slowly.
    #[test]
    fn a_copy_that_stopped_moving_says_nothing() {
        let start = Instant::now();
        let mut samples = Samples::default();
        samples.restart(at(start, 0, 0));
        for t in (250..=10_000).step_by(250) {
            samples.note(at(start, t, t * 1_000));
        }
        // Nothing more for six seconds.
        let stalled = samples.reading(at(start, 16_000, 10_000_000), Left::Bytes(5_000_000));
        assert_eq!(stalled, Reading::default());
    }

    /// The speed follows a change within its window; the time left is
    /// reckoned over a longer one, and moves less.
    #[test]
    fn the_speed_follows_a_change_and_the_time_left_moves_less() {
        let start = Instant::now();
        let mut samples = Samples::default();
        samples.restart(at(start, 0, 0));
        let mut bytes = 0;
        for t in (250..=30_000).step_by(250) {
            // 2 MB a second for 20 s, then 1 MB a second.
            bytes += if t <= 20_000 { 500_000 } else { 250_000 };
            samples.note(at(start, t, bytes));
        }
        let now = at(start, 30_000, bytes);
        let reading = samples.reading(now, Left::Bytes(10_000_000));
        let speed = reading.speed.unwrap();
        assert!((speed - 1e6).abs() < 1e4, "the last five seconds: {speed}");
        // Over the last 20 s: 10 s at 2 MB/s, 10 s at 1 MB/s.
        let left = reading.left.unwrap().as_secs_f64();
        assert!((left - 10_000_000.0 / 1.5e6).abs() < 0.2, "{left}");
    }

    /// Armed, the work is measured from what is noted first, its first
    /// bytes, never from the wait before them.
    #[test]
    fn the_wait_for_a_first_byte_is_not_slowness() {
        let start = Instant::now();
        let mut samples = Samples::default();
        samples.note(at(start, 0, 0));
        assert_eq!(
            samples.reading(at(start, 9_000, 0), Left::Bytes(1)),
            Reading::default(),
            "not armed"
        );
        samples.arm();
        // Seven seconds to the first bytes, then 2 MB a second.
        for t in (7_000..=11_000).step_by(250) {
            samples.note(at(start, t, (t - 6_000) * 2_000));
        }
        let reading = samples.reading(at(start, 11_000, 10_000_000), Left::Bytes(20_000_000));
        let speed = reading.speed.unwrap();
        assert!((speed - 2e6).abs() < 1e4, "{speed}");
        assert!((reading.left.unwrap().as_secs_f64() - 10.0).abs() < 0.1);
    }

    /// Paused, nothing is noted nor said; going on, it starts again from
    /// there, what came before forgotten.
    #[test]
    fn a_pause_forgets_what_came_before() {
        let start = Instant::now();
        let mut samples = Samples::default();
        samples.restart(at(start, 0, 0));
        for t in (250..=5_000).step_by(250) {
            samples.note(at(start, t, t * 10_000));
        }
        samples.clear();
        samples.note(at(start, 6_000, 50_000_000));
        assert_eq!(
            samples.reading(at(start, 9_000, 50_000_000), Left::Bytes(1)),
            Reading::default()
        );
        samples.restart(at(start, 60_000, 50_000_000));
        for t in (60_250..=63_000).step_by(250) {
            samples.note(at(start, t, 50_000_000 + (t - 60_000) * 1_000));
        }
        let reading = samples.reading(at(start, 63_000, 53_000_000), Left::Bytes(1));
        assert!(
            reading.speed.is_some_and(|s| (s - 1e6).abs() < 1e3),
            "{reading:?}"
        );
    }

    /// A plan that moves no byte is reckoned by its steps: a time left, no
    /// speed.
    #[test]
    fn a_plan_of_no_bytes_is_reckoned_by_its_steps() {
        let start = Instant::now();
        let mut samples = Samples::default();
        samples.restart(Sample {
            at: start,
            bytes: 0,
            steps: 0,
        });
        for t in (250..=8_000).step_by(250) {
            samples.note(Sample {
                at: start + Duration::from_millis(t),
                bytes: 0,
                steps: usize::try_from(t / 10).unwrap(),
            });
        }
        let now = Sample {
            at: start + Duration::from_secs(8),
            bytes: 0,
            steps: 800,
        };
        let reading = samples.reading(now, Left::Steps(1_000));
        assert_eq!(reading.speed, None);
        assert!((reading.left.unwrap().as_secs_f64() - 10.0).abs() < 0.1);
    }

    /// What ran is counted without its pauses.
    #[test]
    fn what_ran_leaves_its_pauses_out() {
        let start = Instant::now();
        let mut ran = Ran::default();
        ran.go(start);
        ran.stop(start + Duration::from_secs(3));
        ran.go(start + Duration::from_secs(10));
        assert_eq!(
            ran.until(start + Duration::from_secs(12)),
            Duration::from_secs(5)
        );
    }
}
