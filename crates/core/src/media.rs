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
    pub error: Option<String>,
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
    Unavailable(String),
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
