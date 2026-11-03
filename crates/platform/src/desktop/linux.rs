//! Linux: the desktop's servers on the program's session bus (`bus`), and
//! logind on the system's.

use std::collections::HashMap;
use std::os::fd::OwnedFd;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender};
use parking_lot::Mutex;
use skwir_core::CancelToken;
use zbus::blocking::Connection;
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

use super::icon::Icons;
use super::{
    Asked, Clicked, Kind, Notice, OnAsked, Presence, Progress, Setup, Shown, Standing, State,
};

mod tray;

/// The notification server: its name, object and interface.
const NOTICES: (&str, &str, &str) = (
    "org.freedesktop.Notifications",
    "/org/freedesktop/Notifications",
    "org.freedesktop.Notifications",
);
/// logind's manager on the system bus.
const LOGIN: (&str, &str, &str) = (
    "org.freedesktop.login1",
    "/org/freedesktop/login1",
    "org.freedesktop.login1.Manager",
);
/// The bus itself.
const DBUS: (&str, &str, &str) = (
    "org.freedesktop.DBus",
    "/org/freedesktop/DBus",
    "org.freedesktop.DBus",
);
/// The tray's watcher, which items register with and hosts read
/// (`StatusNotifierItem`, KDE's specification).
const WATCHER: (&str, &str, &str) = (
    "org.kde.StatusNotifierWatcher",
    "/StatusNotifierWatcher",
    "org.kde.StatusNotifierWatcher",
);
/// The desktop portal's inhibitor.
const PORTAL: (&str, &str, &str) = (
    "org.freedesktop.portal.Desktop",
    "/org/freedesktop/portal/desktop",
    "org.freedesktop.portal.Inhibit",
);
/// What a portal's call answers: closed, its inhibition ends.
const REQUEST: &str = "org.freedesktop.portal.Request";
/// The portal's inhibitor flags: the session's end, and sleep.
const LOGOUT: u32 = 1;
const SUSPEND: u32 = 4;
/// The session managers that show a dialog of their own at the session's
/// end, listing what the portal holds: logind's block is not asked where
/// one is. Where one is, sleep is held off through the portal too.
const SESSION_MANAGERS: [&str; 2] = ["org.gnome.SessionManager", "org.kde.ksmserver"];
const LAUNCHER_ENTRY: &str = "com.canonical.Unity.LauncherEntry";
/// The action a click on the notice itself invokes.
const DEFAULT: &str = "default";
const ACTIONS: [&str; 2] = [DEFAULT, "Show"];
/// The notice that stays's other action.
const STOP: &str = "stop";
const RESIDENT_ACTIONS: [&str; 4] = [DEFAULT, "Open Skwir", STOP, "Stop all"];
/// How long a server is waited for: the session's connection is
/// shared and has no timeout of its own, and one that hangs would
/// hold every later ask.
const ANSWER: Duration = Duration::from_secs(5);
/// How long the server's signals are waited to be listened to before
/// the first notice, so no click on it is missed.
const LISTENING: Duration = Duration::from_secs(2);
/// How long a tray that went is waited for before the presence becomes a
/// notice: a panel started again (waybar, plasmashell) is back within it.
const LOST: Duration = Duration::from_secs(3);

pub(super) type Connect = Arc<dyn Fn() -> zbus::Result<Connection> + Send + Sync>;

/// Where the session's and the system's connections come from.
#[derive(Clone)]
pub(super) struct Buses {
    /// The session's: the program's own (`bus::session`), as its
    /// notification server may tell its signals to the connection that
    /// showed the notice alone.
    pub(super) session: Connect,
    /// The system's, made for a call and closed after it.
    pub(super) system: Connect,
}

impl Buses {
    pub(super) fn the_programs() -> Buses {
        Buses {
            session: Arc::new(crate::bus::session),
            system: Arc::new(Connection::system),
        }
    }
}

/// The notices shown, as the server knows them: shared with the thread
/// that follows its signals.
#[derive(Default)]
pub(super) struct Showing {
    /// The notice shown one at a time: the server's id of it, and the key
    /// it was shown with.
    pub(super) shown: Option<(u32, u64)>,
    /// The token the server gave last, for the notice of that id.
    token: Option<(u32, String)>,
    /// The presence's notice that stays, while there is no tray: the
    /// server's id of it, until the user closes it.
    pub(super) resident: Option<u32>,
    /// The server that showed them, by its connection's name: what another
    /// says of an id is of another's notice (a server started again counts
    /// its ids anew, and some tell every program of every click).
    server: Option<String>,
    /// How many times the server went or came: what was shown before is
    /// gone with the server.
    pub(super) servers: u32,
}

impl Showing {
    fn is(&self, id: u32) -> bool {
        self.shown.is_some_and(|(shown, _)| shown == id)
    }

    /// The server went or came (or the bus did): what it showed is gone,
    /// and its ids name nothing of ours.
    fn forget(&mut self) {
        *self = Showing {
            servers: self.servers.wrapping_add(1),
            ..Showing::default()
        };
    }

    /// Whether `message` comes from the server that showed ours.
    fn said_by_server(&self, message: &zbus::Message) -> bool {
        let header = message.header();
        header
            .sender()
            .is_some_and(|sender| self.server.as_deref() == Some(sender.as_str()))
    }

    /// The token given for `id`, taken.
    fn token(&mut self, id: u32) -> Option<String> {
        self.token
            .take_if(|(of, _)| *of == id)
            .map(|(_, token)| token)
    }
}

/// What shows the presence.
enum Shows {
    Nothing,
    /// An item in the tray.
    Tray(tray::Served),
    /// The notice that stays.
    Notice,
}

/// The presence's notice that stays, as last shown.
#[derive(Default)]
struct Resident {
    /// What it said: shown again only when this changes, never for a
    /// percent, and not after the user closed it until then.
    said: Option<(String, String, Standing)>,
    /// The connection it was shown on, and how many times the server had
    /// gone or come then: a bus or a server started again shows it anew.
    on: (Option<String>, u32),
}

/// The machine kept awake.
enum Awake {
    /// By the portal, where a session manager is on the bus: its request,
    /// asked on the connection named (its inhibition ends with it), and
    /// why, to ask again on a bus started again.
    Portal {
        why: String,
        on: Option<String>,
        request: Option<OwnedObjectPath>,
    },
    /// By logind's descriptor.
    Logind(OwnedFd),
}

/// The session's end held off.
struct SessionHold {
    why: String,
    /// The connection the portal was asked on: its inhibition ends with it.
    on: Option<String>,
    /// The portal's request, closed to let go.
    portal: Option<OwnedObjectPath>,
    /// logind's descriptor.
    logind: Option<OwnedFd>,
}

pub(super) struct Backend {
    app_id: String,
    app_name: String,
    asked: OnAsked,
    buses: Buses,
    showing: Arc<Mutex<Showing>>,
    /// The program's entry, as the launcher's signal names it.
    entry: String,
    /// The object the launcher's signal is said from.
    path: String,
    /// The thread following the bus's signals, once something was shown
    /// or held, until this ends.
    following: Option<CancelToken>,
    /// Told by that thread when the tray came or went, or the bus started
    /// again: this looks again ([`Backend::woken`]). This keeps a sender,
    /// so the receiver is never disconnected.
    wake: (Sender<()>, Receiver<()>),
    /// What keeps the machine awake, while it is.
    awake: Option<Awake>,
    /// Whether the last progress could not be told: said once, until
    /// one can.
    untold: bool,
    /// Whether the server reads markup, as it said when asked on the
    /// connection and with the server named: asked once each.
    markup: Option<((Option<String>, u32), bool)>,
    icons: Icons,
    /// The presence asked last.
    wanted: Option<Presence>,
    shows: Shows,
    /// Tray items served so far, naming the next.
    items: u32,
    /// When the presence becomes a notice if no tray came back by then.
    lost: Option<Instant>,
    resident: Resident,
    session: Option<SessionHold>,
    /// Portal requests asked so far, naming the next.
    requests: u32,
}

/// The progress as the launcher shows it: whole percents, none for an
/// error (it has no such state); paused shown as it was.
pub(super) fn shown(progress: Option<Progress>) -> Option<Shown> {
    progress
        .filter(|progress| progress.state != State::Error)
        .map(|progress| Shown {
            permille: super::permille(progress.fraction, 10),
            state: State::Normal,
        })
}

impl Backend {
    pub(super) fn new(setup: &Setup) -> Backend {
        let path = setup
            .app_id
            .split('.')
            .filter(|part| !part.is_empty())
            .map(|part| {
                part.chars()
                    .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
                    .collect::<String>()
            })
            .fold(String::new(), |path, part| path + "/" + &part);
        Backend {
            app_id: setup.app_id.clone(),
            app_name: setup.app_name.clone(),
            asked: Arc::clone(&setup.asked),
            buses: setup.buses.clone(),
            showing: Arc::clone(&setup.showing),
            entry: format!("application://{}.desktop", setup.app_id),
            path: path + "/LauncherEntry",
            following: None,
            wake: crossbeam_channel::unbounded(),
            awake: None,
            untold: false,
            markup: None,
            icons: Icons::new(setup.icon),
            wanted: None,
            shows: Shows::Nothing,
            items: 0,
            lost: None,
            resident: Resident::default(),
            session: None,
            requests: 0,
        }
    }

    /// What tells this to look again: the tray or the bus changed.
    pub(super) fn wakes(&self) -> Receiver<()> {
        self.wake.1.clone()
    }

    /// When this wants [`Backend::tick`]: a tray gone, waited for.
    pub(super) fn due(&self) -> Option<Instant> {
        self.lost
    }

    /// The tray waited for did not come back in time: look again, once.
    /// The deadline is taken first, so a look that cannot settle it (no
    /// bus) never comes due again: the module's thread would wake at once,
    /// for ever.
    pub(super) fn tick(&mut self, now: Instant) {
        if self.lost.take_if(|lost| now >= *lost).is_some() {
            self.settle(true);
        }
    }

    /// The tray or the bus changed: look again, once for all told.
    pub(super) fn woken(&mut self) {
        while self.wake.1.try_recv().is_ok() {}
        self.settle(false);
    }

    pub(super) fn notify(&mut self, key: u64, notice: &Notice) {
        let bus = match (self.buses.session)() {
            Ok(bus) => bus,
            Err(error) => {
                tracing::debug!("the notice was not shown: no session bus: {error}");
                return;
            }
        };
        self.follow();
        let replaces = self.showing.lock().shown.map_or(0, |(id, _)| id);
        let category = match notice.kind {
            Kind::Finished => "transfer.complete",
            Kind::Stopped => "transfer.error",
            Kind::Waiting => "transfer",
        };
        let hints = HashMap::from([
            ("category", Value::from(category)),
            ("desktop-entry", Value::from(self.app_id.clone())),
            ("urgency", Value::U8(1)),
        ]);
        let shown = self.show_notice(
            &bus,
            replaces,
            (&notice.title, &notice.body),
            &ACTIONS,
            hints,
            -1,
        );
        match shown {
            Ok(id) => self.showing.lock().shown = Some((id, key)),
            Err(error) => tracing::debug!("the notice was not shown: {error}"),
        }
    }

    /// Show a notice in place of `replaces` (0 for none): its id. The
    /// server that answers is the one whose signals are ours.
    fn show_notice(
        &mut self,
        bus: &Connection,
        replaces: u32,
        (summary, body): (&str, &str),
        actions: &[&str],
        hints: HashMap<&str, Value<'_>>,
        timeout: i32,
    ) -> zbus::Result<u32> {
        // Escaped only for a server that reads markup: another shows
        // the body as written.
        let body = if self.reads_markup(bus) {
            super::escape(body)
        } else {
            body.to_owned()
        };
        let reply = call(
            bus,
            NOTICES,
            "Notify",
            &(
                self.app_name.as_str(),
                replaces,
                self.app_id.as_str(),
                summary,
                body.as_str(),
                actions,
                hints,
                timeout,
            ),
        )?;
        let id = reply.body().deserialize::<u32>()?;
        let server = reply.header().sender().map(ToString::to_string);
        let mut showing = self.showing.lock();
        if showing.server != server {
            // Another server than the last: its ids are not the last's.
            showing.shown = None;
            showing.resident = None;
            showing.server = server;
        }
        Ok(id)
    }

    /// Whether the server reads markup in a notice's body, asked once on
    /// each connection and of each server.
    fn reads_markup(&mut self, bus: &Connection) -> bool {
        let of = (unique(bus), self.showing.lock().servers);
        if let Some((asked, markup)) = &self.markup
            && *asked == of
        {
            return *markup;
        }
        let markup = call(bus, NOTICES, "GetCapabilities", &())
            .and_then(|reply| reply.body().deserialize::<Vec<String>>())
            .is_ok_and(|capabilities| capabilities.iter().any(|c| c == "body-markup"));
        self.markup = Some((of, markup));
        markup
    }

    pub(super) fn withdraw(&mut self, key: Option<u64>) {
        let id = {
            let mut showing = self.showing.lock();
            match showing.shown {
                Some((id, shown)) if key.is_none_or(|key| key == shown) => {
                    showing.shown = None;
                    id
                }
                _ => return,
            }
        };
        let closed =
            (self.buses.session)().and_then(|bus| call(&bus, NOTICES, "CloseNotification", &id));
        if let Err(error) = closed {
            tracing::debug!("the notice was not taken back: {error}");
        }
    }

    pub(super) fn progress(&mut self, shown: Option<Shown>) {
        let progress = shown.map_or(0.0, |shown| f64::from(shown.permille) / 1000.0);
        let properties = HashMap::from([
            ("progress", Value::F64(progress)),
            ("progress-visible", Value::Bool(shown.is_some())),
        ]);
        let told = (self.buses.session)().and_then(|bus| {
            bus.emit_signal(
                None::<&str>,
                self.path.as_str(),
                LAUNCHER_ENTRY,
                "Update",
                &(self.entry.as_str(), properties),
            )
        });
        match told {
            Ok(()) => self.untold = false,
            Err(error) => {
                if !std::mem::replace(&mut self.untold, true) {
                    tracing::debug!("the progress is not shown on the program's entry: {error}");
                }
            }
        }
    }

    /// Hold off sleep: through the portal where a session manager is on
    /// the bus, else (or where the portal refuses) by logind's block, on a
    /// system connection made for the call, the descriptor it answers
    /// outliving the connection. GNOME's session turns the portal's into
    /// logind's `block-weak` and keeps its own idle from suspending, as
    /// KDE's `PowerDevil` does: the user's own Suspend still suspends, which
    /// logind refuses outright while a `block` is held (systemd 257 on).
    pub(super) fn hold(&mut self, why: &str) {
        let bus = (self.buses.session)().ok();
        if let Some(bus) = bus.filter(|bus| session_manager(bus).is_some()) {
            // To ask the portal again on a bus started again.
            self.follow();
            if let Some(request) = self.ask_portal(&bus, SUSPEND, why) {
                self.awake = Some(Awake::Portal {
                    why: why.to_owned(),
                    on: unique(&bus),
                    request: Some(request),
                });
                return;
            }
        }
        match self.inhibit("sleep", why) {
            Ok(fd) => self.awake = Some(Awake::Logind(fd)),
            Err(error) => tracing::debug!("the machine may sleep meanwhile: {error}"),
        }
    }

    pub(super) fn let_go(&mut self) {
        match self.awake.take() {
            Some(Awake::Portal {
                on,
                request: Some(request),
                ..
            }) => self.close_request(on.as_deref(), &request),
            Some(Awake::Portal { request: None, .. }) | None => {}
            Some(Awake::Logind(descriptor)) => drop(descriptor),
        }
    }

    /// logind's block of `what`, with `why`.
    fn inhibit(&self, what: &str, why: &str) -> zbus::Result<OwnedFd> {
        (self.buses.system)()
            .and_then(|bus| {
                call(
                    &bus,
                    LOGIN,
                    "Inhibit",
                    &(what, self.app_name.as_str(), why, "block"),
                )
            })
            .and_then(|reply| reply.body().deserialize::<zbus::zvariant::OwnedFd>())
            .map(OwnedFd::from)
    }

    /// Show `presence` where it shows (the tray, else a notice that stays),
    /// or take it back.
    pub(super) fn presence(&mut self, presence: Option<&Presence>) {
        self.wanted = presence.cloned();
        let Some(presence) = presence else {
            self.take_back();
            return;
        };
        let bus = match (self.buses.session)() {
            Ok(bus) => bus,
            Err(error) => {
                tracing::debug!("nothing shows the program at work: no session bus: {error}");
                return;
            }
        };
        self.follow();
        match &mut self.shows {
            Shows::Tray(served) if served.is_on(&bus) => served.show(presence, &mut self.icons),
            Shows::Notice => self.resident(&bus, presence),
            // What was served on a bus that ended went with it.
            Shows::Tray(_) | Shows::Nothing => {
                self.shows = Shows::Nothing;
                let tray = tray_host(&bus);
                self.show(&bus, tray, presence);
            }
        }
    }

    /// Show `presence` in the tray whose watcher `tray` names, else as a
    /// notice that stays: nothing showing it yet, so no tray is waited for.
    fn show(&mut self, bus: &Connection, tray: Option<String>, presence: &Presence) {
        self.lost = None;
        if let Some(watcher) = tray {
            self.items += 1;
            // Unique to the process and to the presence: a host that knew
            // the last one's name forgets it before taking this one.
            let name = format!(
                "org.kde.StatusNotifierItem-{}-{}",
                std::process::id(),
                self.items
            );
            let app = (self.app_id.as_str(), self.app_name.as_str());
            match tray::Served::on(bus, name, app, presence, &mut self.icons, &self.asked) {
                Ok(mut served) => {
                    if served.register(watcher) {
                        self.shows = Shows::Tray(served);
                        return;
                    }
                }
                Err(error) => tracing::debug!("the program is not shown in the tray: {error}"),
            }
        }
        self.shows = Shows::Notice;
        self.resident(bus, presence);
    }

    /// Look again at where the presence shows and what holds the session,
    /// the tray or the bus having changed: register again with a watcher
    /// started again, become a notice once the tray is gone for [`LOST`],
    /// go back to the tray once there is one, show everything anew on a
    /// bus started again. `expired`: the tray gone was waited for [`LOST`]
    /// (`tick`). `lost` is set only here, only while an item is served
    /// whose tray is gone: every other way out leaves it unset.
    fn settle(&mut self, expired: bool) {
        let bus = match (self.buses.session)() {
            Ok(bus) => bus,
            Err(error) => {
                tracing::debug!("no session bus: {error}");
                return;
            }
        };
        self.hold_again(&bus);
        let Some(wanted) = self.wanted.clone() else {
            self.lost = None;
            return;
        };
        let tray = tray_host(&bus);
        match &mut self.shows {
            Shows::Tray(served) if served.is_on(&bus) => {
                if let Some(watcher) = tray {
                    self.lost = None;
                    if served.watcher() != Some(watcher.as_str()) {
                        served.register(watcher);
                    }
                    return;
                }
                if !expired {
                    // Waited for from when it was first missed.
                    self.lost.get_or_insert_with(|| Instant::now() + LOST);
                    return;
                }
                tracing::debug!("no tray came back: the program at work is shown in a notice");
                self.lost = None;
                // The item taken back with it.
                self.shows = Shows::Notice;
                self.resident(&bus, &wanted);
            }
            Shows::Notice if tray.is_some() => {
                self.close_resident(&bus);
                self.shows = Shows::Nothing;
                self.show(&bus, tray, &wanted);
            }
            // Shown anew on a bus started again.
            Shows::Notice => {
                self.lost = None;
                self.resident(&bus, &wanted);
            }
            Shows::Tray(_) | Shows::Nothing => {
                self.shows = Shows::Nothing;
                self.show(&bus, tray, &wanted);
            }
        }
    }

    /// The presence as a notice that stays: shown again only when its
    /// title, what it is about or how it stands changed, never for a
    /// percent; once the user closed it, not until then.
    fn resident(&mut self, bus: &Connection, presence: &Presence) {
        let on = (unique(bus), self.showing.lock().servers);
        if self.resident.on != on {
            // Shown on a bus that ended, by a server gone, or never: gone
            // with it.
            self.resident = Resident { said: None, on };
            self.showing.lock().resident = None;
        }
        let said = (
            presence.title.clone(),
            presence.about.clone(),
            presence.standing,
        );
        if self.resident.said.as_ref() == Some(&said) {
            return;
        }
        let replaces = self.showing.lock().resident.unwrap_or(0);
        let hints = HashMap::from([
            ("category", Value::from("transfer")),
            ("desktop-entry", Value::from(self.app_id.clone())),
            ("resident", Value::Bool(true)),
            ("urgency", Value::U8(1)),
        ]);
        let shown = self.show_notice(
            bus,
            replaces,
            (&presence.title, &presence.about),
            &RESIDENT_ACTIONS,
            hints,
            0,
        );
        match shown {
            Ok(id) => self.showing.lock().resident = Some(id),
            // Not asked again until it says something new: a server that
            // hangs would hold every ask meanwhile, each second.
            Err(error) => tracing::debug!("nothing shows the program at work: {error}"),
        }
        self.resident.said = Some(said);
    }

    /// Close the notice that stays, if shown.
    fn close_resident(&mut self, bus: &Connection) {
        self.resident.said = None;
        let id = self.showing.lock().resident.take();
        if let Some(id) = id
            && let Err(error) = call(bus, NOTICES, "CloseNotification", &id)
        {
            tracing::debug!("the notice that stays was not taken back: {error}");
        }
    }

    /// Take back what shows the presence: the item's name and objects, or
    /// the notice that stays.
    fn take_back(&mut self) {
        self.lost = None;
        match std::mem::replace(&mut self.shows, Shows::Nothing) {
            Shows::Notice => match (self.buses.session)() {
                Ok(bus) => self.close_resident(&bus),
                Err(error) => tracing::debug!("the notice that stays stays: {error}"),
            },
            // Dropped: its name and objects taken back.
            Shows::Tray(served) => drop(served),
            Shows::Nothing => {}
        }
    }

    /// Hold off the session's end with `why` (in place of the last), or
    /// let go: the new hold taken before the last is let go of, so none
    /// lapses between.
    pub(super) fn hold_session(&mut self, why: Option<&str>) {
        let before = self.session.take();
        if let Some(why) = why {
            self.session = Some(self.hold_off(why));
        }
        if let Some(before) = before {
            self.let_session_go(before);
        }
    }

    /// The portal's inhibitor of the session's end, which GNOME's session
    /// lists in its log-out and power-off dialogs, with `why`; and logind's
    /// shutdown block, but only where no session manager showing a dialog
    /// of its own is on the bus. GNOME mishandles logind's block
    /// inhibitors: power-off silently does nothing, or polkit asks for a
    /// password without saying why
    /// (<https://discussion.fedoraproject.org/t/poweroff-reboot-doesnt-work-in-gnome-when-an-inhibitor-is-held/148592>).
    fn hold_off(&mut self, why: &str) -> SessionHold {
        let bus = (self.buses.session)()
            .inspect_err(|error| tracing::debug!("no session bus: {error}"))
            .ok();
        if bus.is_some() {
            // To ask the portal again on a bus started again.
            self.follow();
        }
        let portal = bus
            .as_ref()
            .and_then(|bus| self.ask_portal(bus, LOGOUT, why));
        let showing_its_own = bus.as_ref().and_then(session_manager);
        let logind = match showing_its_own {
            Some(manager) => {
                tracing::debug!("logind is not asked: {manager} shows the session's end itself");
                None
            }
            None => self
                .inhibit("shutdown", why)
                .inspect_err(|error| tracing::debug!("logind does not hold off shutdown: {error}"))
                .ok(),
        };
        if portal.is_none() && logind.is_none() {
            tracing::debug!("the session may end while actions run: nothing holds it off");
        }
        SessionHold {
            why: why.to_owned(),
            on: bus.as_ref().and_then(unique),
            portal,
            logind,
        }
    }

    /// The portal's inhibitor of what `flags` name (logging out, sleep),
    /// with `why`: its request.
    fn ask_portal(&mut self, bus: &Connection, flags: u32, why: &str) -> Option<OwnedObjectPath> {
        self.requests += 1;
        let token = format!("skwir_session_{}", self.requests);
        let options = HashMap::from([
            ("reason", Value::from(why)),
            ("handle_token", Value::from(token.as_str())),
        ]);
        call(bus, PORTAL, "Inhibit", &("", flags, options))
            .and_then(|reply| reply.body().deserialize::<OwnedObjectPath>())
            .inspect_err(|error| {
                tracing::debug!("the portal does not inhibit (flags {flags}): {error}");
            })
            .ok()
    }

    /// Ask the portal again on a bus started again: what it held went with
    /// the connection that asked.
    fn hold_again(&mut self, bus: &Connection) {
        let on = unique(bus);
        if let Some(why) = self
            .session
            .as_ref()
            .filter(|hold| hold.on != on)
            .map(|hold| hold.why.clone())
        {
            let portal = self.ask_portal(bus, LOGOUT, &why);
            if let Some(hold) = &mut self.session {
                hold.portal = portal;
                hold.on.clone_from(&on);
            }
        }
        if let Some(why) = match &self.awake {
            Some(Awake::Portal { why, on: asked, .. }) if *asked != on => Some(why.clone()),
            _ => None,
        } {
            let request = self.ask_portal(bus, SUSPEND, &why);
            self.awake = Some(Awake::Portal { why, on, request });
        }
    }

    /// logind's descriptor goes first: closing the portal's request waits
    /// for its answer, and once the portal has it the session is held by
    /// nothing of Skwir's.
    fn let_session_go(&self, hold: SessionHold) {
        drop(hold.logind);
        if let Some(request) = &hold.portal {
            self.close_request(hold.on.as_deref(), request);
        }
    }

    /// Close the portal's `request`, asked on the connection named `on`:
    /// on a bus that ended, it went with the connection.
    fn close_request(&self, on: Option<&str>, request: &OwnedObjectPath) {
        let closed = (self.buses.session)()
            .ok()
            .filter(|bus| unique(bus).as_deref() == on)
            .map(|bus| call(&bus, (PORTAL.0, request.as_str(), REQUEST), "Close", &()));
        if let Some(Err(error)) = closed {
            tracing::debug!("the portal's hold was not let go of: {error}");
        }
    }

    /// Follow the bus's signals on a thread of their own, for the
    /// program's life (`bus::keep`): started with the first notice,
    /// presence or hold, which waits until they are listened to. The
    /// server's are taken there; the tray's, and the bus started again,
    /// wake this.
    fn follow(&mut self) {
        if self.following.is_some() {
            return;
        }
        let stop = CancelToken::new();
        self.following = Some(stop.clone());
        let (ready, listening) = crossbeam_channel::bounded::<()>(1);
        let connect = Arc::clone(&self.buses.session);
        let showing = Arc::clone(&self.showing);
        let asked = Arc::clone(&self.asked);
        let wake = self.wake.0.clone();
        let started = std::thread::Builder::new()
            .name("skwir-notices".into())
            .spawn(move || {
                let rules = match rules() {
                    Ok(rules) => rules,
                    Err(error) => {
                        tracing::debug!("the desktop's signals are not followed: {error}");
                        return;
                    }
                };
                let mut again = false;
                crate::bus::keep(
                    || connect(),
                    &stop,
                    |bus| {
                        if std::mem::replace(&mut again, true) {
                            // The bus started again, and its servers with
                            // it: what was shown went with them, and what
                            // was served and held is asked anew.
                            showing.lock().forget();
                            let _ = wake.send(());
                        }
                        let ready = ready.clone();
                        crate::bus::listen(
                            bus,
                            rules.clone(),
                            &stop,
                            move || {
                                let _ = ready.try_send(());
                            },
                            |message| told(message, &showing, &asked, &wake),
                        );
                    },
                );
            });
        match started {
            Ok(_) => {
                let _ = listening.recv_timeout(LISTENING);
            }
            Err(error) => tracing::debug!("the desktop's signals are not followed: {error}"),
        }
    }
}

impl Drop for Backend {
    fn drop(&mut self) {
        // Taken back by the module's thread before; anything left with it.
        self.take_back();
        if let Some(hold) = self.session.take() {
            self.let_session_go(hold);
        }
        self.let_go();
        if let Some(following) = self.following.take() {
            following.cancel();
        }
    }
}

/// The signals followed: the notification server's (a notice clicked,
/// the token given with the click, a notice closed) and its owner changing,
/// the tray's watcher's owner changing, and a host coming to or leaving the
/// watcher.
fn rules() -> zbus::Result<Vec<zbus::MatchRule<'static>>> {
    let signal = || zbus::MatchRule::builder().msg_type(zbus::message::Type::Signal);
    let host = |member| -> zbus::Result<zbus::MatchRule<'static>> {
        Ok(signal()
            .interface(WATCHER.2)?
            .path(WATCHER.1)?
            .member(member)?
            .build())
    };
    let owner = |name| -> zbus::Result<zbus::MatchRule<'static>> {
        Ok(signal()
            .sender(DBUS.0)?
            .interface(DBUS.2)?
            .member("NameOwnerChanged")?
            .arg(0, name)?
            .build())
    };
    Ok(vec![
        signal().interface(NOTICES.2)?.path(NOTICES.1)?.build(),
        owner(NOTICES.0)?,
        owner(WATCHER.0)?,
        host("StatusNotifierHostRegistered")?,
        host("StatusNotifierHostUnregistered")?,
    ])
}

/// A signal followed, taken at once: the notification server's handled
/// here, the server gone or come forgotten; the tray's, and the server's
/// change, told to the module's thread, which asks the bus (the notice that
/// stays shown anew).
fn told(message: &zbus::Message, showing: &Mutex<Showing>, asked: &OnAsked, wake: &Sender<()>) {
    let header = message.header();
    let (Some(interface), Some(member)) = (header.interface(), header.member()) else {
        return;
    };
    if interface.as_str() == NOTICES.2 {
        noticed(member.as_str(), message, showing, asked);
        return;
    }
    if interface.as_str() == DBUS.2
        && message
            .body()
            .deserialize::<(String, String, String)>()
            .is_ok_and(|(name, ..)| name == NOTICES.0)
    {
        showing.lock().forget();
    }
    let _ = wake.send(());
}

/// A signal of the notification server's: a click on a notice of ours told
/// with the token given just before it (spec 1.2), a notice closed
/// forgotten. Others' notices are none of ours, nor what another server
/// says.
fn noticed(member: &str, message: &zbus::Message, showing: &Mutex<Showing>, asked: &OnAsked) {
    if !showing.lock().said_by_server(message) {
        return;
    }
    let body = message.body();
    match member {
        "ActivationToken" => {
            let Ok((id, token)) = body.deserialize::<(u32, String)>() else {
                return;
            };
            let mut showing = showing.lock();
            if showing.is(id) || showing.resident == Some(id) {
                showing.token = Some((id, token));
            }
        }
        "ActionInvoked" => {
            let Ok((id, action)) = body.deserialize::<(u32, String)>() else {
                return;
            };
            let what = {
                let mut showing = showing.lock();
                match showing.shown {
                    Some((shown, key)) if shown == id => (action == DEFAULT).then(|| {
                        Asked::Notice(Clicked {
                            key,
                            token: showing.token(id),
                        })
                    }),
                    _ if showing.resident == Some(id) => match action.as_str() {
                        DEFAULT => Some(Asked::Open {
                            token: showing.token(id),
                        }),
                        STOP => Some(Asked::StopAll),
                        _ => None,
                    },
                    _ => None,
                }
            };
            if let Some(what) = what {
                asked(what);
            }
        }
        "NotificationClosed" => {
            let Ok((id, _reason)) = body.deserialize::<(u32, u32)>() else {
                return;
            };
            let mut showing = showing.lock();
            if showing.is(id) {
                showing.shown = None;
            }
            // Closed by the user: not shown again until it says something
            // new.
            if showing.resident == Some(id) {
                showing.resident = None;
            }
            showing.token.take_if(|(of, _)| *of == id);
        }
        _ => {}
    }
}

/// The tray's watcher's owner, if a host shows the items registered there:
/// what is asked is whether the desktop can, never which desktop it is.
fn tray_host(bus: &Connection) -> Option<String> {
    let owner = call(bus, DBUS, "GetNameOwner", &WATCHER.0)
        .and_then(|reply| reply.body().deserialize::<String>())
        .ok()?;
    let hosted = call(
        bus,
        (WATCHER.0, WATCHER.1, "org.freedesktop.DBus.Properties"),
        "Get",
        &(WATCHER.2, "IsStatusNotifierHostRegistered"),
    )
    .and_then(|reply| reply.body().deserialize::<OwnedValue>())
    .is_ok_and(|hosted| bool::try_from(&hosted).unwrap_or(false));
    hosted.then_some(owner)
}

/// The session manager on the bus that shows the session's end itself
/// and holds sleep off for the portal, if any.
fn session_manager(bus: &Connection) -> Option<&'static str> {
    SESSION_MANAGERS
        .into_iter()
        .find(|name| has_owner(bus, name))
}

fn has_owner(bus: &Connection, name: &str) -> bool {
    call(bus, DBUS, "NameHasOwner", &name)
        .and_then(|reply| reply.body().deserialize::<bool>())
        .unwrap_or(false)
}

/// The connection's own name on the bus: another once the bus started
/// again.
fn unique(bus: &Connection) -> Option<String> {
    bus.unique_name().map(ToString::to_string)
}

/// Call `method` of `(name, path, interface)` on `bus`, waiting at most
/// [`ANSWER`]: the reply given up on, zbus drops it when it comes.
fn call<B>(
    bus: &Connection,
    (name, path, interface): (&str, &str, &str),
    method: &str,
    body: &B,
) -> zbus::Result<zbus::Message>
where
    B: serde::Serialize + zbus::zvariant::DynamicType,
{
    let answered = bus
        .inner()
        .call_method(Some(name), path, Some(interface), method, body);
    zbus::block_on(futures_lite::future::or(answered, async {
        async_io::Timer::after(ANSWER).await;
        Err(zbus::Error::Failure(format!(
            "{name} did not answer {method} within {} s",
            ANSWER.as_secs()
        )))
    }))
}
