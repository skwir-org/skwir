//! Says when a new version of skwir is out, where nothing else tells it
//! (ADR-142), and what is new once it is updated.
//!
//! A copy says how it came (`skwir_platform::channel`): only one downloaded
//! from the release (the `AppImage`, the tarball, Windows' installer and
//! zip, the Mac app where Homebrew's cask is not installed, a `.deb` or
//! `.rpm` on a system without Skwir's repository) is ever checked, and
//! only once its user said yes, asked
//! once in a toast that stays (`App::ask_quietly`) two days after the copy
//! was first seen. Then at the first look of each calendar day
//! (`Plugin::on_looked_at`) a thread of its own asks the site for
//! `updates/1.json` (`check`): quietly when it fails, on a metered
//! connection only when asked for now (`updates.check`, which always
//! answers), sending nothing about the copy, reading only versions. A
//! newer version is told once, in a toast, 14 days apart unless one is
//! important (`kept`), and kept said in About and More (`registry::News`)
//! until the copy is updated. Nothing is ever downloaded or installed.
//!
//! What's new is said once after an update, on every channel, with no
//! request: the running version against the highest one run before.
//!
//! Nothing here reads or writes a file on the window's thread, nor under
//! a lock another waits for: a look's work is a job of its own, outside
//! the plugin host's lock, which reads the state file only where it may
//! decide something (`Inner::looked_at`); the answer to the question is
//! kept on a worker (`Inner::answer`); and the plugin's own lock of what
//! is kept is held only over memory (`Inner::save`).

pub mod check;
pub mod kept;
pub mod version;

#[cfg(test)]
mod tests;

use std::sync::{Arc, Weak};
use std::time::{Duration, Instant, SystemTime};

use parking_lot::Mutex;
use schemars::JsonSchema;
use serde::Deserialize;
use skwir_core::app::{NotifyAction, NotifyLevel};
use skwir_core::command::{Effect, Invocation, Outcome, Topic};
use skwir_core::plugin::Plugin;
use skwir_core::registry::News;
use skwir_core::settings::{SettingSpec, page};
use skwir_core::words::Words;
use skwir_core::{App, CancelToken, words};
use skwir_platform::channel::Channel;

use crate::kept::{Kept, Telling};
use crate::version::Version;

/// The plugin's id.
pub const ID: &str = "updates";
/// Whether a downloaded copy asks the site, once a day.
pub const TELL: &str = "updates.tell";
/// Whether What's new is said after an update.
pub const WHATS_NEW: &str = "updates.whats_new";
/// The settings' section its two settings are under.
const SECTION: &str = "updates";
/// A check that got no answer (or was put off, the connection metered) is
/// tried again at a later look, this long after at the soonest.
const AGAIN_AFTER: Duration = Duration::from_hours(1);

/// Who asked for a check: what it may do and what it says. Ordered by how
/// much is said, so a check under way that is asked again answers as the
/// one who asks most (`Inner::start_check`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Asked {
    /// The day's, at a look or as the setting is turned on: put off on a
    /// metered connection, and an hour after one that got no answer; says
    /// only a version `kept` tells.
    Daily,
    /// The question answered Yes: as the day's, and says the confirmation
    /// where it tells no version, rather than one toast over the other.
    Answered,
    /// `updates.check`, the user's own "now": made on a metered connection
    /// too, whatever failed within the hour, and always answered: the
    /// newer version found (the 14 days aside), up to date, or why not.
    Now,
}

/// How this copy came, as read on a worker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Here {
    /// The word its package wrote, if skwir's own package wrote one.
    pub channel: Option<Channel>,
    /// Whether a package manager of the system updates it: Skwir's
    /// repository added for a `.deb` or an `.rpm`, Homebrew's cask
    /// installed for the Mac app (`skwir_platform::channel::managed`).
    pub managed: bool,
}

impl Here {
    /// How the running program came. Reads the disk: on a worker.
    pub fn read() -> Here {
        let channel = skwir_platform::channel::of_this_program();
        Here {
            channel,
            managed: channel.is_some_and(skwir_platform::channel::managed),
        }
    }

    /// The channel of a copy nothing else updates: one skwir's package
    /// wrote, which no package manager of the system updates.
    pub fn downloaded(self) -> Option<Channel> {
        self.channel
            .filter(|channel| !(channel.may_be_managed() && self.managed))
    }
}

/// A system the site gives the repository's commands of: what the page
/// a `.deb` or an `.rpm` copy's question links to shows
/// (`/download?package=<word>`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    Debian,
    Ubuntu,
    Fedora,
}

impl Family {
    /// The site's word for it.
    pub fn word(self) -> &'static str {
        match self {
            Family::Debian => "debian",
            Family::Ubuntu => "ubuntu",
            Family::Fedora => "fedora",
        }
    }

    /// The system a copy that came by `channel` runs on, as its os-release
    /// says it (`os`, read only for a `.deb` or an `.rpm`), where the site
    /// has its repository's commands. For an `.rpm`, Fedora where the
    /// system is or is like Fedora (its spins, Nobara; Red Hat's and
    /// those built from it, Rocky Linux among them, which say they are like
    /// it); another rpm system (openSUSE, Mageia) is none, the site serving
    /// Fedora alone.
    /// For a `.deb` (one word for Debian's and Ubuntu's builds, whose
    /// repository packages differ), Ubuntu where the system is or is like
    /// Ubuntu (Linux Mint, Pop!_OS, elementary OS, Zorin OS), else Debian
    /// where it is or is like Debian (LMDE, MX Linux). None named, the site
    /// guesses.
    pub fn of(
        channel: Channel,
        os: impl FnOnce() -> Option<skwir_platform::os_release::OsRelease>,
    ) -> Option<Family> {
        let known = |os: &skwir_platform::os_release::OsRelease, family: Family| {
            os.is_like(family.word()).then_some(family)
        };
        match channel {
            Channel::Rpm => known(&os()?, Family::Fedora),
            Channel::Deb => {
                let os = os()?;
                known(&os, Family::Ubuntu).or_else(|| known(&os, Family::Debian))
            }
            _ => None,
        }
    }
}

/// The site's page on adding Skwir's repository: the commands of the
/// system the copy runs on (`?package=`) where it is known, else of the
/// one the site guesses from the browser; `#repository` scrolls to them,
/// and is what an older page reads alone.
fn repository_page(site: &str, family: Option<Family>) -> String {
    match family {
        Some(family) => format!("{site}/download?package={}#repository", family.word()),
        None => format!("{site}/download#repository"),
    }
}

type Clock = Arc<dyn Fn() -> i64 + Send + Sync>;
type Families = Arc<dyn Fn(Channel) -> Option<Family> + Send + Sync>;

/// The run a worker was started for ended (the plugin turned off): what
/// it read is let go.
struct Ended;

/// What the plugin is given: the site asked, the version that runs, and
/// what tells where it runs. The program's own by default; a test's.
pub struct Setup {
    /// The site's address, without its last slash (`https://skwir.org`).
    pub site: String,
    pub running: Option<Version>,
    pub here: Arc<dyn Fn() -> Here + Send + Sync>,
    /// The user's calendar day (`calendar::Calendar::day_of`).
    pub today: Clock,
    pub metered: Arc<dyn Fn() -> Option<bool> + Send + Sync>,
    /// The system a `.deb` or an `.rpm` runs on (`Family::of`, from its
    /// os-release): read only as the question is asked, for its link.
    pub family: Families,
    /// The clock the hour after a check that got no answer is counted on
    /// (`Instant::now`).
    pub now: Arc<dyn Fn() -> Instant + Send + Sync>,
}

impl Default for Setup {
    fn default() -> Self {
        Setup {
            site: env!("CARGO_PKG_HOMEPAGE").trim_end_matches('/').to_owned(),
            running: Version::running(),
            here: Arc::new(Here::read),
            today: Arc::new(|| {
                skwir_platform::calendar::Calendar::system().day_of(SystemTime::now())
            }),
            metered: Arc::new(skwir_platform::network::metered),
            family: Arc::new(|channel| {
                Family::of(channel, skwir_platform::os_release::OsRelease::read)
            }),
            now: Arc::new(Instant::now),
        }
    }
}

pub struct UpdatesPlugin {
    inner: Arc<Inner>,
}

impl Default for UpdatesPlugin {
    fn default() -> Self {
        UpdatesPlugin::with(Setup::default())
    }
}

impl UpdatesPlugin {
    pub fn with(setup: Setup) -> Self {
        UpdatesPlugin {
            inner: Arc::new(Inner {
                setup,
                run: Mutex::new(Run::default()),
                kept: Mutex::new(()),
            }),
        }
    }
}

struct Inner {
    setup: Setup,
    /// While registered.
    run: Mutex<Run>,
    /// Held across a read and a write of what is kept in memory, so a look
    /// and a check never write over each other; never across a file read
    /// or written. Taken before `run` where both are held.
    kept: Mutex<()>,
}

#[derive(Default)]
struct Run {
    /// Cancelled on disable: an answer that comes after it is let go.
    lifetime: Option<CancelToken>,
    /// How this copy came, once read.
    here: Option<Here>,
    /// Whether this run looked for what is new yet.
    looked_for_whats_new: bool,
    /// A check under way, and who asked it (the most of those who did);
    /// when the last that got no answer ended.
    checking: Option<Asked>,
    failed: Option<Instant>,
    /// The last check's thread, for a test to wait for its end.
    #[cfg(test)]
    thread: Option<std::thread::JoinHandle<()>>,
    /// How many looks ended, for a test to wait for one.
    #[cfg(test)]
    looks: u64,
}

impl Plugin for UpdatesPlugin {
    fn id(&self) -> &'static str {
        ID
    }
    fn title(&self) -> Words {
        words!(updates_plugin_title = "Updates")
    }
    fn description(&self) -> Words {
        words!(
            updates_plugin_description = "Say when a new version of Skwir is out, for a copy downloaded from its website that no package manager updates, and what is new after an update"
        )
    }
    fn category(&self) -> skwir_core::plugin::Category {
        skwir_core::plugin::Category::Desktop
    }
    fn capabilities(&self) -> skwir_core::permissions::Capabilities {
        use skwir_core::permissions::Capability;
        skwir_core::permissions::Capabilities::of(&[Capability::Network])
    }
    fn register(&self, app: &App) {
        app.settings.declare(
            SettingSpec::toggle(
                WHATS_NEW,
                words!(updates_whats_new_setting = "Say what’s new after an update"),
                words!(
                    updates_whats_new_setting_description = "Once Skwir is updated, a message says so once, with a button for the new version’s notes."
                ),
                true,
            )
            .page(page::GENERAL)
            .section_of(SECTION, section_title()),
        );
        app.settings.declare(SettingSpec::state(
            kept::KEY,
            words!(
                updates_kept_description = "What the updates plugin keeps: when this copy was first seen and asked, the last check and what it told."
            ),
            toml::Value::Table(toml::Table::new()),
        ));
        self.register_commands(app);
        let lifetime = app.lifetime().child();
        {
            let mut run = self.inner.run.lock();
            if let Some(before) = run.lifetime.replace(lifetime.clone()) {
                before.cancel();
            }
            run.here = None;
            run.checking = None;
            run.failed = None;
        }
        // How the copy came is read on a worker; what depends on it waits.
        let inner = Arc::clone(&self.inner);
        let weak = app.weak();
        let cancel = lifetime.clone();
        app.scheduler.spawn(
            skwir_core::scheduler::Priority::Background,
            "updates-here",
            lifetime,
            move || {
                let here = (inner.setup.here)();
                let Some(app) = weak.upgrade() else {
                    return;
                };
                // A window of the user's own looked at meanwhile is looked
                // at now.
                if inner.take_here(&app, here, &cancel) && app.looked_at_own() {
                    inner.looked_at(&app);
                }
            },
        );
    }
    fn on_settings_changed(&self, app: &App, keys: &[String]) {
        if !keys.iter().any(|key| key == TELL) {
            return;
        }
        // Chosen in Settings (or in another skwir): the question is never
        // asked, and one up goes. Saved on a job of its own, outside the
        // plugin host's lock this is called under.
        app.take_back_question(ID);
        if self.inner.answered(app) {
            self.inner.save_later(app);
        }
        self.inner.stand(app);
        if self.inner.downloaded().is_some() && tells(app) {
            self.inner.start_check(app, Asked::Daily);
        }
    }
    fn on_looked_at(&self, app: &App) {
        self.inner.look_later(app);
    }
    fn on_disable(&self, app: &App) {
        let mut run = self.inner.run.lock();
        if let Some(lifetime) = run.lifetime.take() {
            lifetime.cancel();
        }
        *run = Run::default();
        // What a worker of this run added under this lock after the host
        // withdrew the plugin's contributions, and before the cancel just
        // made, goes too: commands have no owner's gate.
        app.commands.unregister_owner(ID);
        app.settings.undeclare(TELL);
    }
}

#[derive(Default, Deserialize, JsonSchema)]
struct NoArgs {}

#[derive(Deserialize, JsonSchema)]
struct AnswerArgs {
    /// Whether to be told when a new version is out.
    tell: bool,
}

impl UpdatesPlugin {
    /// The commands of every copy; `updates.check` and `updates.answer` are
    /// a downloaded copy's alone (`Inner::register_downloaded`).
    fn register_commands(&self, app: &App) {
        let inner = Arc::clone(&self.inner);
        app.commands
            .register::<NoArgs, _>(
                ID,
                Topic::Skwir,
                "updates.latest",
                words!(updates_latest = "What the last check found"),
                words!(
                    updates_latest_description = "How this copy came, whether it is told of new versions, and the newest version the last check found, from memory"
                ),
                move |_, app| Ok(Outcome::Value(inner.latest(app))),
            )
            .data();
    }
}

/// The settings' section heading.
fn section_title() -> Words {
    words!(
        updates_section = "Updates",
        note = "A heading of a section of the settings: being told of new versions of Skwir."
    )
}

/// Whether the user said yes.
fn tells(app: &App) -> bool {
    app.settings.get::<bool>(TELL).unwrap_or(false)
}

/// The site's name as people write it (`skwir.org`).
fn host(site: &str) -> &str {
    site.split_once("://").map_or(site, |(_, host)| host)
}

/// Open `version`'s page on the site: its notes and its files.
fn release_page(site: &str, version: Version) -> Invocation {
    open_url(&format!("{site}/releases/v{version}"))
}

fn open_url(url: &str) -> Invocation {
    Invocation::with_args("ui.open_url", &serde_json::json!({ "url": url }))
}

/// Say `text`, as the plugin's: refused while it is off, and gone with it
/// when it is turned off, queued or shown (`App::notify_from`, ADR-142 4).
fn say(app: &App, level: NotifyLevel, text: impl Into<Words>) {
    app.notify_from(ID, level, text, Vec::new());
}

/// `say`, with a button: `label`, running `command`.
fn say_with(
    app: &App,
    level: NotifyLevel,
    text: impl Into<Words>,
    label: Words,
    command: Invocation,
) {
    app.notify_from(
        ID,
        level,
        text,
        vec![NotifyAction {
            label,
            command,
            answers: true,
        }],
    );
}

/// Why a copy that came as `here`, told of new versions or not (`tells`),
/// asks the site nothing: `None` where it does.
fn not_checked(here: Here, tells: bool) -> Option<Words> {
    if here.downloaded().is_none() {
        return Some(words!(
            updates_managed =
                "This copy of Skwir is not checked: whatever installed it tells of new versions",
            note = "Why a check for a new version was not made: a package manager (the AUR, Nix, a distribution, Skwir's repository, Homebrew) or a build from source."
        ));
    }
    (!tells).then(|| {
        words!(
            updates_off = "This copy is not told of new versions: turn it on in Settings › Privacy",
            note = "Why a check for a new version was not made: the setting Tell me when a new version is out is off."
        )
    })
}

/// The confirmation of a yes, said where the check it starts tells no
/// version.
fn told_yes() -> Words {
    words!(
        updates_told_yes = "Skwir will tell you when a new version is out",
        note = "Said once the question about new versions was answered yes."
    )
}

fn whats_new_label() -> Words {
    words!(
        updates_whats_new = "What’s new",
        note = "The button opening a version's notes on Skwir's website, in the browser."
    )
}

/// What is said of a newer version out.
fn said_out(telling: Telling) -> Words {
    let version = telling.version.to_string();
    if telling.important {
        words!(
            updates_out_important = "Skwir { $version } is out, with an important fix",
            version = version,
            note = "Said once, and in About until updated, when a newer version is out that fixes a loss of data or a hole in security: version is its number (0.6.2)."
        )
    } else {
        words!(
            updates_out = "Skwir { $version } is out",
            version = version,
            note = "Said once, and in About until updated, when a newer version of Skwir is out: version is its number (0.6.2)."
        )
    }
}

impl Inner {
    /// Save what is kept, merged once more with what the state file holds
    /// just before (`Kept::merged`), written again only where that file
    /// brought something. Reads and writes the disk: on a worker, the
    /// plugin's lock taken only over the merge in memory (`Settings::save`
    /// serializes its writers itself).
    fn save(&self, app: &App) {
        if let Some(theirs) = Kept::on_disk(&app.settings) {
            let _held = self.kept.lock();
            let mine = Kept::read(&app.settings);
            let merged = mine.clone().merged(&theirs);
            if merged != mine {
                merged.write(&app.settings);
            }
        }
        if let Err(error) = app.settings.save() {
            tracing::warn!("cannot save what the updates plugin keeps: {error:#}");
        }
    }

    /// `save`, on a worker: the window's thread never writes.
    fn save_later(self: &Arc<Self>, app: &App) {
        let (inner, weak) = (Arc::clone(self), app.weak());
        app.scheduler.spawn(
            skwir_core::scheduler::Priority::Background,
            "updates-save",
            app.lifetime().child(),
            move || {
                if let Some(app) = weak.upgrade() {
                    inner.save(&app);
                }
            },
        );
    }

    fn here(&self) -> Option<Here> {
        self.run.lock().here
    }

    fn downloaded(&self) -> Option<Channel> {
        self.here().and_then(Here::downloaded)
    }

    /// How the copy came (`Setup::here`), read as the plugin starts and
    /// again at each look of a copy its system may come to update (a
    /// `.deb` or an `.rpm`, whose repository may be added or removed
    /// since, or the Mac app, whose cask may be installed or uninstalled):
    /// what a downloaded copy alone has follows it, the setting
    /// and `updates.check`, offered while it is downloaded and withdrawn
    /// once the system updates it (the setting; the command stays, and
    /// says so), with what About says. Taken only by
    /// the run of `lifetime`, under its lock, so a read of an earlier run
    /// adds nothing to the next or to a plugin turned off: whether it was.
    fn take_here(self: &Arc<Self>, app: &App, here: Here, lifetime: &CancelToken) -> bool {
        {
            let mut run = self.run.lock();
            if lifetime.is_cancelled() {
                return false;
            }
            let before = run.here.replace(here);
            let was = before.and_then(Here::downloaded).is_some();
            match (was, here.downloaded().is_some()) {
                (false, true) => {
                    self.declare_tell(app);
                    self.register_downloaded(app);
                }
                (true, false) => {
                    // `updates.check` stays until the next start, saying
                    // that whatever installed the copy tells of versions;
                    // the question, moot, goes.
                    tracing::debug!("updates: the system updates this copy now");
                    app.settings.undeclare(TELL);
                    app.take_back_question(ID);
                }
                _ if before == Some(here) => return true,
                _ => {}
            }
        }
        self.stand(app);
        true
    }

    /// A downloaded copy's commands: `updates.check`, and `updates.answer`,
    /// which only the question's buttons run (`Declared::asked_only`: no
    /// list of actions offers it), a person's alone.
    fn register_downloaded(self: &Arc<Self>, app: &App) {
        let inner = Arc::clone(self);
        let answer = app.commands.register::<AnswerArgs, _>(
            ID,
            Topic::Skwir,
            "updates.answer",
            words!(updates_answer = "Answer whether to be told of new versions"),
            words!(
                updates_answer_description = "The answer to the question a downloaded copy asks once: whether to ask { $site } once a day",
                site = host(&self.setup.site),
                note = "site: Skwir's website, as written (skwir.org)."
            ),
            move |args, app| {
                inner.answer(app, args.tell);
                Ok(Outcome::Done)
            },
        );
        answer.asked_only();
        answer.effect(Effect::UserOnly);
        let inner = Arc::clone(self);
        app.commands.register::<NoArgs, _>(
            ID,
            Topic::Skwir,
            "updates.check",
            words!(updates_check = "Check for a new version"),
            words!(
                updates_check_description = "Ask { $site } now whether a newer version of Skwir is out, for a copy that is told of them, on a metered connection too: a toast says what it found, that Skwir is up to date, or why it could not tell",
                site = host(&self.setup.site),
                note = "site: Skwir's website, as written (skwir.org)."
            ),
            move |_, app| inner.check_now(app),
        );
    }

    /// The setting a downloaded copy shows: no other copy asks anything.
    fn declare_tell(&self, app: &App) {
        let site = host(&self.setup.site);
        app.settings.declare(
            SettingSpec::toggle(
                TELL,
                words!(updates_tell_setting = "Tell me when a new version is out"),
                (
                    words!(
                        updates_tell_setting_description = "Skwir asks { $site } once a day whether a newer version is out. Cloudflare, which serves it, sees your IP address.",
                        site = site,
                        note = "site: Skwir's website, as written (skwir.org)."
                    ),
                    words!(
                        updates_tell_setting_description_more = "Only a copy downloaded from the website asks: one a package manager updates is told by it."
                    ),
                ),
                false,
            )
            // Where the window says what leaves this machine.
            .page(page::PRIVACY)
            .section_of(SECTION, section_title()),
        );
    }

    /// A window came to the front (`Plugin::on_looked_at`, called under the
    /// plugin host's lock): the look is a job of its own (`looked_at`),
    /// started only where it may do something: at a run's first look
    /// (What's new), and for a copy downloaded or one its system may stop
    /// updating (a `.deb`, an `.rpm` or the Mac app, read again at each
    /// look). A copy
    /// something else updates does nothing more once looked at. Only a
    /// look at a window of the user's own counts (`App::looked_at_own`):
    /// all the plugin says unasked (What's new, the question, a version
    /// out) is said once, and in a window answering another program's
    /// dialog (ADR-141) it would be spent where the user is doing
    /// something else, or go unseen with a skwir started for that dialog.
    fn look_later(self: &Arc<Self>, app: &App) {
        let lifetime = {
            let run = self.run.lock();
            let Some(lifetime) = run.lifetime.clone() else {
                return;
            };
            let follows = run.here.is_some_and(|here| {
                here.downloaded().is_some() || here.channel.is_some_and(Channel::may_be_managed)
            });
            if !app.looked_at_own() || run.looked_for_whats_new && !follows {
                drop(run);
                self.look_ended();
                return;
            }
            lifetime
        };
        let (inner, weak) = (Arc::clone(self), app.weak());
        app.scheduler.spawn(
            skwir_core::scheduler::Priority::Background,
            "updates-look",
            lifetime,
            move || {
                if let Some(app) = weak.upgrade() {
                    inner.looked_at(&app);
                }
            },
        );
    }

    /// A window was looked at: what is new is said once a run, the
    /// question asked when its day comes, and the day's check started. On
    /// a worker, and outside the plugin host's lock.
    fn looked_at(self: &Arc<Self>, app: &App) {
        self.look(app);
        self.look_ended();
    }

    /// A look ended: counted, for a test to wait for it.
    // Nothing is counted outside tests.
    #[cfg_attr(not(test), allow(clippy::unused_self))]
    fn look_ended(&self) {
        #[cfg(test)]
        {
            self.run.lock().looks += 1;
        }
    }

    /// How the copy came, as it is now: a `.deb`, an `.rpm` or the Mac
    /// app, whose system may have come to update it (Skwir's repository
    /// added, Homebrew's cask installed) or no longer does, read again (a
    /// stat: on a worker) and taken (`take_here`); `None` while not read
    /// yet. `Ended` once the run of `lifetime` ended.
    fn here_now(
        self: &Arc<Self>,
        app: &App,
        lifetime: &CancelToken,
    ) -> Result<Option<Here>, Ended> {
        match self.here() {
            Some(known) if known.channel.is_some_and(Channel::may_be_managed) => {
                let now = (self.setup.here)();
                if self.take_here(app, now, lifetime) {
                    Ok(Some(now))
                } else {
                    Err(Ended)
                }
            }
            other => Ok(other),
        }
    }

    fn look(self: &Arc<Self>, app: &App) {
        let (lifetime, look_for_whats_new) = {
            let mut run = self.run.lock();
            let Some(lifetime) = run.lifetime.clone() else {
                return;
            };
            let look = !run.looked_for_whats_new;
            run.looked_for_whats_new = true;
            (lifetime, look)
        };
        let Ok(here) = self.here_now(app, &lifetime) else {
            return;
        };
        let today = (self.setup.today)();
        let downloaded = here.and_then(Here::downloaded);
        // The state file, which another skwir writes too, is read only
        // where this look may decide what that one may have decided first
        // (`Kept::merged`): What's new, once a run; the question, or the
        // day's check, where what this process holds says one may come
        // today. Read before the plugin's lock is taken. The question is
        // asked, and counted, only while a window of the user's own has
        // the keyboard (`App::looked_at_own`): asked again here, as it may
        // have gone to another program's dialog since the look began.
        let (asks, checks) = match downloaded {
            Some(_) => {
                let mut held = Kept::read(&app.settings);
                held.as_of(today);
                let checks = tells(app) && !held.checked_today(today) && self.checks_daily();
                (held.asks(today) && app.looked_at_own(), checks)
            }
            None => (false, false),
        };
        let theirs = if look_for_whats_new || asks || checks {
            Kept::on_disk(&app.settings)
        } else {
            None
        };
        // A settings file that cannot be written (Home Manager's) cannot
        // keep the answer: nothing is asked of it, where `updates.tell` is
        // said as the rest is declared.
        let answerable = asks && !app.settings.read_only();
        // The system, for the question's link: read only as it may be
        // asked.
        let family = downloaded
            .filter(|channel| answerable && channel.has_repository())
            .and_then(|channel| (self.setup.family)(channel));
        let (check, changed) = {
            let _held = self.kept.lock();
            let before = Kept::read(&app.settings);
            let mut kept = match &theirs {
                Some(theirs) => before.clone().merged(theirs),
                None => before.clone(),
            };
            kept.as_of(today);
            if look_for_whats_new
                && let Some(running) = self.setup.running
                && let Some(updated) = kept.whats_new(running)
                && app.settings.get::<bool>(WHATS_NEW).unwrap_or(true)
            {
                say_with(
                    app,
                    NotifyLevel::Info,
                    words!(
                        updates_updated = "Skwir was updated to { $version }",
                        version = updated.to_string(),
                        note = "Said once, the first time a newer version of Skwir runs: version is its number (0.6.2)."
                    ),
                    whats_new_label(),
                    release_page(&self.setup.site, updated),
                );
            }
            let mut check = false;
            if let Some(channel) = downloaded {
                kept.first_seen.get_or_insert(today);
                // The answer is the setting: a copy that wrote it (here,
                // in another skwir, by hand) is never asked, whatever the
                // state file lost.
                if app.settings.stored::<bool>(TELL).is_some() {
                    kept.answered = true;
                }
                if answerable && kept.asks(today) {
                    kept.asked += 1;
                    kept.asked_on = Some(today);
                    self.ask(app, channel, family);
                }
                check = checks && !kept.checked_today(today);
            }
            let changed = kept != before;
            if changed {
                kept.write(&app.settings);
            }
            (check, changed)
        };
        if changed {
            self.save(app);
        }
        if check {
            self.start_check(app, Asked::Daily);
        }
    }

    /// Whether a check the day's look starts would be made now: none under
    /// way, none that got no answer within the hour (`start_check`).
    fn checks_daily(&self) -> bool {
        let run = self.run.lock();
        run.checking.is_none()
            && run.failed.is_none_or(|failed| {
                (self.setup.now)().saturating_duration_since(failed) >= AGAIN_AFTER
            })
    }

    /// The question, in a toast that stays: Yes, No, and for a `.deb` or
    /// an `.rpm` the site's way to update with the system (`family`'s,
    /// where known), which answers nothing.
    fn ask(&self, app: &App, channel: Channel, family: Option<Family>) {
        let answer = |tell: bool| {
            Invocation::with_args("updates.answer", &serde_json::json!({ "tell": tell }))
        };
        let mut answers = vec![
            NotifyAction {
                label: words!(
                    updates_ask_yes = "Yes",
                    note = "The button saying yes to being told of new versions."
                ),
                command: answer(true),
                answers: true,
            },
            NotifyAction {
                label: words!(
                    updates_ask_no = "No",
                    note = "The button saying no to being told of new versions."
                ),
                command: answer(false),
                answers: true,
            },
        ];
        if channel.has_repository() {
            answers.push(NotifyAction {
                label: words!(
                    updates_ask_repository = "Update with your system…",
                    note = "A button of the question about new versions, for a .deb or an .rpm: opens the website's page on adding Skwir's repository, so the system updates it."
                ),
                command: open_url(&repository_page(&self.setup.site, family)),
                // A page to read: the question stays up.
                answers: false,
            });
        }
        app.ask_quietly(
            ID,
            words!(
                updates_ask = "Tell you when a new version of Skwir is out? Skwir would ask { $site } once a day; Cloudflare, which serves it, sees your IP address.",
                site = host(&self.setup.site),
                note = "Asked once, in a message that stays, of a copy downloaded from the website. site: the website, as written (skwir.org)."
            ),
            answers,
        );
    }

    /// The question's answer (`updates.answer`, its buttons): kept on a
    /// worker, as the window's thread never writes nor waits for a save
    /// under way (`Settings::set`); where it is asked elsewhere (a test),
    /// there.
    fn answer(self: &Arc<Self>, app: &App, tell: bool) {
        if !skwir_core::scheduler::on_window_thread() {
            self.keep_answer(app, tell);
            return;
        }
        let Some(lifetime) = self.run.lock().lifetime.clone() else {
            return;
        };
        let (inner, weak) = (Arc::clone(self), app.weak());
        app.scheduler.spawn(
            skwir_core::scheduler::Priority::Normal,
            "updates-answer",
            lifetime,
            move || {
                if let Some(app) = weak.upgrade() {
                    inner.keep_answer(&app, tell);
                }
            },
        );
    }

    /// The answer kept: the setting written and saved, the question never
    /// asked again, and a yes checked at once, which says the version it
    /// finds in place of the confirmation (`Asked::Answered`). An answer
    /// the settings cannot keep (their file made read-only, a full drive)
    /// is taken back and said, and the question may come back another
    /// day. A question made moot meanwhile (the setting chosen elsewhere,
    /// the copy one its system now updates) is only taken down. Writes the
    /// disk: on a worker.
    fn keep_answer(self: &Arc<Self>, app: &App, tell: bool) {
        let Some(lifetime) = self.run.lock().lifetime.clone() else {
            return;
        };
        let Ok(here) = self.here_now(app, &lifetime) else {
            return;
        };
        let moot = here.and_then(Here::downloaded).is_none()
            || app.settings.stored::<bool>(TELL).is_some();
        if moot {
            app.take_back_question(ID);
            return;
        }
        let before = app.settings.stored::<toml::Value>(TELL);
        app.settings.set(TELL, toml::Value::Boolean(tell));
        if let Err(error) = app.settings.save() {
            // A read-only file's settings are put back by `save`; any
            // other failure leaves the change to be saved: taken back.
            if app.settings.stored::<bool>(TELL) == Some(tell) {
                match before {
                    Some(value) => app.settings.set(TELL, value),
                    None => app.settings.unset(TELL),
                }
            }
            say(
                app,
                NotifyLevel::Error,
                Words::of_error(error.context(words!(
                    updates_answer_not_kept = "Skwir could not keep your answer",
                    note = "Said when the answer to the question about new versions could not be saved in the settings; why follows (the settings file is read-only, the drive is full)."
                ))),
            );
            return;
        }
        if self.answered(app) {
            self.save(app);
        }
        self.stand(app);
        if tell {
            if !self.start_check(app, Asked::Answered) {
                say(app, NotifyLevel::Done, told_yes());
            }
        } else {
            say(
                app,
                NotifyLevel::Done,
                words!(
                    updates_told_no = "Skwir will not ask again; Settings can turn it on",
                    note = "Said once the question about new versions was answered no: the setting Tell me when a new version is out, in Settings › Privacy."
                ),
            );
        }
    }

    /// The question is never asked again: what is kept says so, in
    /// memory. Whether it did not yet, for the caller to save.
    fn answered(&self, app: &App) -> bool {
        let _held = self.kept.lock();
        let mut kept = Kept::read(&app.settings);
        if kept.answered {
            return false;
        }
        kept.answered = true;
        kept.write(&app.settings);
        true
    }

    /// `updates.check`: a check now, whatever the day.
    fn check_now(self: &Arc<Self>, app: &App) -> anyhow::Result<Outcome> {
        let Some(here) = self.here() else {
            anyhow::bail!(words!(
                updates_not_ready = "Skwir is still reading how it was installed: try again",
                note = "Why a check for a new version was not made: just started, Skwir has not read yet how it was installed."
            ));
        };
        if let Some(why) = not_checked(here, tells(app)) {
            anyhow::bail!(why);
        }
        if !self.start_check(app, Asked::Now) {
            anyhow::bail!(words!(
                updates_not_started = "The check could not be started",
                note = "Why a check for a new version was not made: the system refused a thread for it."
            ));
        }
        Ok(Outcome::Value(serde_json::json!({ "checking": true })))
    }

    /// `updates.latest`, from memory.
    fn latest(&self, app: &App) -> serde_json::Value {
        let here = self.here();
        let kept = {
            let _held = self.kept.lock();
            Kept::read(&app.settings)
        };
        let day = |day: Option<i64>| day.map(skwir_platform::calendar::date_string);
        serde_json::json!({
            "running": self.setup.running.map(|v| v.to_string()),
            "channel": here.and_then(|here| here.channel).map(Channel::word),
            "checked": here.and_then(Here::downloaded).is_some(),
            "tell": tells(app),
            "latest": kept.latest,
            "important_since": kept.important_since,
            "checked_on": day(kept.checked_on),
        })
    }

    /// Start a check for `asked` on a thread of its own: whether one will
    /// answer it. A check under way answers it, as the one who asked most;
    /// the day's waits `AGAIN_AFTER` after one that got no answer.
    fn start_check(self: &Arc<Self>, app: &App, asked: Asked) -> bool {
        let lifetime = {
            let mut run = self.run.lock();
            if let Some(running) = &mut run.checking {
                *running = (*running).max(asked);
                return true;
            }
            if asked == Asked::Daily
                && run.failed.is_some_and(|failed| {
                    (self.setup.now)().saturating_duration_since(failed) < AGAIN_AFTER
                })
            {
                return false;
            }
            let Some(lifetime) = run.lifetime.clone() else {
                return false;
            };
            run.checking = Some(asked);
            lifetime
        };
        let inner = Arc::clone(self);
        let weak = app.weak();
        let spawned = std::thread::Builder::new()
            .name("skwir-updates".into())
            .spawn(move || inner.check(&weak, &lifetime));
        match spawned {
            Ok(thread) => {
                #[cfg(test)]
                {
                    self.run.lock().thread = Some(thread);
                }
                #[cfg(not(test))]
                drop(thread);
                true
            }
            Err(error) => {
                self.run.lock().checking = None;
                tracing::warn!("updates: cannot start a check: {error}");
                false
            }
        }
    }

    /// The check's thread: ask the site, unless the connection is metered
    /// and nobody asked for now, then keep what it answered and say what
    /// who asked is told. The check ends (`Run::checking`) where who asked
    /// is read for the last time, so one asked meanwhile is answered by it
    /// or started anew. What may have changed since it was asked is asked
    /// again before the site is: a `.deb` or an `.rpm` its system came to
    /// update, the setting turned off. The site is asked nothing then, and
    /// only a check asked for now says why.
    fn check(self: &Arc<Self>, app: &Weak<App>, lifetime: &CancelToken) {
        if let Some(app) = app.upgrade() {
            let Ok(here) = self.here_now(&app, lifetime) else {
                return;
            };
            let unknown = Here {
                channel: None,
                managed: false,
            };
            if let Some(why) = not_checked(here.unwrap_or(unknown), tells(&app)) {
                let asked = {
                    let mut run = self.run.lock();
                    if lifetime.is_cancelled() {
                        return;
                    }
                    run.checking.take()
                };
                if asked == Some(Asked::Now) {
                    say(&app, NotifyLevel::Error, why);
                }
                return;
            }
        }
        let fetched = loop {
            let now = self.run.lock().checking == Some(Asked::Now);
            if now || (self.setup.metered)() != Some(true) {
                break check::fetch(&self.setup.site);
            }
            // Put off to a later look, unless asked for now meanwhile.
            let asked = {
                let mut run = self.run.lock();
                if lifetime.is_cancelled() {
                    return;
                }
                if run.checking == Some(Asked::Now) {
                    continue;
                }
                run.failed = Some((self.setup.now)());
                run.checking.take()
            };
            tracing::debug!("updates: the connection is metered, put off");
            if asked == Some(Asked::Answered)
                && let Some(app) = app.upgrade()
            {
                say(&app, NotifyLevel::Done, told_yes());
            }
            return;
        };
        let Some(app) = app.upgrade() else {
            return;
        };
        let today = (self.setup.today)();
        // What another skwir on the state file told meanwhile, read before
        // the plugin's lock is taken.
        let theirs = fetched
            .as_ref()
            .ok()
            .and_then(|_| Kept::on_disk(&app.settings));
        {
            // `kept` before `run`, wherever both are held.
            let _held = self.kept.lock();
            let (asked, downloaded) = {
                let mut run = self.run.lock();
                if lifetime.is_cancelled() {
                    return;
                }
                run.failed = fetched.is_err().then(|| (self.setup.now)());
                let asked = run.checking.take().unwrap_or(Asked::Daily);
                (asked, run.here.and_then(Here::downloaded).is_some())
            };
            let answer = match fetched {
                Ok(answer) => answer,
                Err(error) => {
                    tracing::debug!("updates: no answer: {error:#}");
                    match asked {
                        Asked::Now => say(
                            &app,
                            NotifyLevel::Error,
                            Words::of_error(error.context(words!(
                                updates_check_failed = "Skwir could not check for a new version",
                                note = "Said when a check for a new version asked for now got no answer it reads; why follows (the website's answer, no connection)."
                            ))),
                        ),
                        Asked::Answered => say(&app, NotifyLevel::Done, told_yes()),
                        Asked::Daily => {}
                    }
                    return;
                }
            };
            // Turned off, or updated by the system, while it was asked:
            // nothing kept, nothing said.
            if !downloaded || !tells(&app) {
                return;
            }
            let mut kept = Kept::read(&app.settings);
            if let Some(theirs) = &theirs {
                kept = kept.merged(theirs);
            }
            kept.as_of(today);
            kept.latest = Some(answer.latest.to_string());
            kept.important_since = answer.important_since.map(|v| v.to_string());
            kept.checked_on = Some(today);
            let telling = self.setup.running.and_then(|running| match asked {
                // Asked for now: the newer version, told or not, the 14
                // days aside.
                Asked::Now => kept.standing(running),
                Asked::Answered | Asked::Daily => kept.tells(running, today),
            });
            match telling {
                Some(telling) => {
                    let told = kept.told.as_deref().and_then(Version::parse);
                    if told.is_none_or(|told| told < telling.version) {
                        kept.told = Some(telling.version.to_string());
                        kept.told_on = Some(today);
                    }
                    say_with(
                        &app,
                        NotifyLevel::Info,
                        said_out(telling),
                        whats_new_label(),
                        release_page(&self.setup.site, telling.version),
                    );
                }
                None => match asked {
                    Asked::Now => say(
                        &app,
                        NotifyLevel::Done,
                        words!(
                            updates_up_to_date = "Skwir is up to date",
                            note = "The answer to a check for a new version asked for now: no newer version is out."
                        ),
                    ),
                    Asked::Answered => say(&app, NotifyLevel::Done, told_yes()),
                    Asked::Daily => {}
                },
            }
            kept.write(&app.settings);
            self.stand_with(&app, &kept);
        }
        self.save(&app);
    }

    /// What About and More say, from what is kept.
    /// Published under the lock it was read under, so what is said last
    /// is what was read last.
    fn stand(&self, app: &App) {
        let _held = self.kept.lock();
        self.stand_with(app, &Kept::read(&app.settings));
    }

    /// A newer version kept said while the copy is told of them and older.
    /// Called under the `kept` lock.
    fn stand_with(&self, app: &App, kept: &Kept) {
        let shown = self
            .setup
            .running
            .filter(|_| self.downloaded().is_some() && tells(app))
            .and_then(|running| kept.standing(running));
        let news: Vec<News> = shown
            .map(|telling| News {
                text: said_out(telling),
                label: whats_new_label(),
                command: release_page(&self.setup.site, telling.version),
            })
            .into_iter()
            .collect();
        // The same said again wakes nothing.
        if app.registry.news.replace_owner_if_changed(ID, news) {
            app.wake();
        }
    }
}
