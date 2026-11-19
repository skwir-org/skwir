//! The plugin against a site of the tests' own: a plain HTTP server on
//! this machine answering what the test sets, and a calendar the test
//! moves. What is asked, when, and what is told.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
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
/// naming the file's tag), keeping the head of each request.
struct Site {
    address: String,
    file: Arc<Mutex<File>>,
    heads: Arc<Mutex<Vec<String>>>,
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
        let (served, kept) = (Arc::clone(&file), Arc::clone(&heads));
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
                let file = served.lock().clone();
                let wanted = format!("if-none-match: {}", file.etag);
                let same = head
                    .to_ascii_lowercase()
                    .contains(&wanted.to_ascii_lowercase());
                let found = head.starts_with("GET /updates/1.json ");
                kept.lock().push(head);
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
        }
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
/// copy that came as `here`, asking `site`, on the test's day.
struct Fixture {
    _folder: tempfile::TempDir,
    app: Arc<App>,
    plugin: UpdatesPlugin,
    day: Arc<AtomicI64>,
}

impl Fixture {
    fn new(site: &str, running: Version, here: Here) -> Fixture {
        let folder = tempfile::tempdir().unwrap();
        let app = App::new(skwir_core::app::AppConfig::ephemeral(folder.path())).unwrap();
        let day = Arc::new(AtomicI64::new(20_000));
        let today = Arc::clone(&day);
        let plugin = UpdatesPlugin::with(Setup {
            site: site.into(),
            running: Some(running),
            here: Arc::new(move || here),
            today: Arc::new(move || today.load(Ordering::SeqCst)),
            metered: Arc::new(|| None),
        });
        plugin.register(&app);
        let fixture = Fixture {
            _folder: folder,
            app,
            plugin,
            day,
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

    /// The checks under way are done.
    fn settled(&self) {
        self.wait_until("the check ends", |f| !f.plugin.inner.run.lock().checking);
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
/// checks at once and tells of the newer version, kept said in About
/// until updated; the next days ask again, conditionally, and an answer
/// unchanged tells nothing more. The request says nothing about the copy.
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
    let said: Vec<String> = f.said().iter().map(|n| n.text.english()).collect();
    assert_eq!(
        said,
        [
            "Skwir will tell you when a new version is out",
            "Skwir 0.6.2 is out"
        ]
    );
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
/// important one at once, said as such, and its page opened.
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

/// A site that fails says nothing, and is asked again at a later look
/// only an hour after.
#[test]
fn a_check_that_fails_says_nothing() {
    let site = Site::new("not json", "\"a\"");
    let f = Fixture::new(&site.address, Version(0, 5, 1), DOWNLOADED);
    f.app
        .dispatch(&Invocation::with_args(
            "updates.answer",
            &serde_json::json!({ "tell": true }),
        ))
        .unwrap();
    f.settled();
    let said: Vec<String> = f.said().iter().map(|n| n.text.english()).collect();
    assert_eq!(said, ["Skwir will tell you when a new version is out"]);
    assert_eq!(f.kept().checked_on, None, "no answer, no day's check");
    f.look_on(20_001);
    f.settled();
    assert_eq!(site.heads().len(), 1, "not again within the hour");
    assert!(f.news().is_empty());
}
