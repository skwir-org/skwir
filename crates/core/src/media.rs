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
    /// Levels of `SPECTRUM_BANDS` logarithmic bands, low to high, each 0..=1.
    #[serde(default)]
    pub spectrum: Vec<f32>,
    /// What the stream says about itself beyond title, artist and album
    /// (track, year, genre, codec, bitrate, sample rate, channels), as label
    /// and value, for the panel's Details. Bounded by the worker.
    #[serde(default)]
    pub details: Vec<(String, String)>,
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
            details: Vec::new(),
            audio_only: false,
        }
    }
}

/// Bands of the spectrum a player shows, and of FFT bins it is folded from.
pub const SPECTRUM_BANDS: usize = 32;
pub const SPECTRUM_BINS: u32 = 512;
/// The span shown: below is rumble, above is air hardly anyone hears.
const LOWEST_HZ: f32 = 40.;
const HIGHEST_HZ: f32 = 16_000.;
/// The quietest level drawn, and how much music thins out per octave: a
/// spectrum falls towards the treble, and drawn flat the top half would
/// never move. The tilt is the usual one of analysers.
const FLOOR_DB: f32 = -60.;
const TILT_DB_PER_OCTAVE: f32 = 3.;

/// Fold an FFT's linear bins (dB, 0 Hz to `rate / 2`) into `SPECTRUM_BANDS`
/// logarithmic ones, each 0..=1. Music is heard in octaves: in linear bands
/// of 700 Hz nearly all of it sits in the first five and the rest stay dark.
/// A band takes the loudest bin it covers; below a bin's width, where a band
/// covers none, it reads between the two bins around it.
pub fn log_bands(bins_db: &[f32], rate: u32) -> Vec<f32> {
    if bins_db.is_empty() || rate == 0 {
        return vec![0.; SPECTRUM_BANDS];
    }
    let width = rate as f32 / 2. / bins_db.len() as f32;
    let top = HIGHEST_HZ.min(rate as f32 / 2.);
    let edge =
        |band: usize| LOWEST_HZ * (top / LOWEST_HZ).powf(band as f32 / SPECTRUM_BANDS as f32);
    let level = |bin: usize| {
        let db = bins_db[bin.min(bins_db.len() - 1)];
        if db.is_finite() { db } else { FLOOR_DB }
    };
    (0..SPECTRUM_BANDS)
        .map(|band| {
            let (from, to) = (edge(band), edge(band + 1));
            let (first, last) = ((from / width) as usize, (to / width) as usize);
            let db = if last > first {
                (first..last).map(level).fold(FLOOR_DB, f32::max)
            } else {
                // Narrower than a bin: between the centres of its neighbours.
                let at = (f32::midpoint(from, to) / width - 0.5).max(0.);
                let (below, share) = (at as usize, at.fract());
                level(below) * (1. - share) + level(below + 1) * share
            };
            if db <= FLOOR_DB {
                // Silence is silence: the tilt lifts what is there, not the floor.
                return 0.;
            }
            let tilt = TILT_DB_PER_OCTAVE * ((from * to).sqrt() / 1000.).log2();
            ((db + tilt - FLOOR_DB) / -FLOOR_DB).clamp(0., 1.)
        })
        .collect()
}

/// Loudness is heard roughly as the cube of amplitude: a volume control set
/// to half should sound half as loud, not four fifths. This is the amplitude
/// for a control at `position` (0..=1), the "cubic" scale of `GStreamer`.
pub fn amplitude(position: f64) -> f64 {
    position.clamp(0., 1.).powi(3)
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub enum Control {
    Play(bool),
    Seek(f64),
    /// Where the volume control stands, 0..=1: perceptual, see [`amplitude`].
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
    /// The session moved on to the file queued with `play_next`.
    pub advanced: Option<std::path::PathBuf>,
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
            && self.advanced.is_none()
            && self.notice.is_none()
            && self.error.is_none()
    }
}
pub trait MediaSession: Send + Sync {
    /// Enqueue only; never wait for a decoder or pipe on the UI thread.
    fn control(&self, command: Control) -> anyhow::Result<()>;
    /// The file to go on with when this one ends, without a gap: the engine
    /// pre-rolls it and switches at the sample boundary. `None` forgets it.
    /// A session that cannot do this says so; the caller then opens the next
    /// track itself when the end is reported.
    fn play_next(&self, _path: Option<std::path::PathBuf>) -> anyhow::Result<()> {
        anyhow::bail!("this session plays one file")
    }
    /// Take the latest update, discarding intermediate frames on the producer.
    fn take_update(&self) -> Update;
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Request {
    Control {
        generation: u64,
        command: Control,
    },
    /// See `MediaSession::play_next`.
    Next {
        path: Option<std::path::PathBuf>,
    },
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
    /// The queued file has taken over; the state that follows is its.
    Advanced {
        path: std::path::PathBuf,
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A spectrum that is silent except for one tone.
    fn tone(hz: f32, rate: u32) -> Vec<f32> {
        let mut bins = vec![-80.; SPECTRUM_BINS as usize];
        let width = rate as f32 / 2. / bins.len() as f32;
        bins[(hz / width) as usize] = -6.;
        bins
    }

    fn loudest(bands: &[f32]) -> usize {
        (0..bands.len())
            .max_by(|a, b| bands[*a].total_cmp(&bands[*b]))
            .unwrap()
    }

    #[test]
    fn music_spreads_over_the_ring_instead_of_its_first_bands() {
        let rate = 44_100;
        // Octaves are evenly spaced: 110, 440, 1760 and 7040 Hz land a quarter
        // of the ring apart, where linear bands would put three in band 0..2.
        let bands: Vec<usize> = [110., 440., 1760., 7040.]
            .into_iter()
            .map(|hz| loudest(&log_bands(&tone(hz, rate), rate)))
            .collect();
        assert!(
            bands.windows(2).all(|pair| pair[1] > pair[0] + 5),
            "{bands:?}"
        );
        assert!(bands[0] >= 3 && bands[3] <= 29, "{bands:?}");
        // A bass note between two bins still shows, in a band narrower than a bin.
        let bass = log_bands(&tone(60., rate), rate);
        assert!(bass[..6].iter().any(|level| *level > 0.5), "{bass:?}");
        // Silence, garbage and nothing at all are all a still ring.
        assert!(
            log_bands(&vec![-90.; 512], rate)
                .iter()
                .all(|level| *level == 0.)
        );
        assert!(
            log_bands(&[f32::NAN; 512], rate)
                .iter()
                .all(|level| *level == 0.)
        );
        assert_eq!(log_bands(&[], rate).len(), SPECTRUM_BANDS);
    }

    #[test]
    fn half_the_control_sounds_like_half_the_volume() {
        assert!((amplitude(0.5) - 0.125).abs() < 1e-12);
        assert!((amplitude(1.) - 1.).abs() < 1e-12);
        assert!(amplitude(-3.) == 0. && (amplitude(7.) - 1.).abs() < 1e-12);
    }
}
