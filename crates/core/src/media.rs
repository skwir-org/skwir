//! Nonblocking playback sessions and the private media-worker protocol.
//! One acknowledged pixel slot and one latest UI update bound frame memory.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum PlaybackStatus {
    #[default]
    Paused,
    Playing,
    Ended,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Playback {
    pub status: PlaybackStatus,
    pub position: f64,
    pub duration: Option<f64>,
    pub volume: f64,
    pub muted: bool,
    pub rate: f64,
    pub audio_tracks: Vec<String>,
    pub subtitle_tracks: Vec<String>,
    pub audio: i32,
    pub subtitle: i32,
    pub seekable: bool,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub artist: String,
    #[serde(default)]
    pub album: String,
    #[serde(default)]
    pub spectrum: Vec<f32>,
    /// Determined from decoded streams, since container MIME types may be ambiguous.
    #[serde(default)]
    pub audio_only: bool,
}
impl Playback {
    pub fn playing(&self) -> bool {
        self.status == PlaybackStatus::Playing
    }
    pub fn ended(&self) -> bool {
        self.status == PlaybackStatus::Ended
    }
}
impl Default for Playback {
    fn default() -> Self {
        Self {
            status: PlaybackStatus::Paused,
            position: 0.,
            duration: None,
            volume: 1.,
            muted: false,
            rate: 1.,
            audio_tracks: Vec::new(),
            subtitle_tracks: Vec::new(),
            audio: -1,
            subtitle: -1,
            seekable: false,
            title: String::new(),
            artist: String::new(),
            album: String::new(),
            spectrum: Vec::new(),
            audio_only: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub enum Control {
    Play(bool),
    Seek(f64),
    Volume(f64),
    Mute(bool),
    Rate(f64),
    Audio(i32),
    Subtitle(i32),
}
impl Control {
    pub fn valid(&self) -> bool {
        match self {
            Self::Seek(v) => v.is_finite() && *v >= 0.,
            Self::Volume(v) => v.is_finite() && (0. ..=1.).contains(v),
            Self::Rate(v) => v.is_finite() && (0.25..=3.).contains(v),
            Self::Audio(v) => *v >= 0,
            Self::Subtitle(v) => *v >= -1,
            Self::Play(_) | Self::Mute(_) => true,
        }
    }
}

pub struct Frame {
    pub width: u32,
    pub height: u32,
    /// BGRA, ready for GPUI; no UI-thread channel conversion.
    pub bgra: Vec<u8>,
}
#[derive(Default)]
pub struct Update {
    pub state: Option<Playback>,
    pub frame: Option<Frame>,
    /// A control the player could not carry out (for example seeking a
    /// non-seekable stream). Transient: the session stays usable and a
    /// `state` with the same generation restores the actual playback state.
    pub notice: Option<String>,
    /// Fatal: the session has ended and its controls must be withdrawn.
    pub error: Option<String>,
}
impl Update {
    pub fn is_empty(&self) -> bool {
        self.state.is_none()
            && self.frame.is_none()
            && self.notice.is_none()
            && self.error.is_none()
    }
}
pub trait MediaSession: Send + Sync {
    /// Enqueue only; never wait for a decoder or pipe on the UI thread.
    fn control(&self, command: Control) -> anyhow::Result<()>;
    /// Take the latest update, discarding intermediate frames on the producer.
    fn take_update(&self) -> Update;
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Request {
    Control { generation: u64, command: Control },
    Consumed,
}
#[derive(Debug, Serialize, Deserialize)]
pub enum Reply {
    Ready,
    State {
        generation: u64,
        playback: Playback,
    },
    Frame {
        generation: u64,
        width: u32,
        height: u32,
        /// Stream time and pipeline position at delivery, for clock-lateness checks.
        seconds: Option<f64>,
        clock: Option<f64>,
        dropped: Option<u64>,
    },
    /// A control failed; playback continues and a `State` follows.
    Rejected {
        generation: u64,
        message: String,
    },
    /// The runtime is missing. Fatal after `Ready`.
    Unavailable(String),
    /// Fatal: the worker is exiting.
    Error(String),
}
pub const MAX_SIDE: u32 = 1920;
pub const MAX_MESSAGE: usize = 64 * 1024;
pub const PIXELS_FILE: &str = "video.bgra";
pub fn frame_bytes(width: u32, height: u32) -> anyhow::Result<usize> {
    anyhow::ensure!(
        width > 0 && height > 0 && width <= MAX_SIDE && height <= MAX_SIDE,
        "invalid video frame dimensions"
    );
    Ok(width as usize * height as usize * 4)
}
