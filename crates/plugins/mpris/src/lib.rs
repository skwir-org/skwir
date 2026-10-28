//! skwir plugin: the desktop's media controls (MPRIS, on the session bus).
//!
//! It mirrors core's `App::player` and nothing else: what plays becomes the
//! properties of `org.mpris.MediaPlayer2.Player`, and the calls the desktop
//! makes (media keys, the panel's widget, `playerctl`, a headset's buttons)
//! become `PlayerRequest`s, which the player takes when it next wakes. The
//! plugin knows neither the UI nor the player, and they do not know it.
//!
//! A file manager is not always a music player: the bus name is held only
//! while something plays, so the desktop shows no idle "skwir" player.
//! Everything runs on the plugin's own thread, on the program's one
//! session connection (`skwir_platform::bus`), what it serves there taken
//! back when it ends; without a session bus (a headless run) it logs once
//! and ends.
use std::collections::HashMap;
use std::sync::{Arc, Weak};

use parking_lot::Mutex;
use skwir_core::media::PlaybackStatus;
use skwir_core::playing::{NowPlaying, PlayerRequest};
use skwir_core::{App, CancelToken, plugin::Plugin};
use zbus::object_server::SignalEmitter;
use zbus::zvariant::{ObjectPath, OwnedValue, Value};

const PATH: &str = "/org/mpris/MediaPlayer2";
const NO_TRACK: &str = "/org/mpris/MediaPlayer2/TrackList/NoTrack";

#[derive(Default)]
pub struct MprisPlugin {
    running: Mutex<Option<(CancelToken, std::thread::JoinHandle<()>)>>,
}

impl Plugin for MprisPlugin {
    fn id(&self) -> &'static str {
        "mpris"
    }
    fn title(&self) -> &'static str {
        "Media controls"
    }
    fn description(&self) -> &'static str {
        "Let the desktop show and steer what is playing: media keys, the panel's player, headset buttons (MPRIS)"
    }
    fn category(&self) -> skwir_core::plugin::Category {
        skwir_core::plugin::Category::Desktop
    }
    fn register(&self, app: &App) {
        let stop = app.lifetime().child();
        let (weak, changes, token) = (app.weak(), app.player.subscribe(self.id()), stop.clone());
        let thread = std::thread::Builder::new()
            .name("skwir-mpris".into())
            .spawn(move || {
                if let Err(error) = serve(&weak, &changes, &token) {
                    tracing::debug!("media controls are unavailable: {error:#}");
                }
            });
        if let Ok(thread) = thread {
            *self.running.lock() = Some((stop, thread));
        }
    }
    fn on_disable(&self, _: &App) {
        if let Some((stop, thread)) = self.running.lock().take() {
            stop.cancel();
            let _ = thread.join();
        }
    }
}

fn request(app: &Weak<App>, request: PlayerRequest) {
    if let Some(app) = app.upgrade()
        && app.player.request(request)
    {
        app.wake();
    }
}

fn now(app: &Weak<App>) -> Option<Arc<NowPlaying>> {
    app.upgrade()?.player.get()
}

struct Root(Weak<App>);

// The bus macro wants every method and property on `&self`, constant or not.
#[allow(clippy::unused_self)]
#[zbus::interface(name = "org.mpris.MediaPlayer2")]
impl Root {
    fn raise(&self) {
        request(&self.0, PlayerRequest::Raise);
    }
    // Quitting a file manager from a media widget would be a surprise.
    fn quit(&self) {}
    #[zbus(property)]
    fn can_quit(&self) -> bool {
        false
    }
    #[zbus(property)]
    fn can_raise(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn has_track_list(&self) -> bool {
        false
    }
    #[zbus(property)]
    fn identity(&self) -> &'static str {
        "skwir"
    }
    /// The desktop entry's name, without `.desktop`: the application id.
    #[zbus(property)]
    fn desktop_entry(&self) -> &'static str {
        skwir_core::APP_ID
    }
    #[zbus(property)]
    fn supported_uri_schemes(&self) -> Vec<&'static str> {
        vec!["file"]
    }
    #[zbus(property)]
    fn supported_mime_types(&self) -> Vec<&'static str> {
        vec![
            "audio/mpeg",
            "audio/flac",
            "audio/ogg",
            "audio/mp4",
            "audio/x-wav",
        ]
    }
}

struct Player(Weak<App>);

/// MPRIS counts in microseconds.
fn micros(seconds: f64) -> i64 {
    (seconds.max(0.) * 1e6) as i64
}

/// A track's identity on the bus: an object path, so only letters and digits.
fn track_id(now: &NowPlaying) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    now.path.hash(&mut hasher);
    format!("/org/skwir/track/{:016x}", hasher.finish())
}

fn metadata(now: Option<&NowPlaying>) -> HashMap<String, OwnedValue> {
    let mut map = HashMap::new();
    let mut put = |key: &str, value: Value<'_>| {
        if let Ok(value) = value.try_to_owned() {
            map.insert(key.to_owned(), value);
        }
    };
    let Some(now) = now else {
        if let Ok(path) = ObjectPath::try_from(NO_TRACK) {
            put("mpris:trackid", path.into());
        }
        return map;
    };
    if let Ok(path) = ObjectPath::try_from(track_id(now)) {
        put("mpris:trackid", path.into());
    }
    put("xesam:title", now.title.as_str().into());
    put("xesam:url", skwir_platform::uri::file_uri(&now.path).into());
    if !now.artist.is_empty() {
        put("xesam:artist", vec![now.artist.as_str()].into());
    }
    if !now.album.is_empty() {
        put("xesam:album", now.album.as_str().into());
    }
    if let Some(duration) = now.duration {
        put("mpris:length", micros(duration).into());
    }
    if let Some(cover) = &now.cover {
        put("mpris:artUrl", skwir_platform::uri::file_uri(cover).into());
    }
    map
}

// The bus macro wants every method and property on `&self`, constant or not,
// and arguments by value, as they arrive.
#[allow(clippy::unused_self, clippy::needless_pass_by_value)]
#[zbus::interface(name = "org.mpris.MediaPlayer2.Player")]
impl Player {
    fn next(&self) {
        request(&self.0, PlayerRequest::Next);
    }
    fn previous(&self) {
        request(&self.0, PlayerRequest::Previous);
    }
    fn pause(&self) {
        request(&self.0, PlayerRequest::Pause);
    }
    fn play_pause(&self) {
        request(&self.0, PlayerRequest::PlayPause);
    }
    fn stop(&self) {
        request(&self.0, PlayerRequest::Stop);
    }
    fn play(&self) {
        request(&self.0, PlayerRequest::Play);
    }
    fn seek(&self, offset: i64) {
        request(&self.0, PlayerRequest::SeekBy(offset as f64 / 1e6));
    }
    fn set_position(&self, track: ObjectPath<'_>, position: i64) {
        // A position meant for a track that is gone must not move this one.
        if now(&self.0).is_some_and(|now| track_id(&now) == track.as_str()) {
            request(&self.0, PlayerRequest::SeekTo(position as f64 / 1e6));
        }
    }
    // Opening what the desktop hands us is not a preview's business.
    fn open_uri(&self, uri: &str) {
        tracing::debug!(length = uri.len(), "ignored a request to open a location");
    }

    #[zbus(signal)]
    async fn seeked(emitter: &SignalEmitter<'_>, position: i64) -> zbus::Result<()>;

    #[zbus(property)]
    fn playback_status(&self) -> &'static str {
        match now(&self.0).map(|now| now.status) {
            Some(PlaybackStatus::Playing) => "Playing",
            Some(PlaybackStatus::Paused) => "Paused",
            Some(PlaybackStatus::Ended) | None => "Stopped",
        }
    }
    #[zbus(property)]
    fn rate(&self) -> f64 {
        now(&self.0).map_or(1., |now| now.rate)
    }
    #[zbus(property)]
    fn set_rate(&self, rate: f64) {
        // The spec: a rate of 0 is a pause; out of range, it is ignored.
        if rate == 0. {
            request(&self.0, PlayerRequest::Pause);
        } else if skwir_core::media::RATES.contains(&rate) {
            request(&self.0, PlayerRequest::Rate(rate));
        }
    }
    // Rates are seeks: a stream that cannot seek plays at 1× alone, and
    // says so, so the desktop offers no speed it would not take.
    #[zbus(property)]
    fn minimum_rate(&self) -> f64 {
        rates(now(&self.0).as_deref()).0
    }
    #[zbus(property)]
    fn maximum_rate(&self) -> f64 {
        rates(now(&self.0).as_deref()).1
    }
    #[zbus(property)]
    fn metadata(&self) -> HashMap<String, OwnedValue> {
        metadata(now(&self.0).as_deref())
    }
    #[zbus(property)]
    fn volume(&self) -> f64 {
        now(&self.0).map_or(1., |now| now.volume)
    }
    #[zbus(property)]
    fn set_volume(&self, volume: f64) {
        request(&self.0, PlayerRequest::Volume(volume.clamp(0., 1.)));
    }
    #[zbus(property(emits_changed_signal = "false"))]
    fn position(&self) -> i64 {
        now(&self.0).map_or(0, |now| micros(now.position_now()))
    }
    #[zbus(property)]
    fn can_go_next(&self) -> bool {
        now(&self.0).is_some_and(|now| now.can_next)
    }
    #[zbus(property)]
    fn can_go_previous(&self) -> bool {
        now(&self.0).is_some_and(|now| now.can_previous)
    }
    #[zbus(property)]
    fn can_play(&self) -> bool {
        now(&self.0).is_some()
    }
    #[zbus(property)]
    fn can_pause(&self) -> bool {
        now(&self.0).is_some()
    }
    #[zbus(property)]
    fn can_seek(&self) -> bool {
        now(&self.0).is_some_and(|now| now.can_seek)
    }
    #[zbus(property)]
    fn can_control(&self) -> bool {
        true
    }
}

/// The slowest and fastest rates `now` plays at: those of
/// `media::RATES` while it can seek, else 1× (or nothing playing).
fn rates(now: Option<&NowPlaying>) -> (f64, f64) {
    if now.is_some_and(|now| now.can_seek) {
        (
            *skwir_core::media::RATES.start(),
            *skwir_core::media::RATES.end(),
        )
    } else {
        (1., 1.)
    }
}

/// What is served on the session's connection, which the program shares
/// (`bus::session`): taken back when the plugin ends, the name too.
struct Served {
    connection: zbus::blocking::Connection,
    name: String,
    named: bool,
}

impl Drop for Served {
    fn drop(&mut self) {
        let server = self.connection.object_server();
        let _ = server.remove::<Player, _>(PATH);
        let _ = server.remove::<Root, _>(PATH);
        if self.named {
            let _ = self.connection.release_name(self.name.as_str());
        }
    }
}

/// Hold the bus name while something plays, and tell the bus what changed.
fn serve(
    app: &Weak<App>,
    changes: &std::sync::mpsc::Receiver<()>,
    stop: &CancelToken,
) -> anyhow::Result<()> {
    #[cfg(target_os = "linux")]
    let connection = skwir_platform::bus::session()?;
    #[cfg(not(target_os = "linux"))]
    let connection = zbus::blocking::Connection::session()?;
    // One name per process: two windows of two instances are two players.
    let mut served = Served {
        connection,
        name: format!(
            "org.mpris.MediaPlayer2.skwir.instance{}",
            std::process::id()
        ),
        named: false,
    };
    let objects = served.connection.object_server();
    objects.at(PATH, Root(app.clone()))?;
    objects.at(PATH, Player(app.clone()))?;
    let player = objects.interface::<_, Player>(PATH)?;
    let mut last: Option<Arc<NowPlaying>> = None;
    // What plays as it starts (turned on again while a song plays), then
    // asleep until something changes: the channel ends when the plugin is
    // turned off (its listener is dropped before `on_disable` waits for
    // this thread) or the app goes. A timer woke it four times a second,
    // forever.
    let mut starting = true;
    while std::mem::take(&mut starting) || (changes.recv().is_ok() && !stop.is_cancelled()) {
        let Some(app) = app.upgrade() else { break };
        let current = app.player.get();
        drop(app);
        match (current.is_some(), served.named) {
            (true, false) => {
                served.named = served.connection.request_name(served.name.as_str()).is_ok();
            }
            (false, true) => {
                let _ = served.connection.release_name(served.name.as_str());
                served.named = false;
            }
            _ => {}
        }
        let jumped = match (&last, &current) {
            (Some(old), Some(new)) => {
                old.path == new.path && (new.position - old.position_now()).abs() > 1.5
            }
            _ => false,
        };
        let emitter = player.signal_emitter();
        zbus::block_on(async {
            let iface = player.get();
            // Cheap to say too much: listeners ask for what they show.
            let _ = iface.playback_status_changed(emitter).await;
            let _ = iface.metadata_changed(emitter).await;
            let _ = iface.volume_changed(emitter).await;
            let _ = iface.rate_changed(emitter).await;
            let _ = iface.minimum_rate_changed(emitter).await;
            let _ = iface.maximum_rate_changed(emitter).await;
            let _ = iface.can_go_next_changed(emitter).await;
            let _ = iface.can_go_previous_changed(emitter).await;
            let _ = iface.can_play_changed(emitter).await;
            let _ = iface.can_pause_changed(emitter).await;
            let _ = iface.can_seek_changed(emitter).await;
            if jumped && let Some(new) = &current {
                let _ = Player::seeked(emitter, micros(new.position)).await;
            }
        });
        last = current;
    }
    Ok(())
}

#[cfg(test)]
// Helpers of the tests that run on Linux alone (ADR-035).
#[cfg_attr(not(target_os = "linux"), allow(dead_code, unused_imports))]
mod tests {
    use super::*;

    fn track() -> NowPlaying {
        NowPlaying {
            path: "/music/it's a café/01 – song.flac".into(),
            title: "A Song".into(),
            artist: "Someone".into(),
            album: String::new(),
            status: PlaybackStatus::Playing,
            position: 12.5,
            at: std::time::Instant::now(),
            rate: 1.,
            duration: Some(200.25),
            volume: 1.,
            can_next: true,
            can_previous: false,
            can_seek: true,
            cover: None,
            pinned: true,
        }
    }

    #[test]
    fn a_stream_that_cannot_seek_offers_one_rate() {
        let seekable = track();
        assert_eq!(
            rates(Some(&seekable)),
            (
                *skwir_core::media::RATES.start(),
                *skwir_core::media::RATES.end()
            )
        );
        let live = NowPlaying {
            can_seek: false,
            ..track()
        };
        assert_eq!(rates(Some(&live)), (1., 1.));
        assert_eq!(rates(None), (1., 1.));
    }

    // MPRIS is the free desktops' (step I, ADR-035).
    #[cfg(target_os = "linux")]
    #[test]
    fn a_track_is_described_the_way_the_bus_expects() {
        let now = track();
        let map = metadata(Some(&now));
        let text = |key: &str| map.get(key).map(|value| format!("{:?}", **value));
        assert!(text("mpris:trackid").unwrap().contains("/org/skwir/track/"));
        assert!(text("xesam:title").unwrap().contains("A Song"));
        assert!(text("xesam:artist").unwrap().contains("Someone"));
        assert!(text("mpris:length").unwrap().contains("200250000"));
        assert!(!map.contains_key("xesam:album"), "a blank is not a value");
        // A path is a URL on the bus, escaped the way the desktop escapes it.
        assert_eq!(
            skwir_platform::uri::file_uri(&now.path),
            "file:///music/it's%20a%20caf%C3%A9/01%20%E2%80%93%20song.flac"
        );
        // The same file is the same track; nothing playing is no track.
        assert_eq!(track_id(&now), track_id(&track()));
        assert!(format!("{:?}", metadata(None)["mpris:trackid"]).contains("NoTrack"));
        assert_eq!(micros(1.5), 1_500_000);
        assert_eq!(micros(-3.), 0);
    }
}
