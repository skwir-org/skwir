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
    io::{BufRead, Read, Seek, SeekFrom, Write},
    sync::mpsc,
    time::{Duration, Instant},
};

fn reply(message: &Reply) -> anyhow::Result<()> {
    let mut out = std::io::stdout().lock();
    let bytes = serde_json::to_vec(message)?;
    anyhow::ensure!(bytes.len() < MAX_MESSAGE, "video message exceeds limit");
    out.write_all(&bytes)?;
    out.write_all(b"\n")?;
    out.flush()?;
    Ok(())
}

struct Player {
    pipeline: gst::Element,
    sink: AppSink,
    state: Playback,
    generation: u64,
    streams: Vec<gst::Stream>,
    selection: Option<Vec<gst::glib::GString>>,
    selected: bool,
    restore: Option<f64>,
    seeking: bool,
}
impl Drop for Player {
    fn drop(&mut self) {
        let _ = self.pipeline.set_state(gst::State::Null);
    }
}
impl Player {
    fn new(path: &std::path::Path) -> anyhow::Result<Self> {
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
        let player = Self {
            pipeline,
            sink,
            state: Playback::default(),
            generation: 0,
            streams: Vec::new(),
            selection: None,
            selected: false,
            restore: None,
            seeking: false,
        };
        player.pipeline.set_state(gst::State::Paused)?;
        Ok(player)
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
                self.pipeline.set_property("volume", volume);
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
        if self.seeking
            && self.pipeline.state(gst::ClockTime::ZERO).0 == Ok(gst::StateChangeSuccess::Success)
        {
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
        if let Some(bus) = self.pipeline.bus() {
            bus.set_flushing(true);
            bus.set_flushing(false);
        }
        self.state.audio = audio;
        self.state.subtitle = subtitle;
        self.state.seekable = false;
        self.selected = false;
        self.selection = None;
        self.restore = Some(position);
        self.pipeline.set_state(gst::State::Paused)?;
        Ok(())
    }
    fn select_tracks(&mut self, audio: i32, subtitle: i32) -> anyhow::Result<()> {
        let mut selected = Vec::new();
        for (kind, index) in [
            (gst::StreamType::VIDEO, 0),
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
    Request(Request),
    Wake,
    Closed,
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
    let (tx, rx) = mpsc::sync_channel(32);
    let input_tx = tx.clone();
    std::thread::spawn(move || {
        let mut input = std::io::stdin().lock();
        loop {
            let mut line = Vec::new();
            if Read::by_ref(&mut input)
                .take((MAX_MESSAGE + 1) as u64)
                .read_until(b'\n', &mut line)
                .ok()
                == Some(0)
            {
                break;
            }
            if line.len() > MAX_MESSAGE {
                break;
            }
            let Ok(request) = serde_json::from_slice::<Request>(&line) else {
                break;
            };
            if input_tx.send(Event::Request(request)).is_err() {
                break;
            }
        }
        let _ = input_tx.send(Event::Closed);
    });
    let mut player = Player::new(&path)?;
    let bus = player.pipeline.bus().context("pipeline bus")?;
    let bus_tx = tx.clone();
    bus.set_sync_handler(move |_, _| {
        let _ = bus_tx.try_send(Event::Wake);
        gst::BusSyncReply::Pass
    });
    let sample_tx = tx.clone();
    player.sink.set_callbacks(
        gstreamer_app::AppSinkCallbacks::builder()
            .new_sample(move |_| {
                let _ = sample_tx.try_send(Event::Wake);
                Ok(gst::FlowSuccess::Ok)
            })
            .new_preroll(move |_| {
                let _ = tx.try_send(Event::Wake);
                Ok(gst::FlowSuccess::Ok)
            })
            .build(),
    );
    let mut occupied = false;
    let mut ready = false;
    let mut dirty = true;
    let mut last_status = Instant::now();
    let started = Instant::now();
    loop {
        // A paused, initialized player sleeps until decoder/input events arrive.
        // Only the playing clock and startup need timed progress checks.
        let event = if ready
            && !player.state.playing()
            && player.restore.is_none()
            && player.selection.is_none()
        {
            rx.recv().map_err(|_| mpsc::RecvTimeoutError::Disconnected)
        } else {
            rx.recv_timeout(Duration::from_millis(250))
        };
        match event {
            Ok(Event::Request(Request::Consumed)) => occupied = false,
            Ok(Event::Request(Request::Control {
                generation,
                command,
            })) => {
                player.generation = generation;
                if let Err(error) = player.control(command) {
                    reply(&Reply::Error(error.to_string()))?;
                }
                dirty = true;
            }
            Ok(Event::Closed) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                // The parent owns storage and process cleanup. Native teardown
                // can block on a decoder that is still changing state; process
                // exit closes audio/device handles without waiting on it.
                std::process::exit(0);
            }
            Ok(Event::Wake) | Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        while let Some(message) = bus.pop() {
            match message.view() {
                gst::MessageView::Error(error) => {
                    anyhow::bail!("Playback failed: {}", error.error())
                }
                gst::MessageView::Eos(_) => {
                    player.pipeline.set_state(gst::State::Paused)?;
                    player.state.status = PlaybackStatus::Ended;
                    dirty = true;
                }
                gst::MessageView::StreamsSelected(message) => {
                    let actual: Vec<_> = message.streams().filter_map(|s| s.stream_id()).collect();
                    if player.selection.as_ref().is_some_and(|wanted| {
                        wanted.len() == actual.len() && wanted.iter().all(|id| actual.contains(id))
                    }) {
                        player.selection = None;
                        dirty = true;
                    }
                }
                gst::MessageView::StreamCollection(message) => {
                    player.streams = message.stream_collection().iter().take(128).collect();
                    player.state.audio_tracks = player.tracks(gst::StreamType::AUDIO);
                    player.state.subtitle_tracks = player.tracks(gst::StreamType::TEXT);
                    if player.state.audio == -1 && !player.state.audio_tracks.is_empty() {
                        player.state.audio = 0;
                    }
                    if !player.selected {
                        player.selected = true;
                        player.select_tracks(player.state.audio, player.state.subtitle)?;
                    }
                    dirty = true;
                }
                gst::MessageView::AsyncDone(_) => {
                    if let Some(position) = player.restore.take() {
                        player.refresh();
                        player.seek(position, player.state.rate)?;
                        if player.state.playing() {
                            player.pipeline.set_state(gst::State::Playing)?;
                        }
                    }
                    if !ready {
                        reply(&Reply::Ready)?;
                        ready = true;
                    }
                    dirty = true;
                }
                _ => {}
            }
        }
        anyhow::ensure!(
            ready || started.elapsed() < Duration::from_secs(15),
            "video startup timed out"
        );
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
fn main() {
    if let Err(error) = run() {
        let _ = reply(&Reply::Error(error.to_string()));
    }
}
