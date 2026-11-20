//! The plugin against a site of the tests' own: a plain HTTP server on
//! this machine answering what the test sets, and a calendar the test
//! moves. What is asked, when, and what is told.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use skwir_core::App;
use skwir_core::app::Notification;
use skwir_core::command::Invocation;
use skwir_core::plugin::Plugin;
use skwir_platform::channel::Channel;

use crate::kept::Kept;
use crate::version::Version;
use crate::{Family, Here, Setup, TELL, UpdatesPlugin, WHATS_NEW};

/// What the site answers: its file and the file's tag.
#[derive(Clone)]
struct File {
    body: String,
    etag: String,
}

/// A site answering `/updates/1.json` with its file and a tag (304 to a
/// request naming the tag, which a copy never sends), keeping the head of
/// each request; held, it answers once let go (`hold`).
struct Site {
    address: String,
    file: Arc<Mutex<File>>,
    heads: Arc<Mutex<Vec<String>>>,
    gate: Arc<Mutex<()>>,
}

impl Site {
    fn new(body: &str, etag: &str) -> Site {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        let file = Arc::new(Mutex::new(File {
            body: body.into(),
            etag: etag.into(),
        }));
        let heads = Arc::new(Mutex::new(Vec::new()));
        let gate = Arc::new(Mutex::new(()));
        let (served, kept, gated) = (Arc::clone(&file), Arc::clone(&heads), Arc::clone(&gate));
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { return };
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut head = String::new();
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                        break;
                    }
                    head.push_str(&line);
                }
                kept.lock().push(head.clone());
                drop(gated.lock());
                let file = served.lock().clone();
                let wanted = format!("if-none-match: {}", file.etag);
                let same = head
                    .to_ascii_lowercase()
                    .contains(&wanted.to_ascii_lowercase());
                let found = head.starts_with("GET /updates/1.json ");
                let answer = if !found {
                    "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                        .to_owned()
                } else if same {
                    format!(
                        "HTTP/1.1 304 Not Modified\r\nETag: {}\r\nConnection: close\r\n\r\n",
                        file.etag
                    )
                } else {
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nETag: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        file.etag,
                        file.body.len(),
                        file.body
                    )
                };
                let _ = stream.write_all(answer.as_bytes());
            }
        });
        Site {
            address,
            file,
            heads,
            gate,
        }
    }

    /// No answer until what this returns is let go: the request waits.
    fn hold(&self) -> parking_lot::MutexGuard<'_, ()> {
        self.gate.lock()
    }

    fn serve(&self, body: &str, etag: &str) {
        *self.file.lock() = File {
            body: body.into(),
            etag: etag.into(),
        };
    }

    fn heads(&self) -> Vec<String> {
        self.heads.lock().clone()
    }
}

/// An application with the plugin registered, running `running` on a
/// copy that came as `here`, asking `site`, on the test's day and minute,
/// on a connection the test says metered or not.
struct Fixture {
    _folder: Option<tempfile::TempDir>,
    app: Arc<App>,
    /// The window looked at: one of the user's own unless the test says
    /// it answers another program's dialog.
    window: skwir_core::workspace::WindowId,
    plugin: Arc<UpdatesPlugin>,
    day: Arc<AtomicI64>,
    minutes: Arc<AtomicU64>,
    metered: Arc<AtomicBool>,
}

impl Fixture {
    fn new(site: &str, running: Version, here: Here) -> Fixture {
        Fixture::with_here(site, running, Arc::new(move || here))
    }

    /// The plugin added to the application's plugins, as the program
    /// adds it, on a copy whose coming `here` says each time it is read.
    fn with_here(
        site: &str,
        running: Version,
        here: Arc<dyn Fn() -> Here + Send + Sync>,
    ) -> Fixture {
        let folder = tempfile::tempdir().unwrap();
        let config = skwir_core::app::AppConfig::ephemeral(folder.path());
        Fixture::made(config, Some(folder), site, running, here)
    }

    /// A downloaded copy whose settings and state are files in `folder`,
    /// as another skwir's may be the same files.
    fn sharing(folder: &std::path::Path, site: &str, running: Version) -> Fixture {
        let config = skwir_core::app::AppConfig {
            settings_path: Some(folder.join("settings.toml")),
            state_path: Some(folder.join("state.toml")),
            ..skwir_core::app::AppConfig::ephemeral(folder.join(running.to_string()))
        };
        Fixture::made(config, None, site, running, Arc::new(|| DOWNLOADED))
    }

    fn made(
        config: skwir_core::app::AppConfig,
        folder: Option<tempfile::TempDir>,
        site: &str,
        running: Version,
        here: Arc<dyn Fn() -> Here + Send + Sync>,
    ) -> Fixture {
        let app = App::new(config).unwrap();
        let day = Arc::new(AtomicI64::new(20_000));
        let minutes = Arc::new(AtomicU64::new(0));
        let metered = Arc::new(AtomicBool::new(false));
        let (today, elapsed, costly) =
            (Arc::clone(&day), Arc::clone(&minutes), Arc::clone(&metered));
        let started = Instant::now();
        let plugin = Arc::new(UpdatesPlugin::with(Setup {
            site: site.into(),
            running: Some(running),
            here,
            today: Arc::new(move || today.load(Ordering::SeqCst)),
            metered: Arc::new(move || Some(costly.load(Ordering::SeqCst))),
            now: Arc::new(move || started + Duration::from_mins(elapsed.load(Ordering::SeqCst))),
        }));
        app.plugins
            .add(Arc::clone(&plugin) as Arc<dyn Plugin>, &app);
        let window = app.workspace.read().active_window_id();
        let fixture = Fixture {
            _folder: folder,
            app,
            window,
            plugin,
            day,
            minutes,
            metered,
        };
        fixture.wait_until("the copy's channel is read", |f| {
            f.plugin.inner.here().is_some()
        });
        fixture
    }

    fn wait_until(&self, what: &str, done: impl Fn(&Fixture) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !done(self) {
            assert!(Instant::now() < deadline, "waited too long: {what}");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// The window looked at on `day`, as the window tells it (the plugins
    /// told on a worker), until the look's job ended; then let go of.
    fn look_on(&self, day: i64) {
        self.day.store(day, Ordering::SeqCst);
        let looks = self.plugin.inner.run.lock().looks;
        self.app.set_window_focused(self.window, true);
        self.wait_until("the look ends", |f| f.plugin.inner.run.lock().looks > looks);
        self.app.set_window_focused(self.window, false);
    }

    /// `minutes` later than the clock says, the day the same.
    fn later_by(&self, minutes: u64) {
        self.minutes.fetch_add(minutes, Ordering::SeqCst);
    }

    /// The answer to the question, as its button gives it.
    fn answer(&self, tell: bool) {
        self.app
            .dispatch(&Invocation::with_args(
                "updates.answer",
                &serde_json::json!({ "tell": tell }),
            ))
            .unwrap();
    }

    /// `updates.check`, then its answer.
    fn check_now(&self) -> Vec<String> {
        self.app
            .dispatch(&Invocation::new("updates.check"))
            .unwrap();
        self.settled();
        self.texts()
    }

    fn texts(&self) -> Vec<String> {
        self.said().iter().map(|n| n.text.english()).collect()
    }

    /// The checks under way are done.
    fn settled(&self) {
        let thread = self.plugin.inner.run.lock().thread.take();
        if let Some(thread) = thread {
            thread.join().unwrap();
        }
    }

    fn said(&self) -> Vec<Notification> {
        self.app.take_notifications()
    }

    fn news(&self) -> Vec<String> {
        self.app
            .registry
            .news
            .all()
            .into_iter()
            .map(|news| news.text.english())
            .collect()
    }

    fn kept(&self) -> Kept {
        Kept::read(&self.app.settings)
    }

    /// What the state file holds of it now.
    fn kept_on_disk(&self) -> Kept {
        self.app
            .settings
            .kept_on_disk(crate::kept::KEY)
            .and_then(|kept| kept.try_into().ok())
            .unwrap_or_default()
    }
}

const DOWNLOADED: Here = Here {
    channel: Some(Channel::AppImage),
    repository: false,
    family: None,
};

fn file(latest: &str) -> String {
    format!(r#"{{"latest": "{latest}"}}"#)
}

/// The URL a notification's only (or first) button opens.
fn opens(said: &Notification) -> String {
    let command = &said.actions.first().expect("a button").command;
    assert_eq!(&*command.name, "ui.open_url");
    command.args["url"].as_str().unwrap().to_owned()
}

/// A downloaded copy says nothing for two days, then asks once; a yes
/// checks at once and tells of the newer version in place of its
/// confirmation, kept said in About until updated; the next days ask
/// again, and an answer unchanged tells nothing more. The request says
/// nothing about the copy, nor sends back the tag the site gave.
#[test]
fn a_downloaded_copy_asks_once_then_checks_once_a_day() {
    let site = Site::new(&file("0.6.2"), "\"a\"");
    let f = Fixture::new(&site.address, Version(0, 5, 1), DOWNLOADED);
    f.look_on(20_000);
    f.look_on(20_001);
    assert!(f.said().is_empty(), "nothing asked before two days");
    assert!(site.heads().is_empty(), "nothing asked of the site");

    f.look_on(20_002);
    let said = f.said();
    assert_eq!(said.len(), 1);
    let question = &said[0];
    assert!(question.stays, "the question stays until answered");
    assert!(
        question
            .text
            .english()
            .starts_with("Tell you when a new version of Skwir is out?")
    );
    let labels: Vec<String> = question.actions.iter().map(|a| a.label.english()).collect();
    assert_eq!(labels, ["Yes", "No"], "no repository for an AppImage");
    f.look_on(20_002);
    assert!(f.said().is_empty(), "asked once a day");

    f.app.dispatch(&question.actions[0].command).unwrap();
    f.settled();
    assert_eq!(f.texts(), ["Skwir 0.6.2 is out"], "one toast, not two");
    assert_eq!(f.news(), ["Skwir 0.6.2 is out"]);
    let heads = site.heads();
    assert_eq!(heads.len(), 1);
    let head = heads[0].to_ascii_lowercase();
    assert!(head.starts_with("get /updates/1.json http/1.1"), "{head}");
    assert!(head.contains("user-agent: skwir\r\n"), "{head}");
    assert!(
        !head.contains("0.5.1") && !head.contains("cookie"),
        "{head}"
    );

    // Again the same day: no request. The next day: asked again, and the
    // answer unchanged tells nothing again.
    f.look_on(20_002);
    f.settled();
    assert_eq!(site.heads().len(), 1);
    f.look_on(20_003);
    f.settled();
    let heads = site.heads();
    assert_eq!(heads.len(), 2);
    for head in heads {
        assert!(
            !head.to_ascii_lowercase().contains("if-none-match"),
            "no tag of the site's sent back: {head}"
        );
    }
    assert!(f.said().is_empty(), "told once");
    assert_eq!(f.news(), ["Skwir 0.6.2 is out"], "kept said");
    assert_eq!(f.kept().checked_on, Some(20_003));
    assert!(f.kept().answered);
}

/// A version out after the last is told 14 days after it, not before; an
/// important one at once, said as such, and its page opened; the one
/// after the important one 14 days after it again.
#[test]
fn the_next_version_waits_two_weeks_unless_it_is_important() {
    let site = Site::new(&file("0.6.0"), "\"1\"");
    let f = Fixture::new(&site.address, Version(0, 5, 1), DOWNLOADED);
    f.app
        .dispatch(&Invocation::with_args(
            "updates.answer",
            &serde_json::json!({ "tell": true }),
        ))
        .unwrap();
    f.settled();
    let told = f.said();
    assert_eq!(told.last().unwrap().text.english(), "Skwir 0.6.0 is out");
    assert_eq!(
        opens(told.last().unwrap()),
        format!("{}/releases/v0.6.0", site.address)
    );

    site.serve(&file("0.6.1"), "\"2\"");
    f.look_on(20_001);
    f.settled();
    assert!(f.said().is_empty(), "two weeks between notices");
    assert_eq!(f.news(), ["Skwir 0.6.1 is out"], "About says the newest");

    site.serve(
        r#"{"latest": "0.6.2", "important_since": "0.6.2"}"#,
        "\"3\"",
    );
    f.look_on(20_002);
    f.settled();
    let said = f.said();
    assert_eq!(said.len(), 1);
    assert_eq!(
        said[0].text.english(),
        "Skwir 0.6.2 is out, with an important fix"
    );
    assert_eq!(opens(&said[0]), format!("{}/releases/v0.6.2", site.address));
    assert_eq!(f.news(), ["Skwir 0.6.2 is out, with an important fix"]);

    site.serve(
        r#"{"latest": "0.6.3", "important_since": "0.6.2"}"#,
        "\"4\"",
    );
    f.look_on(20_003);
    f.settled();
    assert!(f.said().is_empty(), "the important one was told");
    f.look_on(20_002 + crate::kept::SPACING);
    f.settled();
    assert_eq!(f.texts(), ["Skwir 0.6.3 is out, with an important fix"]);
}

/// "Check for a new version" answers every time: the newer version, told
/// already or within the two weeks, Skwir up to date, or why it could not
/// tell.
#[test]
fn a_check_asked_for_now_always_answers() {
    let site = Site::new(&file("0.5.1"), "\"a\"");
    let f = Fixture::new(&site.address, Version(0, 5, 1), DOWNLOADED);
    f.answer(true);
    f.settled();
    assert_eq!(
        f.texts(),
        ["Skwir will tell you when a new version is out"],
        "a yes finding nothing newer is confirmed"
    );
    assert_eq!(f.check_now(), ["Skwir is up to date"]);
    site.serve(&file("0.6.2"), "\"b\"");
    assert_eq!(f.check_now(), ["Skwir 0.6.2 is out"]);
    assert_eq!(
        f.check_now(),
        ["Skwir 0.6.2 is out"],
        "told, and said again"
    );
    site.serve(&file("0.6.3"), "\"c\"");
    assert_eq!(f.check_now(), ["Skwir 0.6.3 is out"], "the 14 days aside");
    site.serve("not json", "\"d\"");
    let failed = f.check_now();
    assert_eq!(failed.len(), 1);
    assert!(
        failed[0].starts_with("Skwir could not check for a new version"),
        "{failed:?}"
    );
    let host = site.address.trim_start_matches("http://");
    assert!(
        failed[0].contains(&format!("{host}'s answer was not understood")),
        "the site asked is named: {failed:?}"
    );
    assert_eq!(site.heads().len(), 6);
    // The daily check still waits the two weeks after what was told.
    site.serve(&file("0.6.4"), "\"e\"");
    f.later_by(61);
    f.look_on(20_001);
    f.settled();
    assert_eq!(site.heads().len(), 7);
    assert!(f.said().is_empty());
}

/// On a metered connection nothing is asked of the site, at a look or
/// after a yes; a check asked for now is made there all the same.
#[test]
fn a_metered_connection_is_asked_nothing_unless_asked_for_now() {
    let site = Site::new(&file("0.6.2"), "\"a\"");
    let f = Fixture::new(&site.address, Version(0, 5, 1), DOWNLOADED);
    f.metered.store(true, Ordering::SeqCst);
    f.look_on(20_000);
    f.look_on(20_002);
    assert!(f.said().pop().is_some_and(|question| question.stays));
    f.answer(true);
    f.settled();
    assert_eq!(f.texts(), ["Skwir will tell you when a new version is out"]);
    for day in 20_003..20_006 {
        f.later_by(61);
        f.look_on(day);
        f.settled();
    }
    assert!(site.heads().is_empty(), "never on a metered connection");
    assert!(f.said().is_empty());
    assert_eq!(f.check_now(), ["Skwir 0.6.2 is out"]);
    assert_eq!(site.heads().len(), 1);
    f.metered.store(false, Ordering::SeqCst);
    f.look_on(20_006);
    f.settled();
    assert_eq!(site.heads().len(), 2, "not metered: the day's check");
}

/// A check under way answers one asked for now meanwhile, with one
/// request; turned off while the site is waited for, it tells nothing.
#[test]
fn a_check_under_way_answers_who_asks_meanwhile_unless_turned_off() {
    let site = Site::new(&file("0.5.1"), "\"a\"");
    let f = Fixture::new(&site.address, Version(0, 5, 1), DOWNLOADED);
    f.answer(true);
    f.settled();
    f.said();
    let held = site.hold();
    f.look_on(20_001);
    f.wait_until("the site is asked", |_| site.heads().len() == 2);
    f.app.dispatch(&Invocation::new("updates.check")).unwrap();
    drop(held);
    f.settled();
    assert_eq!(f.texts(), ["Skwir is up to date"]);
    assert_eq!(site.heads().len(), 2, "one request");

    site.serve(&file("0.6.2"), "\"b\"");
    let held = site.hold();
    f.look_on(20_002);
    f.wait_until("the site is asked", |_| site.heads().len() == 3);
    f.app.settings.set(TELL, toml::Value::Boolean(false));
    drop(held);
    f.settled();
    assert!(f.said().is_empty() && f.news().is_empty());
    assert_eq!(f.kept().told, None);
}

/// A no is kept: nothing is asked of the site, nothing said, and the
/// question never comes back; turning the setting on later checks.
#[test]
fn a_no_asks_nothing_until_the_setting_is_turned_on() {
    let site = Site::new(&file("0.6.2"), "\"a\"");
    let f = Fixture::new(&site.address, Version(0, 5, 1), DOWNLOADED);
    f.look_on(20_000);
    f.look_on(20_002);
    let question = f.said().pop().unwrap();
    f.app.dispatch(&question.actions[1].command).unwrap();
    assert_eq!(
        f.said().pop().unwrap().text.english(),
        "Skwir will not ask again; Settings can turn it on"
    );
    for day in 20_003..20_010 {
        f.look_on(day);
    }
    f.settled();
    assert!(site.heads().is_empty() && f.said().is_empty() && f.news().is_empty());

    f.app.settings.set(TELL, toml::Value::Boolean(true));
    f.plugin.on_settings_changed(&f.app, &[TELL.to_owned()]);
    f.settled();
    assert_eq!(site.heads().len(), 1);
    assert_eq!(f.said().pop().unwrap().text.english(), "Skwir 0.6.2 is out");
}

/// The answer is the setting: a copy whose settings say it, the state
/// file lost (deleted, the settings carried to another machine), is never
/// asked; told already, it checks.
#[test]
fn a_setting_written_is_never_asked_whatever_the_state_lost() {
    let site = Site::new(&file("0.6.2"), "\"a\"");
    for tell in [false, true] {
        let f = Fixture::new(&site.address, Version(0, 5, 1), DOWNLOADED);
        f.app.settings.set(TELL, toml::Value::Boolean(tell));
        for day in 20_000..20_006 {
            f.look_on(day);
            f.settled();
        }
        let questions = f.said().iter().filter(|n| n.stays).count();
        assert_eq!(questions, 0, "tell = {tell}");
        assert!(f.kept().answered);
    }
    assert_eq!(
        site.heads().len(),
        6,
        "the copy told checks each day, the other never"
    );
}

/// The question closed unanswered comes back on another day, three days
/// at most.
#[test]
fn a_question_closed_unanswered_comes_back_three_days_at_most() {
    let site = Site::new(&file("0.6.2"), "\"a\"");
    let f = Fixture::new(&site.address, Version(0, 5, 1), DOWNLOADED);
    f.look_on(20_000);
    let mut asked = 0;
    for day in 20_002..20_012 {
        f.look_on(day);
        asked += f.said().iter().filter(|n| n.stays).count();
    }
    assert_eq!(asked, 3);
    assert!(site.heads().is_empty());
}

/// A copy a package manager updates (no channel written, or a `.deb` on
/// a system with Skwir's repository) asks nothing, is asked nothing, and
/// has no setting for it; a `.deb` without the repository is offered it.
#[test]
fn a_copy_something_else_updates_is_never_checked() {
    let site = Site::new(&file("0.6.2"), "\"a\"");
    for here in [
        Here {
            channel: None,
            repository: false,
            family: None,
        },
        Here {
            channel: Some(Channel::Deb),
            repository: true,
            family: None,
        },
    ] {
        let f = Fixture::new(&site.address, Version(0, 5, 1), here);
        for day in 20_000..20_010 {
            f.look_on(day);
        }
        assert!(f.said().is_empty(), "{here:?}");
        assert!(
            f.app.settings.get::<bool>(TELL).is_none(),
            "no setting: {here:?}"
        );
        assert!(
            !f.app.commands.contains("updates.check"),
            "nothing to check: {here:?}"
        );
        let latest = f.app.dispatch(&Invocation::new("updates.latest")).unwrap();
        let skwir_core::command::Outcome::Value(latest) = latest else {
            panic!("{latest:?}")
        };
        assert_eq!(latest["checked"], false);
    }
    assert!(site.heads().is_empty());

    let f = Fixture::new(
        &site.address,
        Version(0, 5, 1),
        Here {
            channel: Some(Channel::Deb),
            repository: false,
            family: Some(Family::Ubuntu),
        },
    );
    f.look_on(20_000);
    f.look_on(20_002);
    let question = f.said().pop().unwrap();
    let labels: Vec<String> = question.actions.iter().map(|a| a.label.english()).collect();
    assert_eq!(labels, ["Yes", "No", "Update with your system…"]);
    let answers: Vec<bool> = question.actions.iter().map(|a| a.answers).collect();
    assert_eq!(
        answers,
        [true, true, false],
        "the link leaves the question up"
    );
    let repository = &question.actions[2].command;
    assert_eq!(
        repository.args["url"],
        format!("{}/download?package=ubuntu#repository", site.address),
        "the page shows the system's commands"
    );
}

/// The repository's page names the system the copy runs on, read from its
/// os-release for a `.deb`: Ubuntu's for Ubuntu and what is like it,
/// Debian's for Debian and what is like it alone; the site guesses where
/// neither is said.
#[test]
fn the_repository_page_names_the_system_the_copy_runs_on() {
    let page = |channel: Channel, os: Option<&str>| {
        let family = Family::of(channel, || {
            os.map(skwir_platform::about::OsRelease::of_text)
        });
        crate::repository_page("https://skwir.org", family)
    };
    let package = |word: &str| format!("https://skwir.org/download?package={word}#repository");
    let guessed = "https://skwir.org/download#repository";
    assert_eq!(page(Channel::Rpm, None), package("fedora"), "an .rpm");
    for (system, os, expected) in [
        ("Ubuntu", "ID=ubuntu\nID_LIKE=debian\n", package("ubuntu")),
        (
            "Linux Mint",
            "ID=linuxmint\nID_LIKE=\"ubuntu debian\"\n",
            package("ubuntu"),
        ),
        (
            "Pop!_OS",
            "ID=pop\nID_LIKE=\"ubuntu debian\"\n",
            package("ubuntu"),
        ),
        ("Debian", "ID=debian\n", package("debian")),
        ("LMDE", "ID=linuxmint\nID_LIKE=debian\n", package("debian")),
        (
            "another",
            "ID=other\nID_LIKE=\"something else\"\n",
            guessed.to_owned(),
        ),
    ] {
        assert_eq!(page(Channel::Deb, Some(os)), expected, "{system}");
    }
    assert_eq!(page(Channel::Deb, None), guessed, "no os-release");
    assert_eq!(
        Family::of(Channel::AppImage, || panic!("read only for a .deb")),
        None
    );
}

/// What's new is said once after an update, never on a first run, and
/// not when its setting is off; with no request, on any copy.
#[test]
fn what_is_new_is_said_once_after_an_update() {
    let site = Site::new(&file("0.6.2"), "\"a\"");
    let managed = Here {
        channel: None,
        repository: false,
        family: None,
    };
    let f = Fixture::new(&site.address, Version(0, 5, 1), managed);
    f.look_on(20_000);
    assert!(f.said().is_empty(), "a first run says nothing");
    let mut kept = f.kept();
    kept.highest_run = Some("0.5.0".into());
    kept.write(&f.app.settings);
    // The next run of a newer version.
    f.plugin.on_disable(&f.app);
    f.app.unregister_owner(crate::ID);
    f.plugin.register(&f.app);
    f.wait_until("the copy's channel is read", |f| {
        f.plugin.inner.here().is_some()
    });
    f.look_on(20_001);
    let said = f.said();
    assert_eq!(said.len(), 1);
    assert_eq!(said[0].text.english(), "Skwir was updated to 0.5.1");
    assert_eq!(
        said[0].owner.as_deref(),
        Some(crate::ID),
        "the plugin's, gone with it when turned off"
    );
    assert_eq!(opens(&said[0]), format!("{}/releases/v0.5.1", site.address));
    f.look_on(20_001);
    assert!(f.said().is_empty(), "once");
    assert!(site.heads().is_empty(), "no request");

    // Turned off, nothing is said, and the version is still counted.
    let mut kept = f.kept();
    kept.highest_run = Some("0.5.0".into());
    kept.write(&f.app.settings);
    f.app.settings.set(WHATS_NEW, toml::Value::Boolean(false));
    f.plugin.on_disable(&f.app);
    f.app.unregister_owner(crate::ID);
    f.plugin.register(&f.app);
    f.wait_until("the copy's channel is read", |f| {
        f.plugin.inner.here().is_some()
    });
    f.look_on(20_002);
    assert!(f.said().is_empty());
    assert_eq!(f.kept().highest_run.as_deref(), Some("0.5.1"));
}

/// A site that fails says nothing, and is asked again at a later look an
/// hour after at the soonest, the same day too; once it answered, not
/// again that day.
#[test]
fn a_check_that_fails_says_nothing_and_is_made_again_an_hour_later() {
    let site = Site::new("not json", "\"a\"");
    let f = Fixture::new(&site.address, Version(0, 5, 1), DOWNLOADED);
    f.answer(true);
    f.settled();
    assert_eq!(f.texts(), ["Skwir will tell you when a new version is out"]);
    assert_eq!(f.kept().checked_on, None, "no answer, no day's check");
    f.later_by(59);
    f.look_on(20_000);
    f.settled();
    assert_eq!(site.heads().len(), 1, "not again within the hour");
    f.later_by(2);
    f.look_on(20_000);
    f.settled();
    assert_eq!(site.heads().len(), 2, "again an hour after");
    assert!(f.said().is_empty() && f.news().is_empty(), "said nothing");

    site.serve(&file("0.6.2"), "\"b\"");
    f.later_by(61);
    f.look_on(20_000);
    f.settled();
    assert_eq!(f.texts(), ["Skwir 0.6.2 is out"]);
    f.later_by(61);
    f.look_on(20_000);
    f.settled();
    assert_eq!(site.heads().len(), 3, "the day's check was made");
}

/// A `.deb` copy whose system comes to have Skwir's repository is asked
/// again at each look: from then on it checks no more, offers no setting,
/// and About says nothing of a version, until the repository goes again.
#[test]
fn a_copy_the_system_comes_to_update_is_checked_no_more() {
    let site = Site::new(&file("0.6.2"), "\"a\"");
    let repository = Arc::new(AtomicBool::new(false));
    let added = Arc::clone(&repository);
    let f = Fixture::with_here(
        &site.address,
        Version(0, 5, 1),
        Arc::new(move || Here {
            channel: Some(Channel::Deb),
            repository: added.load(Ordering::SeqCst),
            family: None,
        }),
    );
    assert!(
        f.app.commands.contains("updates.check"),
        "a downloaded copy's"
    );
    f.answer(true);
    f.settled();
    assert_eq!(f.news(), ["Skwir 0.6.2 is out"]);
    f.said();

    repository.store(true, Ordering::SeqCst);
    f.look_on(20_001);
    f.settled();
    assert_eq!(site.heads().len(), 1, "no check once the system updates it");
    assert!(f.news().is_empty() && f.said().is_empty());
    assert!(f.app.settings.spec(TELL).is_none(), "no setting");
    assert!(
        f.app.dispatch(&Invocation::new("updates.check")).is_err(),
        "the check says why it is not made"
    );

    repository.store(false, Ordering::SeqCst);
    f.look_on(20_002);
    f.settled();
    assert_eq!(site.heads().len(), 2, "downloaded again: checked");
    assert!(f.app.settings.spec(TELL).is_some());
    assert_eq!(f.news(), ["Skwir 0.6.2 is out"]);
}

/// Turned off while a check waits for the site, the plugin takes away
/// what it said and offered; the earlier run's check, ending after, says
/// nothing, and its failure holds back no check of the next run.
#[test]
fn turned_off_during_a_check_nothing_of_it_stays() {
    let site = Site::new(&file("0.6.2"), "\"a\"");
    let f = Fixture::new(&site.address, Version(0, 5, 1), DOWNLOADED);
    f.answer(true);
    f.settled();
    f.said();
    assert_eq!(f.news(), ["Skwir 0.6.2 is out"]);

    site.serve("not json", "\"b\"");
    let held = site.hold();
    f.look_on(20_001);
    f.wait_until("the site is asked", |_| site.heads().len() == 2);
    let earlier = f.plugin.inner.run.lock().thread.take();
    assert!(f.app.plugins.set_enabled(crate::ID, false, &f.app));
    assert!(f.news().is_empty(), "About says nothing of a plugin off");
    assert!(!f.app.commands.contains("updates.check"));
    assert!(f.app.settings.spec(TELL).is_none());

    assert!(f.app.plugins.set_enabled(crate::ID, true, &f.app));
    f.wait_until("the copy's channel is read", |f| {
        f.plugin.inner.here().is_some()
    });
    drop(held);
    earlier.expect("a check under way").join().unwrap();
    assert!(f.said().is_empty(), "the earlier run says nothing");
    assert_eq!(
        f.news(),
        ["Skwir 0.6.2 is out"],
        "said again by the next run"
    );

    site.serve(&file("0.6.2"), "\"c\"");
    f.look_on(20_001);
    f.settled();
    assert_eq!(
        site.heads().len(),
        3,
        "not held back an hour by the earlier run's failure"
    );
}

/// Pressed in the window, the answer waits there for nothing: it is kept
/// on a worker, even while what is kept is held (a save under way), and
/// says its confirmation once kept.
#[test]
fn an_answer_pressed_in_the_window_waits_there_for_nothing() {
    let site = Site::new(&file("0.5.1"), "\"a\"");
    let f = Fixture::new(&site.address, Version(0, 5, 1), DOWNLOADED);
    skwir_core::scheduler::mark_window_thread(&f.app.scheduler);
    let held = f.plugin.inner.kept.lock();
    f.answer(true);
    drop(held);
    f.wait_until("the answer is kept and checked", |f| {
        f.kept().answered && f.kept().checked_on.is_some()
    });
    f.settled();
    assert_eq!(f.texts(), ["Skwir will tell you when a new version is out"]);
    assert_eq!(f.app.settings.stored::<bool>(TELL), Some(true));
}

/// A settings file that cannot be written (Home Manager's) is asked
/// nothing; an answer it cannot keep is said, taken back, and not counted
/// as answered, and nothing is asked of the site.
#[cfg(unix)]
#[test]
fn an_answer_the_settings_cannot_keep_is_said_and_not_counted() {
    use std::os::unix::fs::PermissionsExt as _;
    let site = Site::new(&file("0.6.2"), "\"a\"");
    let folder = tempfile::tempdir().unwrap();
    let settings = folder.path().join("settings.toml");
    std::fs::write(&settings, "").unwrap();
    let f = Fixture::sharing(folder.path(), &site.address, Version(0, 5, 1));
    std::fs::set_permissions(&settings, std::fs::Permissions::from_mode(0o444)).unwrap();
    f.look_on(20_000);
    f.look_on(20_002);
    assert!(f.said().iter().all(|n| !n.stays), "nothing asked");

    f.answer(true);
    f.settled();
    let said = f.texts();
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(
        said[0].starts_with("Skwir could not keep your answer: "),
        "{said:?}"
    );
    assert_eq!(f.app.settings.stored::<bool>(TELL), None, "taken back");
    assert!(!f.kept().answered && !f.kept_on_disk().answered);
    assert!(site.heads().is_empty());
}

/// The question is asked, and counted, only at a look of a window of the
/// user's own: one answering another program's dialog (ADR-141) asks
/// nothing and spends none of its three days.
#[test]
fn a_question_waits_for_a_window_of_the_users_own() {
    let site = Site::new(&file("0.6.2"), "\"a\"");
    let f = Fixture::new(&site.address, Version(0, 5, 1), DOWNLOADED);
    f.look_on(20_000);
    f.app.workspace.write().set_chooses(f.window, true);
    f.look_on(20_002);
    assert!(f.said().iter().all(|n| !n.stays), "nothing asked");
    assert_eq!(f.kept().asked, 0, "nothing counted");
    f.app.workspace.write().set_chooses(f.window, false);
    f.look_on(20_002);
    assert!(f.said().iter().any(|n| n.stays), "asked");
    assert_eq!(f.kept().asked, 1);
}

/// A window answering another program's dialog (ADR-141) is told
/// nothing: What's new, the question and the day's check (and the version
/// it would tell) wait for a look at a window of the user's own.
#[test]
fn a_dialogs_window_is_told_nothing() {
    let site = Site::new(&file("0.6.2"), "\"a\"");
    let f = Fixture::new(&site.address, Version(0, 5, 1), DOWNLOADED);
    let mut kept = f.kept();
    kept.highest_run = Some("0.5.0".into());
    kept.first_seen = Some(20_000);
    kept.write(&f.app.settings);
    f.app.settings.set(TELL, toml::Value::Boolean(true));
    f.app.workspace.write().set_chooses(f.window, true);
    f.look_on(20_002);
    f.settled();
    assert!(f.said().is_empty(), "nothing said in a dialog's window");
    assert!(site.heads().is_empty(), "no check");

    f.app.workspace.write().set_chooses(f.window, false);
    f.look_on(20_002);
    f.settled();
    assert_eq!(
        f.texts(),
        ["Skwir was updated to 0.5.1", "Skwir 0.6.2 is out"],
        "said at the look of a window of the user's own"
    );
    assert_eq!(site.heads().len(), 1);
}

/// A copy of a `.deb` as the question's link leaves it: the repository
/// added, the question it asked is made moot.
fn deb_copy(site: &Site) -> (Fixture, Arc<AtomicBool>, Notification) {
    let repository = Arc::new(AtomicBool::new(false));
    let added = Arc::clone(&repository);
    let f = Fixture::with_here(
        &site.address,
        Version(0, 5, 1),
        Arc::new(move || Here {
            channel: Some(Channel::Deb),
            repository: added.load(Ordering::SeqCst),
            family: None,
        }),
    );
    f.look_on(20_000);
    f.look_on(20_002);
    let question = f.said().pop().expect("asked");
    assert!(question.stays);
    (f, repository, question)
}

/// The question goes once it is answered elsewhere: the setting chosen in
/// Settings, or the copy one its system now updates, seen at a look.
#[test]
fn a_question_answered_in_settings_or_made_moot_is_taken_back() {
    let site = Site::new(&file("0.5.1"), "\"a\"");
    let (f, _, question) = deb_copy(&site);
    let id = question.question.expect("a question of its owner's");
    assert!(f.app.question_stands(crate::ID, id));
    f.app.settings.set(TELL, toml::Value::Boolean(true));
    f.plugin.on_settings_changed(&f.app, &[TELL.to_owned()]);
    f.settled();
    assert!(!f.app.question_stands(crate::ID, id), "chosen in Settings");

    let (f, repository, question) = deb_copy(&site);
    let id = question.question.expect("a question of its owner's");
    repository.store(true, Ordering::SeqCst);
    f.look_on(20_002);
    assert!(
        !f.app.question_stands(crate::ID, id),
        "the system updates it"
    );
}

/// Yes or No on a question made moot before a look saw it (the repository
/// added from Skwir's own terminal) only takes it down: nothing written,
/// nothing asked of the site, nothing said.
#[test]
fn an_answer_to_a_moot_question_only_takes_it_down() {
    let site = Site::new(&file("0.6.2"), "\"a\"");
    for button in [0, 1] {
        let (f, repository, question) = deb_copy(&site);
        let id = question.question.expect("a question of its owner's");
        repository.store(true, Ordering::SeqCst);
        f.app.dispatch(&question.actions[button].command).unwrap();
        f.settled();
        assert!(!f.app.question_stands(crate::ID, id), "button {button}");
        assert!(f.said().is_empty(), "button {button}");
        assert_eq!(f.app.settings.stored::<bool>(TELL), None);
        assert!(!f.kept().answered);
    }
    assert!(site.heads().is_empty());
}

/// A check started for a copy its system came to update since it was last
/// read asks the site nothing: asked for now, it says why.
#[test]
fn a_check_for_a_copy_its_system_now_updates_asks_nothing() {
    let site = Site::new(&file("0.6.2"), "\"a\"");
    let (f, repository, question) = deb_copy(&site);
    f.app.dispatch(&question.actions[0].command).unwrap();
    f.settled();
    assert_eq!(f.texts(), ["Skwir 0.6.2 is out"]);
    repository.store(true, Ordering::SeqCst);
    f.app.dispatch(&Invocation::new("updates.check")).unwrap();
    f.settled();
    let said = f.said();
    assert_eq!(said.len(), 1);
    assert!(
        said[0]
            .text
            .english()
            .starts_with("This copy of Skwir is not checked"),
        "{:?}",
        said[0].text.english()
    );
    assert_eq!(site.heads().len(), 1, "only the yes's check");
}

/// Two releases on one state file (an older one still running, ADR-115)
/// take back nothing of what the other kept: What's new is said once, the
/// question once a day for both.
#[test]
fn two_copies_on_one_state_file_take_back_nothing_of_each_other() {
    let site = Site::new(&file("0.6.2"), "\"a\"");
    let folder = tempfile::tempdir().unwrap();
    let older = Fixture::sharing(folder.path(), &site.address, Version(0, 6, 0));
    older.look_on(20_000);
    older.wait_until("its first run is kept", |f| {
        f.kept_on_disk().highest_run.as_deref() == Some("0.6.0")
    });

    let newer = Fixture::sharing(folder.path(), &site.address, Version(0, 6, 1));
    newer.look_on(20_001);
    assert_eq!(newer.texts(), ["Skwir was updated to 0.6.1"]);
    newer.wait_until("the newer run is kept", |f| {
        f.kept_on_disk().highest_run.as_deref() == Some("0.6.1")
    });

    // The older asks on its day, keeping what the newer kept.
    older.look_on(20_002);
    assert!(older.said().iter().any(|n| n.stays), "asked");
    older.wait_until("the question is kept", |f| f.kept_on_disk().asked == 1);
    let kept = older.kept_on_disk();
    assert_eq!(kept.highest_run.as_deref(), Some("0.6.1"), "{kept:?}");
    assert_eq!(kept.first_seen, Some(20_000));

    // Asked today already, by the other.
    newer.look_on(20_002);
    assert!(newer.said().is_empty(), "asked once a day for both");

    // The newer started again says nothing of what is new.
    drop(newer);
    let again = Fixture::sharing(folder.path(), &site.address, Version(0, 6, 1));
    again.look_on(20_003);
    assert!(
        again
            .texts()
            .iter()
            .all(|text| !text.starts_with("Skwir was updated")),
        "What's new said once"
    );
}
