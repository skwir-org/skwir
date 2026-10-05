//! `GStreamer` is linked only here. The pipeline owns A/V timing; appsink drops
//! late frames. A single acknowledged RAM-backed slot carries packed BGRA.

use anyhow::Context;
use bibli_core::media::{
    Control, MAX_MESSAGE, MAX_SIDE, PIXELS_FILE, Playback, PlaybackStatus, Reply, Request,
};
use gstreamer::{self as gst, prelude::*};
use gstreamer_app::AppSink;
use gstreamer_video::{VideoFrameExt, VideoFrameRef, VideoInfo};
use std::{
    fs::OpenOptions,
    io::{Seek, SeekFrom, Write},
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

fn reply(message: &Reply) -> anyhow::Result<()> {
    let mut out = std::io::stdout().lock();
    let line = bibli_core::jsonl::encode(message)?;
    anyhow::ensure!(line.len() <= MAX_MESSAGE, "video message exceeds limit");
    out.write_all(&line)?;
    out.flush()?;
    Ok(())
}

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
    selection: Option<Vec<gst::glib::GString>>,
    selected: bool,
    restore: Option<f64>,
    seeking: bool,
    /// The loop's inbox, for the fences a track change sends.
    events: Arc<Events>,
    /// Fences sent and not yet reached: while there is one, what the loop
    /// takes from the bus was posted before the latest READY (see `fence`).
    fences: usize,
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
        let sink = AppSink::builder()
            .caps(
                &gst::Caps::builder("video/x-raw")
                    .field("format", "BGRA")
                    .field(
                        "width",
                        gst::IntRange::<i32>::new(1, i32::try_from(MAX_SIDE)?),
                    )
                    .field(
                        "height",
                        gst::IntRange::<i32>::new(1, i32::try_from(MAX_SIDE)?),
                    )
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
            .build();
        let uri = gst::glib::filename_to_uri(path, None)?;
        let pipeline = gst::ElementFactory::make("playbin3")
            .property("uri", uri.as_str())
            .property("video-sink", &sink)
            .build()?;
        let audio_only = std::env::var_os("BIBLI_MEDIA_AUDIO_ONLY").is_some();
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
        if std::env::var_os("BIBLI_MEDIA_TEST_AUDIO").is_some() {
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
        {
            let (next, handed) = (next.clone(), handed.clone());
            pipeline.connect("about-to-finish", false, move |values| {
                let path = next.lock().unwrap_or_else(PoisonError::into_inner).take()?;
                if let (Ok(pipeline), Ok(uri)) = (
                    values[0].get::<gst::Element>(),
                    gst::glib::filename_to_uri(&path, None),
                ) {
                    pipeline.set_property("uri", uri.as_str());
                    *handed.lock().unwrap_or_else(PoisonError::into_inner) = Some(path);
                }
                None
            });
        }
        // The sync handler runs on the posting thread and hands the message
        // itself to the loop: wake and payload are one object, so the loop can
        // never drain an empty queue and sleep while a message is pending.
        let bus = pipeline.bus().context("pipeline bus")?;
        let bus_events = events.clone();
        bus.set_sync_handler(move |_, message| {
            let _ = bus_events.tx.send(Event::Bus(message.clone()));
            gst::BusSyncReply::Drop
        });
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
            selected: false,
            restore: None,
            seeking: false,
            events: Arc::clone(events),
            fences: 0,
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
                    .set_property("volume", bibli_core::media::amplitude(volume));
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
        self.pipeline.set_state(gst::State::Ready)?;
        self.fence();
        self.state.audio = audio;
        self.state.subtitle = subtitle;
        self.state.seekable = false;
        self.selected = false;
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
    fn select_tracks(&mut self, audio: i32, subtitle: i32) -> anyhow::Result<()> {
        let mut selected = Vec::new();
        for (kind, index) in [
            (gst::StreamType::VIDEO, if self.audio_only { -1 } else { 0 }),
            (gst::StreamType::AUDIO, audio),
            (gst::StreamType::TEXT, subtitle),
        ] {
            if let Ok(index) = usize::try_from(index)
                && let Some(id) = self
                    .streams
                    .iter()
                    .filter(|s| s.stream_type().contains(kind))
                    .nth(index)
                    .and_then(gst::Stream::stream_id)
            {
                selected.push(id);
            }
        }
        // Keep playbin's audio/video/text flags intact; selection chooses streams.
        self.selection = Some(selected.clone());
        let ids: Vec<_> = selected.iter().map(gst::glib::GString::as_str).collect();
        anyhow::ensure!(
            self.pipeline
                .send_event(gst::event::SelectStreams::new(ids)),
            "track selection failed"
        );
        self.state.audio = audio;
        self.state.subtitle = subtitle;
        Ok(())
    }
}

enum Event {
    /// Everything before it was posted by a preroll since abandoned
    /// (`Player::fence`).
    Fence,
    Request(Request),
    /// A bus message, delivered with the wake that announces it.
    Bus(gst::Message),
    Wake,
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
    let mode = std::env::var("BIBLI_MEDIA_REPLAYGAIN").unwrap_or_default();
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
                .property("bands", bibli_core::media::SPECTRUM_BINS)
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

/// Sample rate the spectrum is measured at, from its negotiated caps.
fn spectrum_rate(spectrum: &gst::Element) -> Option<u32> {
    let caps = spectrum.static_pad("sink")?.current_caps()?;
    let rate = caps.structure(0)?.get::<i32>("rate").ok()?;
    u32::try_from(rate).ok()
}

fn run() -> anyhow::Result<()> {
    if let Err(error) = gst::init() {
        reply(&Reply::Unavailable(error.to_string()))?;
        return Ok(());
    }
    if gst::ElementFactory::find("playbin3").is_none()
        || gst::ElementFactory::find("appsink").is_none()
    {
        reply(&Reply::Unavailable(
            "Install GStreamer base playback plugins".into(),
        ))?;
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
        while let Some(Ok(request)) = bibli_core::jsonl::read::<Request>(&mut input, MAX_MESSAGE) {
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
            Ok(Event::Bus(_)) if player.fences > 0 => {}
            Ok(Event::Bus(message)) => {
                handle_message(&mut player, &message, &mut ready, &mut dirty)?;
            }
            Ok(Event::Wake) => events.wake_pending.store(false, Ordering::Release),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        if ready {
            player.refresh();
        }
        // A video preroll can arrive before the audio branch finishes seeking.
        // Publish it only once the whole pipeline has completed the seek.
        if ready
            && !occupied
            && player.selection.is_none()
            && player.restore.is_none()
            && !player.seeking
        {
            let sample = if player.state.playing() {
                player.sink.try_pull_sample(gst::ClockTime::ZERO)
            } else {
                player.sink.try_pull_preroll(gst::ClockTime::ZERO)
            };
            if let Some(sample) = sample {
                let info = VideoInfo::from_caps(sample.caps().context("video caps")?)?;
                bibli_core::media::frame_bytes(info.width(), info.height())?;
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
            if let Some(path) = player
                .handed
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .take()
            {
                player.state.position = 0.;
                player.state.title.clear();
                player.state.artist.clear();
                player.state.album.clear();
                player.state.details.clear();
                reply(&Reply::Advanced { path })?;
                *dirty = true;
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
            // The rest of what the stream says, for the panel's Details.
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
                    .take(bibli_core::media::SPECTRUM_BINS as usize)
                    .filter_map(|v| v.get::<f32>().ok())
                    .collect();
                let rate = player
                    .spectrum
                    .as_ref()
                    .and_then(spectrum_rate)
                    .unwrap_or(44_100);
                player.state.spectrum = bibli_core::media::log_bands(&bins, rate);
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
            if player.state.audio == -1 && !player.state.audio_tracks.is_empty() {
                player.state.audio = 0;
            }
            if !player.selected {
                player.selected = true;
                player.select_tracks(player.state.audio, player.state.subtitle)?;
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
            *dirty = true;
        }
        _ => {}
    }
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        let _ = reply(&Reply::Error(format!("{error:#}")));
    }
}
