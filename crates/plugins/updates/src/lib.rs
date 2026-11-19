//! Says when a new version of skwir is out, where nothing else tells it
//! (ADR-142), and what is new once it is updated.
//!
//! A copy says how it came (`skwir_platform::channel`): only one downloaded
//! from the release (the `AppImage`, the tarball, Windows' installer and
//! zip, the Mac app, a `.deb` or `.rpm` on a system without Skwir's
//! repository) is ever checked, and only once its user said yes, asked
//! once in a toast that stays (`App::ask_quietly`) two days after the copy
//! was first seen. Then at the first look of each calendar day
//! (`Plugin::on_looked_at`) a thread of its own asks the site for
//! `updates/1.json` (`check`): quietly when it fails, never on a metered
//! connection, sending nothing about the copy, reading only versions. A
//! newer version is told once, in a toast, 14 days apart unless one is
//! important (`kept`), and kept said in About and More (`registry::News`)
//! until the copy is updated. Nothing is ever downloaded or installed.
//!
//! What's new is said once after an update, on every channel, with no
//! request: the running version against the highest one run before.

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

/// How this copy came, as read on a worker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Here {
    /// The word its package wrote, if skwir's own package wrote one.
    pub channel: Option<Channel>,
    /// Whether the system has Skwir's repository for it.
    pub repository: bool,
}

impl Here {
    /// How the running program came. Reads the disk: on a worker.
    pub fn read() -> Here {
        let channel = skwir_platform::channel::of_this_program();
        Here {
            channel,
            repository: channel.is_some_and(skwir_platform::channel::repository_added),
        }
    }

    /// The channel of a copy nothing else updates: one skwir's package
    /// wrote, not on a system whose package manager has its repository.
    pub fn downloaded(self) -> Option<Channel> {
        self.channel
            .filter(|channel| !(channel.has_repository() && self.repository))
    }
}

type Clock = Arc<dyn Fn() -> i64 + Send + Sync>;

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
    /// Held across a read and a write of what is kept, so a look and a
    /// check never write over each other.
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
    /// A check under way, and when the last that got no answer ended.
    checking: bool,
    failed: Option<Instant>,
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
            run.checking = false;
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
                if cancel.is_cancelled() {
                    return;
                }
                inner.run.lock().here = Some(here);
                if here.downloaded().is_some() {
                    inner.declare_tell(&app);
                }
                inner.stand(&app);
                // A window looked at meanwhile is looked at now.
                if app.looked_at() {
                    inner.looked_at(&app);
                }
            },
        );
    }
    fn on_settings_changed(&self, app: &App, keys: &[String]) {
        if !keys.iter().any(|key| key == TELL) {
            return;
        }
        // Chosen in Settings: the question is never asked.
        self.inner.answered(app);
        self.inner.stand(app);
        if tells(app) {
            self.inner.start_check(app, false);
        }
    }
    fn on_looked_at(&self, app: &App) {
        self.inner.looked_at(app);
    }
    fn on_disable(&self, _app: &App) {
        let mut run = self.inner.run.lock();
        if let Some(lifetime) = run.lifetime.take() {
            lifetime.cancel();
        }
        *run = Run::default();
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
    fn register_commands(&self, app: &App) {
        let inner = Arc::clone(&self.inner);
        app.commands
            .register::<NoArgs, _>(
                ID,
                Topic::Skwir,
                "updates.check",
                words!(updates_check = "Check for a new version"),
                words!(
                    updates_check_description = "Ask skwir.org now whether a newer version of Skwir is out, for a copy that is told of them: a toast says what it found"
                ),
                move |_, app| inner.check_now(app),
            );
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
        let inner = Arc::clone(&self.inner);
        app.commands
            .register::<AnswerArgs, _>(
                ID,
                Topic::Skwir,
                "updates.answer",
                words!(updates_answer = "Answer whether to be told of new versions"),
                words!(
                    updates_answer_description = "The answer to the question a downloaded copy asks once: whether to ask skwir.org once a day"
                ),
                move |args, app| {
                    inner.answer(app, args.tell);
                    Ok(Outcome::Done)
                },
            )
            .effect(Effect::UserOnly);
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

/// Save what was set, on a worker: the window's thread never writes.
fn save_later(app: &App) {
    let weak = app.weak();
    app.scheduler.spawn(
        skwir_core::scheduler::Priority::Background,
        "updates-save",
        app.lifetime().child(),
        move || {
            if let Some(app) = weak.upgrade()
                && let Err(error) = app.settings.save()
            {
                tracing::warn!("cannot save what the updates plugin keeps: {error:#}");
            }
        },
    );
}

impl Inner {
    fn here(&self) -> Option<Here> {
        self.run.lock().here
    }

    fn downloaded(&self) -> Option<Channel> {
        self.here().and_then(Here::downloaded)
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
            .page(page::GENERAL)
            .section_of(SECTION, section_title()),
        );
    }

    /// A window came to the front: what is new is said once a run, the
    /// question asked when its day comes, and the day's check started.
    fn looked_at(self: &Arc<Self>, app: &App) {
        let (here, look_for_whats_new) = {
            let mut run = self.run.lock();
            if run.lifetime.is_none() {
                return;
            }
            let look = !run.looked_for_whats_new;
            run.looked_for_whats_new = true;
            (run.here, look)
        };
        let today = (self.setup.today)();
        let check = {
            let _held = self.kept.lock();
            let mut kept = Kept::read(&app.settings);
            let before = kept.clone();
            kept.as_of(today);
            if look_for_whats_new
                && let Some(running) = self.setup.running
                && let Some(updated) = kept.whats_new(running)
                && app.settings.get::<bool>(WHATS_NEW).unwrap_or(true)
            {
                app.notify_with_action(
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
            if let Some(channel) = here.and_then(Here::downloaded) {
                kept.first_seen.get_or_insert(today);
                if kept.asks(today) {
                    kept.asked += 1;
                    kept.asked_on = Some(today);
                    self.ask(app, channel);
                }
                check = tells(app) && kept.checked_on != Some(today);
            }
            if kept != before {
                kept.write(&app.settings);
                save_later(app);
            }
            check
        };
        if check {
            self.start_check(app, false);
        }
    }

    /// The question, in a toast that stays: Yes, No, and for a `.deb` or
    /// an `.rpm` the site's way to update with the system, which answers
    /// nothing.
    fn ask(&self, app: &App, channel: Channel) {
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
                command: open_url(&format!("{}/download#repository", self.setup.site)),
                // A page to read: the question stays up.
                answers: false,
            });
        }
        app.ask_quietly(
            words!(
                updates_ask = "Tell you when a new version of Skwir is out? Skwir would ask { $site } once a day; Cloudflare, which serves it, sees your IP address.",
                site = host(&self.setup.site),
                note = "Asked once, in a message that stays, of a copy downloaded from the website. site: the website, as written (skwir.org)."
            ),
            answers,
        );
    }

    /// The question's answer: the setting written, never asked again, and
    /// a yes checked at once.
    fn answer(self: &Arc<Self>, app: &App, tell: bool) {
        app.settings.set(TELL, toml::Value::Boolean(tell));
        self.answered(app);
        self.stand(app);
        if tell {
            app.notify(
                NotifyLevel::Done,
                words!(
                    updates_told_yes = "Skwir will tell you when a new version is out",
                    note = "Said once the question about new versions was answered yes."
                ),
            );
            self.start_check(app, true);
        } else {
            app.notify(
                NotifyLevel::Done,
                words!(
                    updates_told_no = "Skwir will not ask again; Settings can turn it on",
                    note = "Said once the question about new versions was answered no: the setting Tell me when a new version is out, in Settings › General."
                ),
            );
        }
    }

    /// The question is never asked again.
    fn answered(&self, app: &App) {
        let _held = self.kept.lock();
        let mut kept = Kept::read(&app.settings);
        if !kept.answered {
            kept.answered = true;
            kept.write(&app.settings);
        }
        save_later(app);
    }

    /// `updates.check`: a check now, whatever the day.
    fn check_now(self: &Arc<Self>, app: &App) -> anyhow::Result<Outcome> {
        let Some(here) = self.here() else {
            anyhow::bail!(words!(
                updates_not_ready = "Skwir is still reading how it was installed: try again",
                note = "Why a check for a new version was not made: just started, Skwir has not read yet how it was installed."
            ));
        };
        if here.downloaded().is_none() {
            anyhow::bail!(words!(
                updates_managed = "This copy of Skwir is not checked: whatever installed it tells of new versions",
                note = "Why a check for a new version was not made: a package manager (the AUR, Nix, a distribution, Skwir's repository) or a build from source."
            ));
        }
        if !tells(app) {
            anyhow::bail!(words!(
                updates_off =
                    "This copy is not told of new versions: turn it on in Settings › General",
                note = "Why a check for a new version was not made: the setting Tell me when a new version is out is off."
            ));
        }
        self.start_check(app, true);
        Ok(Outcome::Value(serde_json::json!({ "checking": true })))
    }

    /// `updates.latest`, from memory.
    fn latest(&self, app: &App) -> serde_json::Value {
        let here = self.here();
        let kept = {
            let _held = self.kept.lock();
            Kept::read(&app.settings)
        };
        let day = |day: Option<i64>| {
            day.map(|day| {
                let (y, m, d) = skwir_platform::calendar::civil_from_days(day);
                format!("{y:04}-{m:02}-{d:02}")
            })
        };
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

    /// Start a check on a thread of its own, unless one runs, or, unless
    /// `forced`, the last got no answer less than `AGAIN_AFTER` ago.
    fn start_check(self: &Arc<Self>, app: &App, forced: bool) {
        let lifetime = {
            let mut run = self.run.lock();
            if run.checking
                || (!forced
                    && run
                        .failed
                        .is_some_and(|failed| failed.elapsed() < AGAIN_AFTER))
            {
                return;
            }
            let Some(lifetime) = run.lifetime.clone() else {
                return;
            };
            run.checking = true;
            lifetime
        };
        let inner = Arc::clone(self);
        let weak = app.weak();
        let spawned = std::thread::Builder::new()
            .name("skwir-updates".into())
            .spawn(move || {
                let answered = inner.check(&weak, &lifetime, forced);
                if let Err(error) = &answered {
                    tracing::debug!("updates: no answer: {error:#}");
                }
                let mut run = inner.run.lock();
                run.checking = false;
                run.failed = match answered {
                    Ok(true) => None,
                    _ => Some(Instant::now()),
                };
            });
        if let Err(error) = spawned {
            self.run.lock().checking = false;
            tracing::warn!("updates: cannot start a check: {error}");
        }
    }

    /// Ask the site, then keep and tell what it answered: whether it
    /// answered.
    fn check(&self, app: &Weak<App>, lifetime: &CancelToken, forced: bool) -> anyhow::Result<bool> {
        if !forced && (self.setup.metered)() == Some(true) {
            tracing::debug!("updates: the connection is metered, put off");
            return Ok(false);
        }
        let gone = || anyhow::anyhow!("the application ended");
        let etag = {
            let app = app.upgrade().ok_or_else(gone)?;
            let _held = self.kept.lock();
            Kept::read(&app.settings).etag
        };
        let fetched = check::fetch(&self.setup.site, etag.as_deref())?;
        let app = app.upgrade().ok_or_else(gone)?;
        if lifetime.is_cancelled() {
            return Ok(false);
        }
        let today = (self.setup.today)();
        let kept = {
            let _held = self.kept.lock();
            let mut kept = Kept::read(&app.settings);
            kept.as_of(today);
            if let check::Fetched::Read { answer, etag } = fetched {
                kept.latest = Some(answer.latest.to_string());
                kept.important_since = answer.important_since.map(|v| v.to_string());
                kept.etag = etag;
            }
            kept.checked_on = Some(today);
            if let Some(running) = self.setup.running
                && let Some(telling) = kept.tells(running, today)
            {
                kept.told = Some(telling.version.to_string());
                kept.told_on = Some(today);
                app.notify_with_action(
                    NotifyLevel::Info,
                    said_out(telling),
                    whats_new_label(),
                    release_page(&self.setup.site, telling.version),
                );
            }
            kept.write(&app.settings);
            kept
        };
        if let Err(error) = app.settings.save() {
            tracing::warn!("cannot save what the updates plugin keeps: {error:#}");
        }
        self.stand_with(&app, &kept);
        Ok(true)
    }

    /// What About and More say, from what is kept.
    fn stand(&self, app: &App) {
        let kept = {
            let _held = self.kept.lock();
            Kept::read(&app.settings)
        };
        self.stand_with(app, &kept);
    }

    /// A newer version kept said while the copy is told of them and older.
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
        if app.registry.news.replace_owner(ID, news) {
            app.wake();
        }
    }
}
