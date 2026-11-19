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
use crate::{Here, Setup, TELL, UpdatesPlugin, WHATS_NEW};

/// What the site answers: its file and the file's tag.
#[derive(Clone)]
struct File {
    body: String,
    etag: String,
}

/// A site answering `/updates/1.json` with its file (304 to a request
/// naming the file's tag), keeping the head of each request; held, it
/// answers once let go (`hold`).
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
    _folder: tempfile::TempDir,
    app: Arc<App>,
    plugin: UpdatesPlugin,
    day: Arc<AtomicI64>,
    minutes: Arc<AtomicU64>,
    metered: Arc<AtomicBool>,
}

impl Fixture {
    fn new(site: &str, running: Version, here: Here) -> Fixture {
        let folder = tempfile::tempdir().unwrap();
        let app = App::new(skwir_core::app::AppConfig::ephemeral(folder.path())).unwrap();
        let day = Arc::new(AtomicI64::new(20_000));
        let minutes = Arc::new(AtomicU64::new(0));
        let metered = Arc::new(AtomicBool::new(false));
        let (today, elapsed, costly) =
            (Arc::clone(&day), Arc::clone(&minutes), Arc::clone(&metered));
        let started = Instant::now();
        let plugin = UpdatesPlugin::with(Setup {
            site: site.into(),
            running: Some(running),
            here: Arc::new(move || here),
            today: Arc::new(move || today.load(Ordering::SeqCst)),
            metered: Arc::new(move || Some(costly.load(Ordering::SeqCst))),
            now: Arc::new(move || started + Duration::from_mins(elapsed.load(Ordering::SeqCst))),
        });
        plugin.register(&app);
        let fixture = Fixture {
            _folder: folder,
            app,
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

    fn look_on(&self, day: i64) {
        self.day.store(day, Ordering::SeqCst);
        self.plugin.on_looked_at(&self.app);
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
}

const DOWNLOADED: Here = Here {
    channel: Some(Channel::AppImage),
    repository: false,
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
/// again, conditionally, and an answer unchanged tells nothing more. The
/// request says nothing about the copy.
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
    assert!(!head.contains("if-none-match"), "nothing to compare yet");

    // Again the same day: no request. The next day: asked conditionally,
    // and the answer unchanged tells nothing again.
    f.look_on(20_002);
    f.settled();
    assert_eq!(site.heads().len(), 1);
    f.look_on(20_003);
    f.settled();
    let heads = site.heads();
    assert_eq!(heads.len(), 2);
    assert!(
        heads[1]
            .to_ascii_lowercase()
            .contains("if-none-match: \"a\"")
    );
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
        },
        Here {
            channel: Some(Channel::Deb),
            repository: true,
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
        assert!(f.app.dispatch(&Invocation::new("updates.check")).is_err());
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
        format!("{}/download#repository", site.address)
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
