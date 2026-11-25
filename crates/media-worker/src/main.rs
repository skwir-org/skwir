//! `GStreamer` is linked only here. The pipeline owns A/V timing; appsink drops
//! late frames. A single acknowledged RAM-backed slot carries packed BGRA.

use anyhow::Context;
use gstreamer::{self as gst, prelude::*};
use gstreamer_app::AppSink;
use gstreamer_video::{VideoFrameExt, VideoFrameRef, VideoInfo};
use skwir_core::media::{
    Control, MAX_MESSAGE, MAX_SIDE, PIXELS_FILE, Playback, PlaybackStatus, Reply, Request,
};
use std::{
    fs::OpenOptions,
    io::{Seek, SeekFrom, Write},
    sync::{
        Arc, Mutex, PoisonError, TryLockError,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

fn reply(message: &Reply) -> anyhow::Result<()> {
    let mut out = std::io::stdout().lock();
    let line = skwir_core::jsonl::encode(message)?;
    anyhow::ensure!(line.len() <= MAX_MESSAGE, "video message exceeds limit");
    out.write_all(&line)?;
    out.flush()?;
    Ok(())
}

/// How far a volume read back may stand from the one set and still be it:
/// the server holds a stream's volume in steps of its own, and a control
/// set to 0.5 comes back as 0.4999…
const SOUND_ROUNDING: f64 = 0.005;

// Independent switches of one player, not a state machine in disguise.
#[allow(clippy::struct_excessive_bools)]
struct Player {
    pipeline: gst::Element,
    sink: AppSink,
    spectrum: Option<gst::Element>,
    audio_only: bool,
    /// The file to go on with, handed to playbin in `about-to-finish`, and
    /// what it was handed (a switch is reported once its stream starts).
    next: Arc<Mutex<Option<std::path::PathBuf>>>,
    handed: Arc<Mutex<Option<std::path::PathBuf>>>,
    state: Playback,
    generation: u64,
    streams: Vec<gst::Stream>,
    /// The streams asked of the current preroll and not yet said selected.
    selection: Option<Vec<gst::glib::GString>>,
    /// The tracks the next preroll's collection is answered with, by the
    /// bus's sync handler (`Wanted`).
    wanted: Arc<Mutex<Wanted>>,
    restore: Option<f64>,
    seeking: bool,
    /// The loop's inbox, for the fences a track change sends.
    events: Arc<Events>,
    /// Fences sent and not yet reached: while there is one, what the loop
    /// takes from the bus was posted before the latest READY (see `fence`).
    fences: usize,
    /// Whether frames are sent (`Control::Frames`): not while the window
    /// is not shown. The sink drops what is not pulled.
    frames: bool,
}
impl Drop for Player {
    fn drop(&mut self) {
        let _ = self.pipeline.set_state(gst::State::Null);
    }
}
impl Player {
    /// Builds the pipeline and wires its bus and sink to `events` before any
    /// state change, so no message can be queued where the loop cannot see it.
    fn new(path: &std::path::Path, events: &Arc<Events>) -> anyhow::Result<Self> {
        let sink = frame_sink()?;
        let uri = gst::glib::filename_to_uri(path, None)?;
        let pipeline = gst::ElementFactory::make("playbin3")
            .property("uri", uri.as_str())
            .property("video-sink", upright(&sink)?)
            .build()?;
        let audio_only = std::env::var_os("SKWIR_MEDIA_AUDIO_ONLY").is_some();
        if audio_only {
            let flags = pipeline.property_value("flags");
            if let Some(class) = gst::glib::FlagsClass::with_type(flags.type_())
                && let Ok(flags) = class.unset_by_nick(flags, "video")
            {
                pipeline.set_property_from_value("flags", &flags);
            }
        }
        // The audio filter keeps the pitch when the speed changes, and for
        // audio measures the spectrum: compact bins, no visualization frames.
        let Filter {
            element: filter,
            spectrum,
        } = audio_filter(audio_only);
        if let Some(filter) = &filter {
            pipeline.set_property("audio-filter", filter);
        }
        // Decoder parallelism is a preview resource policy: libav's automatic
        // thread count otherwise retains a frame pool proportional to host CPUs.
        pipeline.connect("element-setup", false, |values| {
            if let Ok(element) = values[1].get::<gst::Element>()
                && element.find_property("max-threads").is_some()
            {
                element.set_property("max-threads", 4i32);
            }
            if let Ok(element) = values[1].get::<gst::Element>()
                && element.find_property("n-threads").is_some()
            {
                element.set_property("n-threads", 4u32);
            }
            None
        });
        // Tests may request a clocked fake audio sink without changing desktop volume.
        if std::env::var_os("SKWIR_MEDIA_TEST_AUDIO").is_some() {
            let audio = gst::ElementFactory::make("fakesink")
                .property("sync", true)
                .build()?;
            pipeline.set_property("audio-sink", audio);
        }
        // Gapless: when the current file is nearly decoded, playbin asks for
        // the next URI and pre-rolls it, switching at the sample boundary.
        // The signal comes from a streaming thread: it only takes what the
        // loop left for it.
        let next: Arc<Mutex<Option<std::path::PathBuf>>> = Arc::default();
        let handed: Arc<Mutex<Option<std::path::PathBuf>>> = Arc::default();
        let wanted = Arc::new(Mutex::new(Wanted::opened()));
        // Whether the file playing is a film, as its collection said
        // (`plays_film`): one never goes on gaplessly.
        let film = Arc::new(AtomicBool::new(false));
        {
            let (next, handed) = (next.clone(), handed.clone());
            let told = Arc::clone(events);
            let wanted = Arc::clone(&wanted);
            let film = Arc::clone(&film);
            pipeline.connect("about-to-finish", false, move |values| {
                // A film plays to its end, and the player opens the next
                // file then, with a gap (`plays_film`).
                if film.load(Ordering::Acquire) {
                    return None;
                }
                let path = next.lock().unwrap_or_else(PoisonError::into_inner).take()?;
                // A file gone since it was queued (another program removed
                // it) is not handed over, which would end the session in an
                // error: the track ends, and the player, opening the next,
                // finds it gone and goes on past it (ADR-136).
                if std::fs::metadata(&path).is_err() {
                    return None;
                }
                if let (Ok(pipeline), Ok(uri)) = (
                    values[0].get::<gst::Element>(),
                    gst::glib::filename_to_uri(&path, None),
                ) {
                    // Before its collection can be posted (`hand_over`).
                    hand_over(&wanted);
                    pipeline.set_property("uri", uri.as_str());
                    *handed.lock().unwrap_or_else(PoisonError::into_inner) = Some(path.clone());
                    // Open from now on: the manager keeps it so, for a plan
                    // that would take it (ADR-136).
                    let _ = told.tx.send(Event::Handing(path));
                }
                None
            });
        }
        // The sync handler runs on the posting thread and hands the message
        // itself to the loop: wake and payload are one object, so the loop can
        // never drain an empty queue and sleep while a message is pending.
        // A preroll's stream collection is answered here, before the
        // message is let go (`Wanted`).
        let bus = pipeline.bus().context("pipeline bus")?;
        let bus_events = events.clone();
        let bus_wanted = Arc::clone(&wanted);
        // Weak: the pipeline holds its bus, which holds this handler.
        let bus_pipeline = pipeline.downgrade();
        bus.set_sync_handler(move |_, message| {
            let _ = bus_events.tx.send(Event::Bus(message.clone()));
            if let gst::MessageView::StreamCollection(collection) = message.view()
                && let Some(pipeline) = bus_pipeline.upgrade()
            {
                // The latest input's: the file playing, as playbin asks
                // for the next (it is that file's last collection then).
                film.store(
                    plays_film(&collection.stream_collection(), !audio_only),
                    Ordering::Release,
                );
                answer(
                    &pipeline,
                    &collection.stream_collection(),
                    &bus_wanted,
                    &bus_events,
                    !audio_only,
                );
            }
            gst::BusSyncReply::Drop
        });
        // The volume control is the stream's own: the sound server gives a
        // stream back the volume and mute its application had, and its
        // mixer may move them. What the control shows is what is heard.
        for property in ["volume", "mute"] {
            let sound_events = Arc::clone(events);
            pipeline.connect_notify(Some(property), move |_, _| {
                let _ = sound_events.tx.send(Event::Sound);
            });
        }
        let sample_events = events.clone();
        let preroll_events = events.clone();
        sink.set_callbacks(
            gstreamer_app::AppSinkCallbacks::builder()
                .new_sample(move |_| {
                    sample_events.wake();
                    Ok(gst::FlowSuccess::Ok)
                })
                .new_preroll(move |_| {
                    preroll_events.wake();
                    Ok(gst::FlowSuccess::Ok)
                })
                .build(),
        );
        let player = Self {
            pipeline,
            sink,
            spectrum,
            audio_only,
            next,
            handed,
            state: Playback {
                audio_only,
                ..Playback::default()
            },
            generation: 0,
            streams: Vec::new(),
            selection: None,
            wanted,
            restore: None,
            seeking: false,
            events: Arc::clone(events),
            fences: 0,
            frames: true,
        };
        player.pipeline.set_state(gst::State::Paused)?;
        Ok(player)
    }

    /// Whether the last asynchronous state change has completed.
    fn settled(&self) -> bool {
        self.pipeline.state(gst::ClockTime::ZERO).0 == Ok(gst::StateChangeSuccess::Success)
    }

    fn seek(&mut self, seconds: f64, rate: f64) -> anyhow::Result<()> {
        if let Some(position) = &mut self.restore {
            *position = seconds;
            self.state.position = seconds;
            self.state.rate = rate;
            return Ok(());
        }
        anyhow::ensure!(self.state.seekable, "this video does not support seeking");
        let seconds = seconds.min(self.state.duration.unwrap_or(seconds));
        self.pipeline.seek(
            rate,
            gst::SeekFlags::FLUSH | gst::SeekFlags::ACCURATE,
            gst::SeekType::Set,
            gst::ClockTime::from_nseconds((seconds * 1e9) as u64),
            gst::SeekType::None,
            gst::ClockTime::NONE,
        )?;
        self.seeking = true;
        self.state.position = seconds;
        self.state.rate = rate;
        if self.state.ended() {
            self.state.status = PlaybackStatus::Paused;
        }
        Ok(())
    }

    fn control(&mut self, command: Control) -> anyhow::Result<()> {
        anyhow::ensure!(command.valid(), "invalid playback control");
        match command {
            Control::Play(play) => {
                if play && self.state.ended() {
                    self.seek(0., self.state.rate)?;
                }
                if self.restore.is_none() {
                    self.pipeline.set_state(if play {
                        gst::State::Playing
                    } else {
                        gst::State::Paused
                    })?;
                }
                self.state.status = if play {
                    PlaybackStatus::Playing
                } else {
                    PlaybackStatus::Paused
                };
            }
            Control::Seek(seconds) => self.seek(seconds, self.state.rate)?,
            Control::Rate(rate) => self.seek(self.state.position, rate)?,
            Control::Volume(volume) => {
                // The control is perceptual; the pipeline wants an amplitude.
                self.pipeline
                    .set_property("volume", skwir_core::media::amplitude(volume));
                self.state.volume = volume;
            }
            Control::Mute(muted) => {
                self.pipeline.set_property("mute", muted);
                self.state.muted = muted;
            }
            Control::Audio(index) => {
                anyhow::ensure!(
                    (index as usize) < self.state.audio_tracks.len(),
                    "unknown audio track"
                );
                self.switch_tracks(index, self.state.subtitle)?;
            }
            Control::Subtitle(index) => {
                anyhow::ensure!(
                    index == -1 || (index as usize) < self.state.subtitle_tracks.len(),
                    "unknown subtitle track"
                );
                self.switch_tracks(self.state.audio, index)?;
            }
            Control::Frames(on) => {
                let again = on && !self.frames;
                self.frames = on;
                // Shown again paused, the frame of now is drawn again where
                // it is: what the window last drew may be from before.
                if again && !self.state.playing() && self.state.seekable && self.restore.is_none() {
                    self.seek(self.state.position, self.state.rate)?;
                }
            }
        }
        Ok(())
    }

    fn refresh(&mut self) {
        // Flushing seeks re-preroll asynchronously. Keep the requested position
        // until the pipeline completes that transition, even if an older
        // AsyncDone message is still queued on its bus.
        if self.seeking && self.settled() {
            self.seeking = false;
        }
        if self.restore.is_none() && !self.seeking {
            self.state.position = self
                .pipeline
                .query_position::<gst::ClockTime>()
                .map_or(self.state.position, |t| t.nseconds() as f64 / 1e9);
        }
        if let Some(duration) = self.pipeline.query_duration::<gst::ClockTime>() {
            self.state.duration = Some(duration.nseconds() as f64 / 1e9);
        }
        let mut seeking = gst::query::Seeking::new(gst::Format::Time);
        self.state.seekable = self.pipeline.query(&mut seeking) && seeking.result().0;
    }

    /// The volume and mute the pipeline has, as the sound server holds
    /// them, taken into the state where they moved by more than rounding;
    /// whether they did.
    fn follow_sound(&mut self) -> bool {
        let volume = skwir_core::media::position(self.pipeline.property::<f64>("volume"));
        let muted = self.pipeline.property::<bool>("mute");
        let moved =
            (volume - self.state.volume).abs() > SOUND_ROUNDING || muted != self.state.muted;
        if moved {
            self.state.volume = volume;
            self.state.muted = muted;
        }
        moved
    }

    fn tracks(&self, kind: gst::StreamType) -> Vec<String> {
        self.streams
            .iter()
            .filter(|s| s.stream_type().contains(kind))
            .enumerate()
            .map(|(i, stream)| {
                let tags = stream.tags();
                let label = tags.as_ref().and_then(|tags| {
                    tags.get::<gst::tags::Title>()
                        .map(|v| v.get().to_string())
                        .or_else(|| {
                            tags.get::<gst::tags::LanguageCode>()
                                .map(|v| v.get().to_string())
                        })
                });
                format!(
                    "{}{}",
                    i + 1,
                    label.map_or_else(String::new, |s| format!(
                        ": {}",
                        s.chars().take(64).collect::<String>()
                    ))
                )
            })
            .collect()
    }
    fn switch_tracks(&mut self, audio: i32, subtitle: i32) -> anyhow::Result<()> {
        // Dynamic output replacement can deadlock a paused subtitle pipeline.
        // Re-preroll from READY with the desired streams, then restore position.
        // The UI retains its last image throughout; no stale preroll is published.
        let position = self.restore.unwrap_or(self.state.position);
        // Held while the pipeline returns to READY (`answer`), let go
        // before the new preroll starts, whose collection is answered.
        let wanted = Arc::clone(&self.wanted);
        let mut wanted = wanted.lock().unwrap_or_else(PoisonError::into_inner);
        self.pipeline.set_state(gst::State::Ready)?;
        self.fence();
        self.state.audio = audio;
        self.state.subtitle = subtitle;
        self.state.seekable = false;
        *wanted = Wanted {
            audio,
            subtitle,
            answered: false,
            handed: false,
        };
        drop(wanted);
        self.selection = None;
        self.restore = Some(position);
        self.pipeline.set_state(gst::State::Paused)?;
        Ok(())
    }
    /// Mark where the preroll abandoned by a return to READY ends. Messages
    /// its threads posted may still wait in the loop's inbox, the stream
    /// collection among them: selecting from that one asks decodebin3 for
    /// streams of a collection it no longer knows, and the video branch never
    /// comes back (two quick track changes while playing did this). The bus
    /// reaches the inbox through the sync handler, in order, and once
    /// `set_state(Ready)` has returned the old preroll's threads have
    /// stopped: everything before the fence is stale, and the loop drops it.
    fn fence(&mut self) {
        self.fences += 1;
        let _ = self.events.tx.send(Event::Fence);
    }
}

/// The tracks a preroll's stream collection is answered with, shared by the
/// loop (which sets them as a track change returns to READY) and the bus's
/// sync handler (which answers with them).
///
/// decodebin3 posts its collection with its input lock let go, so that a
/// handler on the posting thread may ask for streams before it goes on;
/// asked for nothing by then, it takes one stream of each kind, subtitles
/// included. The loop's answer, from its own thread, came after that now
/// and then, and the subtitle it then took away, or the audio track it
/// changed, was output replaced in a paused pipeline: the preroll waited
/// for good, the video never came back (`tools/media/check.py` stopped so
/// in about one run in five on a VP9 clip on Linux, two in three under
/// Wine; 0.4.0's helper too).
///
/// A track change holds it while the pipeline returns to READY: the
/// collection the abandoned preroll posts meanwhile, answered, would wait
/// on that change, which waits for the posting thread (two changes in a
/// row while playing did this), so it is left unanswered, and fenced off
/// (`Player::fence`). An answer under way holds it until it is sent, and
/// the change waits for it.
///
/// Each input's first collection is answered: the file opened, a track
/// change's preroll, and a next file playbin goes on with (`hand_over`):
/// its first audio track, and its first subtitle where it has one, as the
/// state says once it starts (`StreamStart`), which takes the subtitle
/// away again by a track change where the file before showed none
/// (`next_tracks`). Not asked it, the switch stops for good: playbin3,
/// choosing first for the collection, has already asked playsink for a
/// subtitle output, and its stream synchronizer holds the picture and the
/// sound of the new file until a subtitle stream starts there, which none
/// does (`GStreamer` 1.28.7; a next file with no subtitle stream is not
/// held). Left to decodebin3, it took the next file's subtitles while the
/// state said none.
struct Wanted {
    /// The audio track, -1 for the first.
    audio: i32,
    /// The subtitle track, -1 for none.
    subtitle: i32,
    /// Whether the input's collection was answered: only its first is.
    answered: bool,
    /// Whether the input is the next file handed to playbin: its answer
    /// holds back no frame, as the file playing goes on to its end, and
    /// tells the loop nothing.
    handed: bool,
}

impl Wanted {
    /// What a file opened is answered with: its first audio track and no
    /// subtitle.
    fn opened() -> Self {
        Self {
            audio: -1,
            subtitle: -1,
            answered: false,
            handed: false,
        }
    }

    /// What the next file handed to playbin is answered with: its first
    /// audio track, and its first subtitle where it has one (`Wanted`).
    fn next() -> Self {
        Self {
            audio: -1,
            subtitle: 0,
            answered: false,
            handed: true,
        }
    }
}

/// The next file handed to playbin (`about-to-finish`, on a streaming
/// thread) is a new input: its first collection is answered
/// (`Wanted::next`). Only tried, as `answer` does: a track change holding
/// it returns the pipeline to READY, which waits for this thread, and its
/// new preroll is answered for the tracks it asked.
fn hand_over(wanted: &Mutex<Wanted>) {
    let mut wanted = match wanted.try_lock() {
        Ok(wanted) => wanted,
        Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
        Err(TryLockError::WouldBlock) => return,
    };
    *wanted = Wanted::next();
}

/// Answers a preroll's first stream collection on the thread posting it
/// (`Wanted`): what is asked goes to the loop first, so it is there before
/// the streams said selected can be.
fn answer(
    pipeline: &gst::Element,
    collection: &gst::StreamCollection,
    wanted: &Mutex<Wanted>,
    events: &Events,
    video: bool,
) {
    // Taken by a track change returning to READY: this preroll is let go.
    let mut wanted = match wanted.try_lock() {
        Ok(wanted) => wanted,
        Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
        Err(TryLockError::WouldBlock) => return,
    };
    if wanted.answered {
        return;
    }
    wanted.answered = true;
    let streams: Vec<gst::Stream> = collection.iter().take(128).collect();
    let ids = chosen(&streams, video, wanted.audio.max(0), wanted.subtitle);
    // A next file's: the file playing goes on drawing its frames, and one
    // refused is left to decodebin3, which plays it still.
    let told = !wanted.handed;
    if told {
        let _ = events.tx.send(Event::Selecting(Some(ids.clone())));
    }
    // Keep playbin's audio/video/text flags intact; selection chooses streams.
    let asked: Vec<_> = ids.iter().map(gst::glib::GString::as_str).collect();
    if !pipeline.send_event(gst::event::SelectStreams::new(asked)) && told {
        let _ = events.tx.send(Event::Selecting(None));
    }
    // Held until sent: a track change waits for it.
    drop(wanted);
}

/// Whether a file of `collection` is a film, played as one (`video`, the
/// helper not playing sound alone): a stream of it is a picture's. A film
/// never goes on gaplessly to the next file: uridecodebin3, freeing the
/// old source as it switched from a film with a subtitle stream to one
/// with none, held a lock one of its threads waited for while it waited
/// for that thread (`GStreamer` 1.28.7), and the player stood still for
/// good. Refused, the film plays to its end and the player opens the next
/// then, with a gap; music, where gapless matters, goes on (what the next
/// file holds is not known before it is read). To be tried again on a
/// later `GStreamer` (docs/invariants/media.md).
fn plays_film(collection: &gst::StreamCollection, video: bool) -> bool {
    video
        && collection
            .iter()
            .take(128)
            .any(|stream| stream.stream_type().contains(gst::StreamType::VIDEO))
}

/// The tracks a next file plays, said in its state (`Wanted::next`): its
/// first audio track, if it has one, and its first subtitle, if it has
/// one; and whether that subtitle is to be taken away, the file before
/// showing none (a track change, once it has started).
fn next_tracks(state: &mut Playback) -> bool {
    let shown = state.subtitle >= 0;
    state.audio = if state.audio_tracks.is_empty() { -1 } else { 0 };
    state.subtitle = if state.subtitle_tracks.is_empty() {
        -1
    } else {
        0
    };
    !shown && state.subtitle == 0
}

/// The streams of `streams` a selection names: the video (none for audio
/// alone), the `audio`th audio track and the `subtitle`th subtitle track,
/// each counted among its kind; an index of -1, or past the last, names
/// none.
fn chosen(
    streams: &[gst::Stream],
    video: bool,
    audio: i32,
    subtitle: i32,
) -> Vec<gst::glib::GString> {
    [
        (gst::StreamType::VIDEO, if video { 0 } else { -1 }),
        (gst::StreamType::AUDIO, audio),
        (gst::StreamType::TEXT, subtitle),
    ]
    .into_iter()
    .filter_map(|(kind, index)| {
        streams
            .iter()
            .filter(|s| s.stream_type().contains(kind))
            .nth(usize::try_from(index).ok()?)
            .and_then(gst::Stream::stream_id)
    })
    .collect()
}

enum Event {
    /// Everything before it was posted by a preroll since abandoned
    /// (`Player::fence`).
    Fence,
    Request(Request),
    /// A bus message, delivered with the wake that announces it.
    Bus(gst::Message),
    /// The streams the bus's sync handler asked of the preroll (`answer`),
    /// or `None`: the pipeline refused what it asked.
    Selecting(Option<Vec<gst::glib::GString>>),
    /// The file to go on with was handed to playbin (`about-to-finish`).
    Handing(std::path::PathBuf),
    Wake,
    /// The stream's volume or mute moved where the sound server holds them
    /// (given back for the application as the stream starts, moved in its
    /// mixer): read back by the loop (`Player::follow_sound`).
    Sound,
    Closed,
}

/// The loop's inbox. Bus messages and requests are never dropped; sink wakes
/// carry no payload and coalesce to one outstanding message.
struct Events {
    tx: mpsc::Sender<Event>,
    wake_pending: AtomicBool,
}
impl Events {
    fn wake(&self) {
        if !self.wake_pending.swap(true, Ordering::AcqRel) {
            let _ = self.tx.send(Event::Wake);
        }
    }
}

/// What sits between the decoder and the sink, and the spectrum element in
/// it when there is one. `scaletempo` keeps the pitch when the speed changes:
/// without it 1.25× raises every voice by four semitones. Either element may
/// be missing from an installation; playback does without.
struct Filter {
    element: Option<gst::Element>,
    spectrum: Option<gst::Element>,
}

fn audio_filter(measure: bool) -> Filter {
    let mut chain: Vec<gst::Element> = Vec::new();
    // Loudness first, from the gain a track's tags carry (the element reads
    // the tag events itself, once the stream flows); the limiter keeps a
    // boosted quiet recording from clipping. Tracks without tags pass as is.
    let mode = std::env::var("SKWIR_MEDIA_REPLAYGAIN").unwrap_or_default();
    if matches!(mode.as_str(), "track" | "album")
        && let (Ok(volume), Ok(limiter)) = (
            gst::ElementFactory::make("rgvolume")
                .property("album-mode", mode == "album")
                .build(),
            gst::ElementFactory::make("rglimiter").build(),
        )
    {
        chain.extend([volume, limiter]);
    }
    chain.extend(gst::ElementFactory::make("scaletempo").build().ok());
    let spectrum = measure
        .then(|| {
            gst::ElementFactory::make("spectrum")
                // Many linear bins, folded into a few logarithmic bands
                // (`media::log_bands`): music is heard in octaves.
                .property("bands", skwir_core::media::SPECTRUM_BINS)
                .property("interval", 50_000_000u64)
                .property("threshold", -80i32)
                .property("post-messages", false)
                .build()
                .ok()
        })
        .flatten();
    chain.extend(spectrum.clone());
    let filter = match chain.as_slice() {
        [] => None,
        [only] => Some(only.clone()),
        [first, .., last] => {
            let bin = gst::Bin::new();
            let linked = bin.add_many(&chain).is_ok()
                && gst::Element::link_many(&chain).is_ok()
                && ghost(&bin, first, "sink")
                && ghost(&bin, last, "src");
            linked.then(|| bin.upcast::<gst::Element>())
        }
    };
    let spectrum = spectrum.filter(|_| filter.is_some());
    Filter {
        element: filter,
        spectrum,
    }
}

fn ghost(bin: &gst::Bin, element: &gst::Element, pad: &str) -> bool {
    element
        .static_pad(pad)
        .and_then(|target| gst::GhostPad::with_target(&target).ok())
        .is_some_and(|ghost| bin.add_pad(&ghost).is_ok())
}

/// Where the frames are taken from: packed BGRA, square pixels, at most
/// `MAX_SIDE` a side either way (playsink scales a larger film down), one
/// at a time, kept to the clock.
fn frame_sink() -> anyhow::Result<AppSink> {
    let side = gst::IntRange::<i32>::new(1, i32::try_from(MAX_SIDE)?);
    Ok(AppSink::builder()
        .caps(
            &gst::Caps::builder("video/x-raw")
                .field("format", "BGRA")
                .field("width", side)
                .field("height", side)
                .field("pixel-aspect-ratio", gst::Fraction::new(1, 1))
                .build(),
        )
        .max_buffers(1)
        .drop(true)
        .sync(true)
        // A preview may skip video frames to preserve the audio clock.
        // Twenty milliseconds is the sink's lateness budget, not a timer.
        .qos(true)
        .max_lateness(20_000_000)
        .enable_last_sample(false)
        .build())
}

/// What playbin draws a film into: `sink`, behind a turn that follows the
/// orientation the stream says (`image-orientation`, which a phone's
/// display matrix becomes: a quarter turn, a half, a mirror, all eight), so
/// every frame handed over is upright, at the size it is shown at, as the
/// poster and the layout have it (`ffprobe`, turned). A tag that comes or
/// changes mid-stream turns the frames after it, a new stream without one
/// does not (videoflip's `video-direction=auto`).
///
/// It turns what playsink made: scaled and converted, BGRA at most
/// `MAX_SIDE` a side, whatever memory the decoder gave (a hardware
/// decoder's frames are in system memory by then), and a film not turned
/// passes untouched. Playbin's `video-filter` would turn before the scale, behind
/// a converter playsink always adds there: every 10-bit film a hardware
/// decoder gives (P010, which videoflip does not take) would be converted
/// at its own size, turned or not. Subtitles are drawn on the picture as
/// stored, before either: a turned film's turn with it.
///
/// Without good's videofilter plugin (an installation that lacks it) the
/// film plays as it is stored; the packages that carry their `GStreamer`
/// carry it.
fn upright(sink: &AppSink) -> anyhow::Result<gst::Element> {
    let Ok(turn) = gst::ElementFactory::make("videoflip")
        .property_from_str("video-direction", "auto")
        .build()
    else {
        return Ok(sink.clone().upcast());
    };
    let bin = gst::Bin::with_name("upright");
    bin.add_many([&turn, sink.upcast_ref()])?;
    turn.link(sink)?;
    anyhow::ensure!(ghost(&bin, &turn, "sink"), "the turn's input");
    Ok(bin.upcast())
}

/// Sample rate the spectrum is measured at, from its negotiated caps.
fn spectrum_rate(spectrum: &gst::Element) -> Option<u32> {
    let caps = spectrum.static_pad("sink")?.current_caps()?;
    let rate = caps.structure(0)?.get::<i32>("rate").ok()?;
    u32::try_from(rate).ok()
}

/// Whether `GStreamer` can play here: it starts, and has the playback
/// plugins (playbin3, appsink), a package of their own on most systems.
fn playable() -> Result<(), String> {
    gst::init().map_err(|error| error.to_string())?;
    if gst::ElementFactory::find("playbin3").is_none()
        || gst::ElementFactory::find("appsink").is_none()
    {
        return Err("Install GStreamer base playback plugins".into());
    }
    Ok(())
}

fn run() -> anyhow::Result<()> {
    if let Err(reason) = playable() {
        reply(&Reply::Unavailable(reason))?;
        return Ok(());
    }
    let mut args = std::env::args_os().skip(1);
    let path = std::path::PathBuf::from(args.next().context("missing video")?);
    let scratch = std::path::PathBuf::from(args.next().context("missing frame slot")?);
    let mut pixels = OpenOptions::new()
        .write(true)
        .open(scratch.join(PIXELS_FILE))?;
    let (tx, rx) = mpsc::channel();
    let events = Arc::new(Events {
        tx,
        wake_pending: AtomicBool::new(false),
    });
    let input_tx = events.tx.clone();
    std::thread::spawn(move || {
        let mut input = std::io::stdin().lock();
        while let Some(Ok(request)) = skwir_core::jsonl::read::<Request>(&mut input, MAX_MESSAGE) {
            if input_tx.send(Event::Request(request)).is_err() {
                break;
            }
        }
        let _ = input_tx.send(Event::Closed);
    });
    let mut player = Player::new(&path, &events)?;
    let mut occupied = false;
    let mut ready = false;
    let mut dirty = true;
    let mut last_status = Instant::now();
    loop {
        // A paused or starting player sleeps until decoder/input events arrive.
        // Only the playing clock needs timed status reports. Startup has no
        // deadline here: the manager owns it and kills the process.
        let event =
            if player.state.playing() || player.restore.is_some() || player.selection.is_some() {
                rx.recv_timeout(Duration::from_millis(250))
            } else {
                rx.recv().map_err(|_| mpsc::RecvTimeoutError::Disconnected)
            };
        match event {
            Ok(Event::Request(Request::Consumed)) => occupied = false,
            Ok(Event::Request(Request::Next { path })) => {
                *player.next.lock().unwrap_or_else(PoisonError::into_inner) = path;
            }
            Ok(Event::Handing(path)) => reply(&Reply::Handing { path })?,
            Ok(Event::Request(Request::Control {
                generation,
                command,
            })) => {
                player.generation = generation;
                if let Err(error) = player.control(command) {
                    // Not fatal: the session continues and the following
                    // State reply restores what the player actually did.
                    reply(&Reply::Rejected {
                        generation,
                        message: error.to_string(),
                    })?;
                }
                dirty = true;
            }
            Ok(Event::Closed) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                // The parent owns storage and process cleanup. Native teardown
                // can block on a decoder that is still changing state; process
                // exit closes audio/device handles without waiting on it.
                std::process::exit(0);
            }
            Ok(Event::Fence) => player.fences -= 1,
            // Posted before the latest return to READY: about a preroll that
            // no longer exists.
            Ok(Event::Bus(_) | Event::Selecting(_)) if player.fences > 0 => {}
            Ok(Event::Bus(message)) => {
                handle_message(&mut player, &message, &mut ready, &mut dirty)?;
            }
            Ok(Event::Selecting(asked)) => {
                player.selection = Some(asked.context("track selection failed")?);
            }
            Ok(Event::Wake) => events.wake_pending.store(false, Ordering::Release),
            Ok(Event::Sound) => dirty |= player.follow_sound(),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        if ready {
            player.refresh();
        }
        // A video preroll can arrive before the audio branch finishes seeking.
        // Publish it only once the whole pipeline has completed the seek.
        if sends_frame(
            player.frames,
            ready && !occupied,
            player.selection.is_none() && player.restore.is_none() && !player.seeking,
        ) {
            let sample = if player.state.playing() {
                player.sink.try_pull_sample(gst::ClockTime::ZERO)
            } else {
                player.sink.try_pull_preroll(gst::ClockTime::ZERO)
            };
            if let Some(sample) = sample {
                let info = VideoInfo::from_caps(sample.caps().context("video caps")?)?;
                skwir_core::media::frame_bytes(info.width(), info.height())?;
                let frame = VideoFrameRef::from_buffer_ref_readable(
                    sample.buffer().context("video buffer")?,
                    &info,
                )?;
                let stride = usize::try_from(frame.plane_stride()[0])?;
                let row = info.width() as usize * 4;
                let data = frame.plane_data(0)?;
                pixels.seek(SeekFrom::Start(0))?;
                if stride == row {
                    pixels.write_all(&data[..row * info.height() as usize])?;
                } else {
                    for y in 0..info.height() as usize {
                        pixels.write_all(&data[y * stride..y * stride + row])?;
                    }
                }
                reply(&Reply::Frame {
                    generation: player.generation,
                    width: info.width(),
                    height: info.height(),
                    seconds: sample
                        .buffer()
                        .and_then(gst::BufferRef::pts)
                        .map(|t| t.nseconds() as f64 / 1e9),
                    clock: player
                        .pipeline
                        .query_position::<gst::ClockTime>()
                        .map(|t| t.nseconds() as f64 / 1e9),
                    dropped: {
                        let late = player.sink.find_property("stats").and_then(|_| {
                            player
                                .sink
                                .property::<gst::Structure>("stats")
                                .get::<u64>("dropped")
                                .ok()
                        });
                        let queued = player
                            .sink
                            .find_property("dropped")
                            .map(|_| player.sink.property::<u64>("dropped"));
                        late.or(queued)
                            .map(|_| late.unwrap_or(0) + queued.unwrap_or(0))
                    },
                })?;
                occupied = true;
            }
        }
        if ready
            && (dirty
                || (player.state.playing() && last_status.elapsed() >= Duration::from_millis(250)))
        {
            reply(&Reply::State {
                generation: player.generation,
                playback: player.state.clone(),
            })?;
            dirty = false;
            last_status = Instant::now();
        }
    }
}

/// Whether the loop sends a frame now: while frames are asked for
/// (`Control::Frames`), the slot free once ready, and the pipeline settled
/// (no track selection, restore or seek on its way). Not sent, a frame
/// stays in the sink, which drops it for the next.
fn sends_frame(frames: bool, free: bool, settled: bool) -> bool {
    frames && free && settled
}

/// Applies one bus message. Errors end the session; everything else updates
/// the published state.
fn handle_message(
    player: &mut Player,
    message: &gst::Message,
    ready: &mut bool,
    dirty: &mut bool,
) -> anyhow::Result<()> {
    match message.view() {
        gst::MessageView::Error(error) => {
            anyhow::bail!("Playback failed: {}", error.error())
        }
        gst::MessageView::Eos(_) => {
            player.pipeline.set_state(gst::State::Paused)?;
            player.state.status = PlaybackStatus::Ended;
            *dirty = true;
        }
        // The queued file's stream has started: what plays has a new name,
        // a fresh position, and tags of its own to come.
        gst::MessageView::StreamStart(_) => {
            let handed = player
                .handed
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .take();
            if let Some(path) = handed {
                player.state.position = 0.;
                player.state.title.clear();
                player.state.artist.clear();
                player.state.album.clear();
                player.state.details.clear();
                // Its tracks, as they were asked of it (`hand_over`).
                let hidden = next_tracks(&mut player.state);
                reply(&Reply::Advanced { path })?;
                *dirty = true;
                // Subtitles off before, off now; failing, the file plays on
                // with them, as the state says.
                if hidden && let Err(error) = player.switch_tracks(player.state.audio, -1) {
                    reply(&Reply::Rejected {
                        generation: player.generation,
                        message: error.to_string(),
                    })?;
                }
            }
        }
        gst::MessageView::StreamsSelected(message) => {
            let actual: Vec<_> = message.streams().filter_map(|s| s.stream_id()).collect();
            if player.selection.as_ref().is_some_and(|wanted| {
                wanted.len() == actual.len() && wanted.iter().all(|id| actual.contains(id))
            }) {
                player.selection = None;
                *dirty = true;
            }
        }
        gst::MessageView::Tag(message) => {
            let tags = message.tags();
            if let Some(v) = tags.get::<gst::tags::Title>() {
                player.state.title = v.get().chars().take(512).collect();
            }
            if let Some(v) = tags.get::<gst::tags::Artist>() {
                player.state.artist = v.get().chars().take(512).collect();
            }
            if let Some(v) = tags.get::<gst::tags::Album>() {
                player.state.album = v.get().chars().take(512).collect();
            }
            // The rest of what the stream says, for the panel's Properties.
            let mut note = |label: &str, value: Option<String>| {
                let Some(value) = value.filter(|value| !value.trim().is_empty()) else {
                    return;
                };
                let value: String = value.trim().chars().take(128).collect();
                let details = &mut player.state.details;
                match details.iter().position(|(known, _)| known == label) {
                    Some(index) => details[index].1 = value,
                    None if details.len() < 16 => details.push((label.to_owned(), value)),
                    None => {}
                }
            };
            note(
                "Track",
                tags.get::<gst::tags::TrackNumber>().map(|n| {
                    match tags.get::<gst::tags::TrackCount>() {
                        Some(count) => format!("{} of {}", n.get(), count.get()),
                        None => n.get().to_string(),
                    }
                }),
            );
            note(
                "Year",
                tags.get::<gst::tags::DateTime>()
                    .map(|date| date.get().year().to_string()),
            );
            note(
                "Genre",
                tags.get::<gst::tags::Genre>().map(|v| v.get().to_owned()),
            );
            note(
                "Album artist",
                tags.get::<gst::tags::AlbumArtist>()
                    .map(|v| v.get().to_owned()),
            );
            note(
                "Composer",
                tags.get::<gst::tags::Composer>()
                    .map(|v| v.get().to_owned()),
            );
            note(
                "Codec",
                tags.get::<gst::tags::AudioCodec>()
                    .map(|v| v.get().to_owned()),
            );
            note(
                "Bitrate",
                tags.get::<gst::tags::Bitrate>()
                    .or_else(|| tags.get::<gst::tags::NominalBitrate>())
                    .map(|v| format!("{} kbit/s", v.get() / 1000)),
            );
            if let Some(caps) = player
                .spectrum
                .as_ref()
                .and_then(|spectrum| spectrum.static_pad("sink")?.current_caps())
                && let Some(format) = caps.structure(0)
            {
                note(
                    "Sample rate",
                    format
                        .get::<i32>("rate")
                        .ok()
                        .map(|rate| format!("{rate} Hz")),
                );
                note(
                    "Channels",
                    format.get::<i32>("channels").ok().map(|n| match n {
                        1 => "Mono".to_owned(),
                        2 => "Stereo".to_owned(),
                        n => n.to_string(),
                    }),
                );
            }
            *dirty = true;
        }
        gst::MessageView::Element(message) => {
            if player.state.playing()
                && let Some(s) = message.structure()
                && s.name() == "spectrum"
                && let Ok(values) = s.get::<gst::List>("magnitude")
            {
                let bins: Vec<f32> = values
                    .iter()
                    .take(skwir_core::media::SPECTRUM_BINS as usize)
                    .filter_map(|v| v.get::<f32>().ok())
                    .collect();
                let rate = player
                    .spectrum
                    .as_ref()
                    .and_then(spectrum_rate)
                    .unwrap_or(44_100);
                player.state.spectrum = skwir_core::media::log_bands(&bins, rate);
                *dirty = true;
            }
        }
        gst::MessageView::StreamCollection(message) => {
            player.streams = message.stream_collection().iter().take(128).collect();
            player.state.audio_only = player.audio_only
                || (!player
                    .streams
                    .iter()
                    .any(|s| s.stream_type().contains(gst::StreamType::VIDEO))
                    && player
                        .streams
                        .iter()
                        .any(|s| s.stream_type().contains(gst::StreamType::AUDIO)));
            // Ambiguous containers (e.g. ASF) become audio previews only
            // after stream discovery. Video pipelines retain no FFT filter.
            if player.state.audio_only
                && player.spectrum.is_none()
                && let Filter {
                    element: Some(filter),
                    spectrum: Some(spectrum),
                } = audio_filter(true)
            {
                player.pipeline.set_property("audio-filter", &filter);
                player.spectrum = Some(spectrum);
            }
            if let Some(filter) = &player.spectrum {
                filter.set_property("post-messages", player.state.audio_only);
            }
            player.state.audio_tracks = player.tracks(gst::StreamType::AUDIO);
            player.state.subtitle_tracks = player.tracks(gst::StreamType::TEXT);
            // The first, as the bus's sync handler asked (`Wanted`); none
            // past the last (a next file's tracks told after it started).
            if usize::try_from(player.state.audio)
                .is_ok_and(|audio| audio >= player.state.audio_tracks.len())
            {
                player.state.audio = -1;
            }
            if usize::try_from(player.state.subtitle)
                .is_ok_and(|subtitle| subtitle >= player.state.subtitle_tracks.len())
            {
                player.state.subtitle = -1;
            }
            if player.state.audio == -1 && !player.state.audio_tracks.is_empty() {
                player.state.audio = 0;
            }
            *dirty = true;
        }
        gst::MessageView::AsyncDone(_) => {
            if player.restore.is_some() {
                // The re-preroll's own: anything older was fenced off.
                let position = player.restore.take().unwrap_or_default();
                player.refresh();
                // A non-seekable stream restarts from its beginning after a
                // track change; failing to restore must not end the session.
                if player.state.seekable
                    && let Err(error) = player.seek(position, player.state.rate)
                {
                    reply(&Reply::Rejected {
                        generation: player.generation,
                        message: error.to_string(),
                    })?;
                }
                if player.state.playing() {
                    player.pipeline.set_state(gst::State::Playing)?;
                }
            }
            if !*ready {
                reply(&Reply::Ready)?;
                *ready = true;
            }
            // The stream there, its volume is the one the server gave it
            // (restored for the application, or moved meanwhile): pulsesink
            // says a change only while it renders, and a volume restored as
            // the stream began not at all. Read, it asks the server.
            player.follow_sound();
            *dirty = true;
        }
        _ => {}
    }
    Ok(())
}
/// The `GStreamer` carried beside this helper (its folder, links followed:
/// `skwir_core::folders::program`), where every package that carries one
/// puts it, is the one it plays with, apart from any the user
/// installed: its plugins alone (none from `GST_PLUGIN_PATH`), its own
/// plugin scanner, and its registry in skwir's cache rather than the one an
/// installed `GStreamer` keeps, which each would write again for its own
/// plugins at every turn. On Windows (ADR-076) the helper sits among its
/// libraries (`gstreamer\bin`), which its scanner, a program of its own,
/// finds on `PATH`; on macOS it is in the bundle's `MacOS`, `GStreamer` a
/// framework in its `Frameworks`; in the `AppImage` and the Linux tarball
/// (ADR-052) the plugins and the scanner are beside it. Linux's other
/// packages use the system's, and find none beside it.
///
/// Set here, for this helper and the scanner it starts alone: what skwir
/// starts (a converter, the terminal) never sees the carried plugins.
///
/// Setting an environment variable is unsafe in edition 2024 because other
/// threads may be reading the environment; this runs first thing in `main`,
/// before any thread exists, which is the one situation where it is sound.
#[allow(unsafe_code)]
fn carried_gstreamer() {
    let Some(here) = skwir_core::folders::program() else {
        return;
    };
    let Some((plugins, scanner)) = carried_beside(here) else {
        return;
    };
    let mut vars: Vec<(&str, std::ffi::OsString)> = vec![
        ("GST_PLUGIN_SYSTEM_PATH_1_0", plugins.into()),
        ("GST_PLUGIN_PATH_1_0", std::ffi::OsString::new()),
        ("GST_PLUGIN_SCANNER_1_0", scanner.into()),
    ];
    if let Some(cache) = skwir_core::folders::cache().map(|cache| cache.join("gstreamer")) {
        let _ = std::fs::create_dir_all(&cache);
        vars.push((
            "GST_REGISTRY_1_0",
            cache
                .join(format!("registry-{}.bin", std::env::consts::ARCH))
                .into(),
        ));
    }
    #[cfg(windows)]
    {
        let path = std::env::var_os("PATH").unwrap_or_default();
        let paths = std::iter::once(here.to_path_buf()).chain(std::env::split_paths(&path));
        if let Ok(joined) = std::env::join_paths(paths) {
            vars.push(("PATH", joined));
        }
    }
    for (name, value) in vars {
        // SAFETY: called first in `main`, before any thread is spawned.
        unsafe { std::env::set_var(name, value) };
    }
}

/// The plugin folder and the scanner of the `GStreamer` a package carries
/// beside the helper in `here`, where it has one.
fn carried_beside(here: &std::path::Path) -> Option<(std::path::PathBuf, std::path::PathBuf)> {
    #[cfg(windows)]
    let (plugins, scanner) = {
        let root = here.parent()?;
        (
            root.join("lib/gstreamer-1.0"),
            root.join("libexec/gstreamer-1.0/gst-plugin-scanner.exe"),
        )
    };
    #[cfg(target_os = "macos")]
    let (plugins, scanner) = {
        let root = here
            .parent()?
            .join("Frameworks/GStreamer.framework/Versions/1.0");
        (
            root.join("lib/gstreamer-1.0"),
            root.join("libexec/gstreamer-1.0/gst-plugin-scanner"),
        )
    };
    // The AppImage's and the tarball's `usr/lib/skwir`
    // (tools/package/appimage/stage.py).
    #[cfg(not(any(windows, target_os = "macos")))]
    let (plugins, scanner) = (here.join("gstreamer-1.0"), here.join("gst-plugin-scanner"));
    plugins.is_dir().then_some((plugins, scanner))
}

fn main() {
    // `--build`, skwir asking which build of it this helper is, when a
    // call of it failed (`platform::process::explain`).
    if std::env::args_os()
        .nth(1)
        .is_some_and(|arg| arg == "--build")
    {
        println!("{}", env!("SKWIR_BUILD"));
        return;
    }
    carried_gstreamer();
    // `--probe`, skwir's settings asking whether this helper works here
    // (`platform::process::probe`): GStreamer and its playback plugins.
    if std::env::args_os()
        .nth(1)
        .is_some_and(|arg| arg == "--probe")
    {
        if let Err(reason) = playable() {
            eprintln!("{reason}");
            std::process::exit(1);
        }
        return;
    }
    if let Err(error) = run() {
        let _ = reply(&Reply::Error(format!("{error:#}")));
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Event, Events, Playback, Wanted, answer, chosen, frame_sink, gst, hand_over, next_tracks,
        plays_film, sends_frame, upright,
    };
    use gstreamer::prelude::*;
    use gstreamer_video::{VideoFrameExt, VideoFrameRef, VideoInfo};
    use std::sync::{Mutex, mpsc};

    /// The frames are turned as the stream says from the frame after its
    /// word, all eight ways, a word that changes mid-stream turning them
    /// anew; a new stream that says nothing is shown as stored. A picture
    /// of three by two whose pixels each say where they were, read back
    /// as rows of those places.
    #[test]
    fn frames_are_turned_as_the_stream_says() {
        gst::init().unwrap();
        let sink = frame_sink().unwrap();
        // Handed over at once: these frames are not played to a clock.
        sink.set_property("sync", false);
        let turned = upright(&sink).unwrap();
        let pipeline = gst::Pipeline::new();
        pipeline.add(&turned).unwrap();
        let src = gst::Pad::builder(gst::PadDirection::Src).build();
        src.link(&turned.static_pad("sink").unwrap()).unwrap();
        src.set_active(true).unwrap();
        pipeline.set_state(gst::State::Playing).unwrap();
        let start = |id: &str| {
            assert!(src.push_event(gst::event::StreamStart::new(id)));
            let caps = gst::Caps::builder("video/x-raw")
                .field("format", "BGRA")
                .field("width", 3)
                .field("height", 2)
                .field("pixel-aspect-ratio", gst::Fraction::new(1, 1))
                .field("framerate", gst::Fraction::new(0, 1))
                .build();
            assert!(src.push_event(gst::event::Caps::new(&caps)));
            let segment = gst::FormattedSegment::<gst::ClockTime>::new();
            assert!(src.push_event(gst::event::Segment::new(&segment)));
        };
        // Place i, in the blue of BGRA.
        let picture: Vec<u8> = (0..6u8).flat_map(|i| [i * 40, 0, 0, 255]).collect();
        let shown = |said: Option<&str>| -> Vec<Vec<u8>> {
            if let Some(said) = said {
                let mut tags = gst::TagList::new();
                tags.make_mut()
                    .add::<gst::tags::ImageOrientation>(&said, gst::TagMergeMode::Replace);
                tags.make_mut().set_scope(gst::TagScope::Stream);
                assert!(src.push_event(gst::event::Tag::new(tags)));
            }
            src.push(gst::Buffer::from_slice(picture.clone())).unwrap();
            let sample = sink
                .try_pull_sample(gst::ClockTime::from_seconds(10))
                .expect("a frame");
            let info = VideoInfo::from_caps(sample.caps().unwrap()).unwrap();
            let frame =
                VideoFrameRef::from_buffer_ref_readable(sample.buffer().unwrap(), &info).unwrap();
            let stride = usize::try_from(frame.plane_stride()[0]).unwrap();
            let data = frame.plane_data(0).unwrap();
            (0..info.height() as usize)
                .map(|y| {
                    (0..info.width() as usize)
                        .map(|x| data[y * stride + x * 4] / 40)
                        .collect()
                })
                .collect()
        };
        start("stored");
        assert_eq!(shown(None), [[0, 1, 2], [3, 4, 5]], "nothing said");
        for (said, rows) in [
            ("rotate-90", vec![vec![3, 0], vec![4, 1], vec![5, 2]]),
            ("rotate-180", vec![vec![5, 4, 3], vec![2, 1, 0]]),
            ("rotate-270", vec![vec![2, 5], vec![1, 4], vec![0, 3]]),
            ("flip-rotate-0", vec![vec![2, 1, 0], vec![5, 4, 3]]),
            ("flip-rotate-90", vec![vec![5, 2], vec![4, 1], vec![3, 0]]),
            ("flip-rotate-180", vec![vec![3, 4, 5], vec![0, 1, 2]]),
            ("flip-rotate-270", vec![vec![0, 3], vec![1, 4], vec![2, 5]]),
            ("rotate-0", vec![vec![0, 1, 2], vec![3, 4, 5]]),
            ("rotate-90", vec![vec![3, 0], vec![4, 1], vec![5, 2]]),
        ] {
            assert_eq!(shown(Some(said)), rows, "{said}");
        }
        assert_eq!(
            shown(None),
            [[3, 0], [4, 1], [5, 2]],
            "the word holds until another"
        );
        start("next");
        assert_eq!(
            shown(None),
            [[0, 1, 2], [3, 4, 5]],
            "a new stream that says nothing"
        );
        pipeline.set_state(gst::State::Null).unwrap();
    }

    /// A next file handed to playbin is answered again: its first audio
    /// track, not the one chosen for the file before, and its first
    /// subtitle, whether the file before showed one or not (playbin3 has
    /// asked a subtitle output of it already). Only tried: held by a track
    /// change, it is left as that change set it.
    #[test]
    fn a_next_file_is_answered_with_its_first_tracks() {
        for shown in [0, 1, -1] {
            let wanted = Mutex::new(Wanted {
                audio: 1,
                subtitle: shown,
                answered: true,
                handed: false,
            });
            hand_over(&wanted);
            let handed = wanted.lock().unwrap();
            assert!(!handed.answered && handed.handed);
            assert_eq!((handed.audio, handed.subtitle), (-1, 0));
        }
        let changing = Mutex::new(Wanted::opened());
        let held = changing.lock().unwrap();
        hand_over(&changing);
        assert!(!held.handed, "held by a track change, left as it set it");
    }

    /// The next file's answer tells the loop nothing, so the file playing
    /// goes on showing its frames to its end; a file opened's or a track
    /// change's is told, frames held until its streams are selected.
    #[test]
    fn a_next_files_answer_holds_back_no_frame() {
        gst::init().unwrap();
        let pipeline: gst::Element = gst::prelude::Cast::upcast(gst::Pipeline::new());
        let stream =
            |id: &str, kind| gst::Stream::new(Some(id), None, kind, gst::StreamFlags::empty());
        let collection = gst::StreamCollection::builder(None)
            .stream(stream("video", gst::StreamType::VIDEO))
            .stream(stream("english", gst::StreamType::AUDIO))
            .stream(stream("words", gst::StreamType::TEXT))
            .build();
        let (tx, rx) = mpsc::channel();
        let events = Events {
            tx,
            wake_pending: false.into(),
        };
        let next = Mutex::new(Wanted::next());
        answer(&pipeline, &collection, &next, &events, true);
        assert!(next.lock().unwrap().answered);
        assert!(
            rx.try_recv().is_err(),
            "a next file's answer is told nothing"
        );
        let opened = Mutex::new(Wanted::opened());
        answer(&pipeline, &collection, &opened, &events, true);
        assert!(matches!(
            rx.try_recv(),
            Ok(Event::Selecting(Some(ids))) if ids == ["video", "english"]
        ));
    }

    /// A film is a file with a picture's stream, played as one: it never
    /// goes on gaplessly; a file of sound alone, or any file the helper
    /// plays the sound of alone, does.
    #[test]
    fn a_film_is_told_by_a_picture_stream_played_as_one() {
        gst::init().unwrap();
        let stream =
            |id: &str, kind| gst::Stream::new(Some(id), None, kind, gst::StreamFlags::empty());
        let film = gst::StreamCollection::builder(None)
            .stream(stream("picture", gst::StreamType::VIDEO))
            .stream(stream("sound", gst::StreamType::AUDIO))
            .stream(stream("words", gst::StreamType::TEXT))
            .build();
        let tune = gst::StreamCollection::builder(None)
            .stream(stream("sound", gst::StreamType::AUDIO))
            .build();
        assert!(plays_film(&film, true));
        assert!(!plays_film(&film, false), "its sound alone played");
        assert!(!plays_film(&tune, true));
    }

    /// A next file's state says the tracks asked of it: its first audio
    /// track, none where it has none, and its first subtitle where it has
    /// one, to be taken away where the file before showed none.
    #[test]
    fn a_next_file_says_the_tracks_asked_of_it() {
        let mut state = Playback {
            audio: 1,
            subtitle: 0,
            audio_tracks: vec!["1: eng".into(), "2: fra".into()],
            subtitle_tracks: vec!["1: eng".into()],
            ..Playback::default()
        };
        assert!(!next_tracks(&mut state), "shown before, shown now");
        assert_eq!((state.audio, state.subtitle), (0, 0));
        state.subtitle = -1;
        assert!(next_tracks(&mut state), "none shown before: taken away");
        assert_eq!(state.subtitle, 0, "until then, what plays");
        state.subtitle = -1;
        state.subtitle_tracks.clear();
        state.audio_tracks.clear();
        assert!(!next_tracks(&mut state), "no subtitle to take away");
        assert_eq!((state.audio, state.subtitle), (-1, -1));
    }

    /// The `AppImage`'s and the tarball's helper finds the plugins and the
    /// scanner beside it; a helper with none beside it (a distribution's
    /// package, a build here) is left the system's.
    #[cfg(not(any(windows, target_os = "macos")))]
    #[test]
    fn a_gstreamer_beside_the_helper_is_the_one_it_plays_with() {
        let here = tempfile::tempdir().unwrap();
        assert_eq!(super::carried_beside(here.path()), None);
        std::fs::create_dir(here.path().join("gstreamer-1.0")).unwrap();
        assert_eq!(
            super::carried_beside(here.path()),
            Some((
                here.path().join("gstreamer-1.0"),
                here.path().join("gst-plugin-scanner")
            ))
        );
    }

    /// A selection names the video, the audio track and the subtitle
    /// asked, each counted among its kind; none of a kind for -1 or an
    /// index past its last.
    #[test]
    fn a_selection_names_each_kind_by_its_index() {
        gst::init().unwrap();
        let stream =
            |id: &str, kind| gst::Stream::new(Some(id), None, kind, gst::StreamFlags::empty());
        let streams = [
            stream("video", gst::StreamType::VIDEO),
            stream("english", gst::StreamType::AUDIO),
            stream("words", gst::StreamType::TEXT),
            stream("french", gst::StreamType::AUDIO),
        ];
        let ids = |video, audio, subtitle| -> Vec<String> {
            chosen(&streams, video, audio, subtitle)
                .iter()
                .map(ToString::to_string)
                .collect()
        };
        assert_eq!(ids(true, 1, -1), ["video", "french"]);
        assert_eq!(ids(true, 0, 0), ["video", "english", "words"]);
        assert_eq!(ids(false, 0, -1), ["english"], "audio alone");
        assert_eq!(ids(true, 2, 1), ["video"], "past the last of each");
    }

    /// No frame while the window is not shown; shown, one while the slot
    /// is free and the pipeline settled.
    #[test]
    fn frames_are_sent_only_while_asked_for() {
        assert!(sends_frame(true, true, true));
        assert!(!sends_frame(false, true, true), "the window not shown");
        assert!(!sends_frame(true, false, true), "the slot taken");
        assert!(!sends_frame(true, true, false), "a seek on its way");
    }
}
