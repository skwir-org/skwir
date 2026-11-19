//! What the plugin keeps from one run to the next, in the state file
//! (`updates.kept`, ADR-056): when the copy was first seen and the
//! question asked, the last check's day and answer, what was told and
//! when, the highest version run. Days are the user's calendar days
//! (`skwir_platform::calendar`, ADR-017). Lost, it costs at most a notice
//! said again; never a request more than once a day.
//!
//! What is decided from it is decided here, from it alone and the day:
//! whether to ask, whether to tell of a version, whether to say what is
//! new. What was read is taken `as_of` the day first: no day kept is
//! after it.

use serde::{Deserialize, Serialize};

use crate::version::Version;

/// The state file's key.
pub const KEY: &str = "updates.kept";

/// Days after the copy was first seen before the question is asked.
pub const ASK_AFTER: i64 = 2;
/// How many times, on as many days, the question is asked while closed
/// unanswered.
pub const ASKS: u32 = 3;
/// Days between two notices of a new version, unless one is important.
pub const SPACING: i64 = 14;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Kept {
    /// The day a downloaded copy was first looked at.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_seen: Option<i64>,
    /// How many times the question was asked, and the last day it was.
    pub asked: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asked_on: Option<i64>,
    /// Whether the user answered it (or chose in Settings): never asked
    /// again.
    pub answered: bool,
    /// The day of the last check that got an answer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checked_on: Option<i64>,
    /// The site's answer then, as it was read, and its tag for the next
    /// conditional request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub etag: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub important_since: Option<String>,
    /// The newest version told in a notice, and the day it was.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub told: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub told_on: Option<i64>,
    /// The highest version this user has run (What's new).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub highest_run: Option<String>,
}

/// A version to tell of, and whether it is told as important.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Telling {
    pub version: Version,
    pub important: bool,
}

impl Kept {
    /// What the state file holds, or nothing kept yet.
    pub fn read(settings: &skwir_core::settings::Settings) -> Kept {
        settings.get::<Kept>(KEY).unwrap_or_default()
    }

    /// Written back into the state file (saved by the caller).
    pub fn write(&self, settings: &skwir_core::settings::Settings) {
        match toml::Value::try_from(self) {
            Ok(value) => settings.set(KEY, value),
            Err(error) => tracing::warn!(%error, "what the updates plugin keeps is not written"),
        }
    }

    /// What is kept as of `today`: a day kept after it (a clock once set
    /// ahead, a dead battery's, a machine's started before its clock was
    /// set) is read as today, so the question and the notices wait no
    /// longer than they would have. Called on what was read, before it
    /// decides anything, so the day is written back so.
    pub fn as_of(&mut self, today: i64) {
        for day in [
            &mut self.first_seen,
            &mut self.asked_on,
            &mut self.checked_on,
            &mut self.told_on,
        ]
        .into_iter()
        .flatten()
        {
            *day = (*day).min(today);
        }
    }

    /// Whether the question is asked `today`: never answered, asked fewer
    /// than `ASKS` times and not yet today, `ASK_AFTER` days after the copy
    /// was first seen.
    pub fn asks(&self, today: i64) -> bool {
        !self.answered
            && self.asked < ASKS
            && self.asked_on != Some(today)
            && self
                .first_seen
                .is_some_and(|first| today >= first + ASK_AFTER)
    }

    /// The last answer's newest version, and the newest important one.
    pub fn answer(&self) -> (Option<Version>, Option<Version>) {
        let read = |text: &Option<String>| text.as_deref().and_then(Version::parse);
        (read(&self.latest), read(&self.important_since))
    }

    /// Whether `running` is told of the last answer's newest version
    /// `today`: newer than it and newer than what was told, `SPACING` days
    /// after the last notice, or at once while `running` is older than the
    /// newest important release and that release was not told yet. Once
    /// it was, the releases after it wait as any do: told as important
    /// still, as each brings the fix `running` lacks.
    pub fn tells(&self, running: Version, today: i64) -> Option<Telling> {
        let (latest, important_since) = self.answer();
        let latest = latest.filter(|latest| *latest > running)?;
        let told = self.told.as_deref().and_then(Version::parse);
        if told.is_some_and(|told| told >= latest) {
            return None;
        }
        let important = important_since.filter(|important| running < *important);
        let urgent = important.is_some_and(|important| told.is_none_or(|told| told < important));
        let spaced = self
            .told_on
            .is_none_or(|told_on| today - told_on >= SPACING);
        (urgent || spaced).then_some(Telling {
            version: latest,
            important: important.is_some(),
        })
    }

    /// What About and More keep said while `running` is older than the
    /// last answer's newest version.
    pub fn standing(&self, running: Version) -> Option<Telling> {
        let (latest, important_since) = self.answer();
        let latest = latest.filter(|latest| *latest > running)?;
        Some(Telling {
            version: latest,
            important: important_since.is_some_and(|important| running < important),
        })
    }

    /// The version to say what is new of as `running` starts, once: higher
    /// than every version run before. Nothing for a first run (none
    /// known), or a version lower than one already run. Keeps `running`
    /// as the highest where it is.
    pub fn whats_new(&mut self, running: Version) -> Option<Version> {
        let highest = self.highest_run.as_deref().and_then(Version::parse);
        match highest {
            Some(highest) if highest >= running => None,
            Some(_) => {
                self.highest_run = Some(running.to_string());
                Some(running)
            }
            None => {
                self.highest_run = Some(running.to_string());
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ASKS, Kept, SPACING, Telling};
    use crate::version::Version;

    #[test]
    fn the_question_waits_two_days_and_comes_at_most_three_times() {
        let mut kept = Kept::default();
        assert!(!kept.asks(100), "never seen");
        kept.first_seen = Some(100);
        assert!(!kept.asks(100) && !kept.asks(101));
        assert!(kept.asks(102));
        for day in 102..102 + i64::from(ASKS) {
            assert!(kept.asks(day), "day {day}");
            kept.asked += 1;
            kept.asked_on = Some(day);
            assert!(!kept.asks(day), "once a day");
        }
        assert!(!kept.asks(200), "closed unanswered three times");
        let answered = Kept {
            first_seen: Some(100),
            answered: true,
            ..Kept::default()
        };
        assert!(!answered.asks(200));
    }

    #[test]
    fn a_newer_version_is_told_once_and_the_next_after_two_weeks() {
        let running = Version(0, 5, 1);
        let mut kept = Kept {
            latest: Some("0.6.0".into()),
            ..Kept::default()
        };
        let first = kept.tells(running, 10);
        assert_eq!(
            first,
            Some(Telling {
                version: Version(0, 6, 0),
                important: false
            })
        );
        kept.told = Some("0.6.0".into());
        kept.told_on = Some(10);
        assert_eq!(kept.tells(running, 11), None, "told once");
        kept.latest = Some("0.6.1".into());
        assert_eq!(kept.tells(running, 10 + SPACING - 1), None, "spaced");
        assert_eq!(
            kept.tells(running, 10 + SPACING).map(|t| t.version),
            Some(Version(0, 6, 1))
        );
        // Nothing newer than what runs: nothing told, nothing standing.
        kept.latest = Some("0.5.1".into());
        assert_eq!(kept.tells(running, 100), None);
        assert_eq!(kept.standing(running), None);
    }

    #[test]
    fn an_important_release_is_told_at_once() {
        let running = Version(0, 5, 1);
        let kept = Kept {
            latest: Some("0.6.2".into()),
            important_since: Some("0.6.1".into()),
            told: Some("0.6.0".into()),
            told_on: Some(10),
            ..Kept::default()
        };
        assert_eq!(
            kept.tells(running, 11),
            Some(Telling {
                version: Version(0, 6, 2),
                important: true
            })
        );
        // A copy that has the important fix is told as usual.
        assert_eq!(kept.tells(Version(0, 6, 1), 11), None);
        assert!(
            kept.standing(Version(0, 6, 1))
                .is_some_and(|t| !t.important)
        );
    }

    /// The releases after an important one that was told wait the two
    /// weeks as any does, said as important while the copy lacks the fix.
    #[test]
    fn the_release_after_an_important_one_told_waits_two_weeks() {
        let running = Version(0, 5, 1);
        let mut kept = Kept {
            latest: Some("0.6.1".into()),
            important_since: Some("0.6.1".into()),
            ..Kept::default()
        };
        assert!(kept.tells(running, 10).is_some_and(|t| t.important));
        kept.told = Some("0.6.1".into());
        kept.told_on = Some(10);
        for (day, latest) in [(11, "0.6.2"), (12, "0.6.3"), (10 + SPACING - 1, "0.6.4")] {
            kept.latest = Some(latest.into());
            assert_eq!(kept.tells(running, day), None, "{latest} on day {day}");
        }
        assert_eq!(
            kept.tells(running, 10 + SPACING),
            Some(Telling {
                version: Version(0, 6, 4),
                important: true
            })
        );
        // A newer important release, not told yet, is told at once.
        kept.important_since = Some("0.6.4".into());
        assert!(kept.tells(running, 11).is_some_and(|t| t.important));
    }

    /// A day kept after today (a clock once set ahead) is read as today:
    /// the question still comes two days on, a notice two weeks on.
    #[test]
    fn a_day_kept_after_today_is_read_as_today() {
        let ahead = 30_000;
        let mut kept = Kept {
            first_seen: Some(ahead),
            asked_on: Some(ahead),
            checked_on: Some(ahead),
            told: Some("0.6.0".into()),
            told_on: Some(ahead),
            latest: Some("0.6.1".into()),
            ..Kept::default()
        };
        kept.as_of(100);
        assert_eq!(
            (
                kept.first_seen,
                kept.asked_on,
                kept.checked_on,
                kept.told_on
            ),
            (Some(100), Some(100), Some(100), Some(100))
        );
        assert!(!kept.asks(101));
        assert!(kept.asks(102));
        let running = Version(0, 5, 1);
        assert_eq!(kept.tells(running, 100 + SPACING - 1), None);
        assert!(kept.tells(running, 100 + SPACING).is_some());
        // What is not ahead stays.
        let mut earlier = kept.clone();
        earlier.as_of(200);
        assert_eq!(earlier, kept);
    }

    #[test]
    fn what_is_new_is_said_once_and_never_on_a_first_run() {
        let mut kept = Kept::default();
        assert_eq!(kept.whats_new(Version(0, 5, 1)), None, "a first run");
        assert_eq!(kept.whats_new(Version(0, 5, 1)), None, "the same again");
        assert_eq!(kept.whats_new(Version(0, 6, 0)), Some(Version(0, 6, 0)));
        assert_eq!(kept.whats_new(Version(0, 6, 0)), None, "said once");
        // An older copy run in between, then the newer again: nothing.
        assert_eq!(kept.whats_new(Version(0, 5, 1)), None);
        assert_eq!(kept.whats_new(Version(0, 6, 0)), None);
        assert_eq!(kept.highest_run.as_deref(), Some("0.6.0"));
    }

    #[test]
    fn what_is_kept_reads_back_as_written() {
        let kept = Kept {
            first_seen: Some(20_000),
            asked: 1,
            asked_on: Some(20_002),
            answered: true,
            checked_on: Some(20_003),
            etag: Some("\"abc\"".into()),
            latest: Some("0.6.2".into()),
            important_since: None,
            told: Some("0.6.2".into()),
            told_on: Some(20_003),
            highest_run: Some("0.5.1".into()),
        };
        let value = toml::Value::try_from(&kept).unwrap();
        assert_eq!(value.try_into::<Kept>().unwrap(), kept);
        let empty: Kept = toml::Value::Table(toml::Table::new()).try_into().unwrap();
        assert_eq!(empty, Kept::default());
    }
}
