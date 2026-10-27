//! What is playing, as core-owned data, and what others ask of the player.
//!
//! Whoever plays (the shell's audio player) publishes a [`NowPlaying`] here;
//! whoever wants to show or steer it (the desktop's media controls, through a
//! plugin; a script; an agent) reads it and leaves a [`PlayerRequest`]. Neither
//! knows the other, as with `pending`: two plugins agree on this and on
//! nothing else. Requests are data the player takes when it next wakes; nobody
//! calls into the UI from another thread.
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::time::Instant;

use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};

use crate::command::Topic;
use crate::media::{Control, PlaybackStatus};

// Each is a separate fact about what the player can do right now.
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Debug, PartialEq)]
pub struct NowPlaying {
    pub path: PathBuf,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub status: PlaybackStatus,
    /// Where playback stood at `at`; a reader works out where it stands now.
    pub position: f64,
    pub at: Instant,
    pub rate: f64,
    pub duration: Option<f64>,
    /// Where the volume control stands, 0..=1.
    pub volume: f64,
    pub can_next: bool,
    pub can_previous: bool,
    pub can_seek: bool,
    /// A picture of the cover, as a file others may read.
    pub cover: Option<PathBuf>,
    /// Playback goes on when the selection moves (the player is pinned).
    pub pinned: bool,
}

impl NowPlaying {
    /// Where playback stands at this moment.
    pub fn position_now(&self) -> f64 {
        let elapsed = if self.status == PlaybackStatus::Playing {
            self.at.elapsed().as_secs_f64() * self.rate
        } else {
            0.
        };
        let position = self.position + elapsed;
        self.duration
            .map_or(position, |duration| position.min(duration))
    }

    /// Whether `other` differs in anything a listener is told about: the
    /// position running on by itself is not news, a jump is.
    fn news(&self, other: &NowPlaying) -> bool {
        let jumped = (other.position - self.position_now()).abs() > 1.5;
        jumped
            || (
                &self.path,
                &self.title,
                &self.artist,
                &self.album,
                self.status,
            ) != (
                &other.path,
                &other.title,
                &other.artist,
                &other.album,
                other.status,
            )
            || (
                self.duration,
                self.can_next,
                self.can_previous,
                self.can_seek,
                self.pinned,
            ) != (
                other.duration,
                other.can_next,
                other.can_previous,
                other.can_seek,
                other.pinned,
            )
            || (self.volume - other.volume).abs() > 0.004
            || (self.rate - other.rate).abs() > 0.004
            || self.cover != other.cover
    }
}

/// What can be asked of the player. Serializable: it is also a command's
/// argument (`player.request`).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PlayerRequest {
    Play,
    Pause,
    PlayPause,
    /// Stop and let go of the track: a pinned player is unpinned.
    Stop,
    Next,
    Previous,
    /// Seconds from where it stands, either way.
    SeekBy(f64),
    SeekTo(f64),
    Volume(f64),
    /// Its sound off, or on again.
    Mute,
    /// How fast it plays, 1 as recorded; the pitch stays.
    Rate(f64),
    /// A film's audio track, by its place among them.
    Audio(i32),
    /// A film's subtitles, by their place among them; -1 for none.
    Subtitle(i32),
    /// A film on the whole screen, or back from it.
    Fullscreen,
    /// Bring the window forward and show the track.
    Raise,
}

impl PlayerRequest {
    pub fn valid(&self) -> bool {
        match *self {
            Self::SeekBy(v) => v.is_finite(),
            Self::SeekTo(v) => Control::Seek(v).valid(),
            Self::Volume(v) => Control::Volume(v).valid(),
            Self::Rate(v) => Control::Rate(v).valid(),
            Self::Audio(v) => Control::Audio(v).valid(),
            Self::Subtitle(v) => Control::Subtitle(v).valid(),
            _ => true,
        }
    }
}

#[derive(Deserialize, schemars::JsonSchema)]
struct RequestArgs {
    request: PlayerRequest,
}

/// The player as commands, so a key binding, a script and an agent steer it
/// the way the desktop's media keys do.
pub(crate) fn register(app: &crate::App) {
    app.commands.register::<RequestArgs, _>(
        "core", Topic::Player,
        "player.request",
        "Steer the player",
        "Ask whatever is playing to play, pause, stop, step to the next or previous track, seek, change its volume or its speed, choose a film's audio or subtitles or show it on the whole screen; it is carried out when the player next wakes",
        |args, app| {
            anyhow::ensure!(args.request.valid(), "this request makes no sense");
            anyhow::ensure!(app.player.get().is_some(), "nothing is playing");
            anyhow::ensure!(app.player.request(args.request), "the player is not answering");
            app.wake();
            Ok(crate::command::Outcome::Done)
        },
    )
        .shows();
    app.commands
        .register::<crate::command::NoArgs, _>(
            "core",
            Topic::Player,
            "player.now",
            "What is playing",
            "The track that plays, its state and where it stands, or null",
            |_, app| {
                Ok(crate::command::Outcome::Value(app.player.get().map_or(
                    serde_json::Value::Null,
                    |now| {
                        serde_json::json!({
                            "path": crate::os_path::PathArg::from(now.path.as_path()),
                            "title": now.title,
                            "artist": now.artist,
                            "album": now.album,
                            "status": now.status,
                            "position": now.position_now(),
                            "duration": now.duration,
                            "volume": now.volume,
                            "can_next": now.can_next,
                            "can_previous": now.can_previous,
                            "pinned": now.pinned,
                        })
                    },
                )))
            },
        )
        .data();
}

#[derive(Default)]
pub struct Player {
    now: RwLock<Option<Arc<NowPlaying>>>,
    revision: AtomicU64,
    requests: Mutex<Vec<PlayerRequest>>,
    /// Who listens, by owner: a contribution like any other, withdrawn when
    /// its plugin is disabled.
    listeners: Mutex<Vec<(String, mpsc::Sender<()>)>>,
    /// Refuses a disabled plugin's listener.
    gate: Arc<crate::registry::OwnerGate>,
}

/// Requests kept for a player that does not wake: more is a flood, not intent.
const PENDING_REQUESTS: usize = 64;

impl Player {
    pub fn get(&self) -> Option<Arc<NowPlaying>> {
        self.now.read().clone()
    }

    /// Advances whenever listeners were told of a change.
    pub fn revision(&self) -> u64 {
        self.revision.load(Ordering::Acquire)
    }

    /// The player says what it plays, as often as it likes: listeners hear of
    /// it only when something they are told about changed, and the position
    /// is kept fresh either way. `None` when nothing plays any more.
    pub fn publish(&self, now: Option<NowPlaying>) {
        let news = {
            let mut slot = self.now.write();
            let news = match (slot.as_deref(), now.as_ref()) {
                (None, None) => false,
                (Some(old), Some(new)) => old.news(new),
                _ => true,
            };
            *slot = now.map(Arc::new);
            news
        };
        if news {
            self.revision.fetch_add(1, Ordering::Release);
            self.listeners
                .lock()
                .retain(|(_, listener)| listener.send(()).is_ok());
        }
    }

    pub fn gated(gate: Arc<crate::registry::OwnerGate>) -> Self {
        Player {
            gate,
            ..Player::default()
        }
    }

    /// A message each time the revision advances, until the receiver is
    /// dropped or `owner` is withdrawn. A disabled owner's receiver is
    /// closed at once.
    pub fn subscribe(&self, owner: &str) -> mpsc::Receiver<()> {
        let (tx, rx) = mpsc::channel();
        let mut listeners = self.listeners.lock();
        if self.gate.is_open(owner) {
            listeners.push((owner.to_owned(), tx));
        }
        rx
    }

    /// The owners listening, for whoever checks that a plugin left no trace.
    pub fn listeners(&self) -> Vec<String> {
        self.listeners
            .lock()
            .iter()
            .map(|(owner, _)| owner.clone())
            .collect()
    }

    /// Their receivers see the channel close, which ends their wait.
    pub fn unregister_owner(&self, owner: &str) {
        self.listeners.lock().retain(|(known, _)| known != owner);
    }

    /// Leave a request for the player; wake it (`App::wake`) afterwards.
    /// Returns whether it was kept.
    pub fn request(&self, request: PlayerRequest) -> bool {
        let mut requests = self.requests.lock();
        let kept = request.valid() && requests.len() < PENDING_REQUESTS;
        if kept {
            requests.push(request);
        }
        kept
    }

    pub fn take_requests(&self) -> Vec<PlayerRequest> {
        std::mem::take(&mut *self.requests.lock())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(position: f64, status: PlaybackStatus) -> NowPlaying {
        NowPlaying {
            path: "/music/it's a café.flac".into(),
            title: "A Song".into(),
            artist: "Someone".into(),
            album: String::new(),
            status,
            position,
            at: Instant::now(),
            rate: 1.,
            duration: Some(200.),
            volume: 1.,
            can_next: true,
            can_previous: false,
            can_seek: true,
            cover: None,
            pinned: false,
        }
    }

    #[test]
    fn listeners_hear_of_changes_not_of_time_passing() {
        let player = Player::default();
        let heard = player.subscribe("test");
        player.publish(Some(track(0., PlaybackStatus::Playing)));
        assert_eq!((player.revision(), heard.try_iter().count()), (1, 1));
        // The position running on is not news; the record still says it.
        player.publish(Some(track(0.4, PlaybackStatus::Playing)));
        assert_eq!((player.revision(), heard.try_iter().count()), (1, 0));
        assert!((player.get().unwrap().position - 0.4).abs() < 1e-9);
        // A jump is, and so is a pause, a new title and the end of it all.
        player.publish(Some(track(90., PlaybackStatus::Playing)));
        player.publish(Some(track(90., PlaybackStatus::Paused)));
        let mut renamed = track(90., PlaybackStatus::Paused);
        renamed.title = "Another".into();
        player.publish(Some(renamed));
        player.publish(None);
        player.publish(None);
        assert_eq!((player.revision(), heard.try_iter().count()), (5, 4));
        // A listener that went away is forgotten, not an error.
        drop(heard);
        player.publish(Some(track(0., PlaybackStatus::Paused)));
        assert!(player.listeners().is_empty());
        // A withdrawn owner's receiver sees the channel close.
        let plugin = player.subscribe("a-plugin");
        assert_eq!(player.listeners(), ["a-plugin"]);
        player.unregister_owner("a-plugin");
        assert!(player.listeners().is_empty() && plugin.recv().is_err());
    }

    #[test]
    fn the_position_runs_on_while_playing_and_stops_at_the_end() {
        let mut playing = track(199.5, PlaybackStatus::Playing);
        playing.at = Instant::now()
            .checked_sub(std::time::Duration::from_secs(5))
            .unwrap();
        assert!(
            (playing.position_now() - 200.).abs() < 1e-9,
            "not past the end"
        );
        playing.position = 10.;
        playing.rate = 2.;
        assert!((playing.position_now() - 20.).abs() < 0.2);
        let mut paused = track(10., PlaybackStatus::Paused);
        paused.at = Instant::now()
            .checked_sub(std::time::Duration::from_secs(5))
            .unwrap();
        assert!((paused.position_now() - 10.).abs() < 1e-9);
    }

    #[test]
    fn requests_wait_for_the_player_and_nonsense_is_refused() {
        let player = Player::default();
        assert!(player.request(PlayerRequest::PlayPause));
        assert!(player.request(PlayerRequest::SeekBy(-5.)));
        assert!(!player.request(PlayerRequest::Volume(7.)));
        assert!(!player.request(PlayerRequest::SeekTo(f64::NAN)));
        assert!(!player.request(PlayerRequest::Rate(0.)));
        assert!(!player.request(PlayerRequest::Subtitle(-2)));
        assert_eq!(
            player.take_requests(),
            [PlayerRequest::PlayPause, PlayerRequest::SeekBy(-5.)]
        );
        assert!(player.take_requests().is_empty());
        for _ in 0..200 {
            player.request(PlayerRequest::Next);
        }
        assert_eq!(player.take_requests().len(), PENDING_REQUESTS);
    }
}
