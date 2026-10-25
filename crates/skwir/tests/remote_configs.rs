//! Servers' configurations changed from Skwir (ADR-086), as the window
//! has them: rclone's remotes added, edited, tested, signed in and removed
//! through the forms the plugin asks the window to fill (answered here as
//! the window would), in a configuration of the test's own, against
//! servers on this machine. Never the user's rclone configuration, ssh
//! agent or browser: each test names its own.
//!
//! Skipped, saying why, where rclone is missing.
#![cfg(target_os = "linux")]

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use skwir_core::app::{App, AppConfig};
use skwir_core::command::Invocation;
use skwir_core::form::{Answer, Asked, Kind, Values};
use skwir_plugin_remote::address::Address;
use skwir_plugin_remote::connector::Connector;
use skwir_plugin_remote::passwords::{self, Keyring};
use skwir_plugin_remote::rclone::Rclone;
use skwir_plugin_remote::{RemotePlugin, Remotes};
use std::sync::Mutex;

/// A folder of the test's on the home's file system, never `/tmp`.
fn fixture(name: &str) -> PathBuf {
    let base = dirs::home_dir()
        .unwrap()
        .join(".cache/skwir-tests/remote")
        .join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    base
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// `rclone serve <kind>` of `root` on a port of its own, stopped when
/// dropped.
struct Served {
    child: Child,
    port: u16,
}

impl Served {
    fn start(kind: &str, root: &Path, extra: &[&str]) -> Served {
        let port = free_port();
        let child = Command::new("rclone")
            .args(["serve", kind])
            .arg(root)
            .args(["--addr", &format!("127.0.0.1:{port}")])
            .args(extra)
            .arg("--config")
            .arg(root.with_extension("conf"))
            .stdout(Stdio::null())
            .stderr(std::fs::File::create(root.with_extension(format!("{kind}.log"))).unwrap())
            .spawn()
            .expect("start rclone serve");
        let deadline = Instant::now() + Duration::from_secs(20);
        while std::net::TcpStream::connect(("127.0.0.1", port)).is_err() {
            assert!(Instant::now() < deadline, "the server never listened");
            std::thread::sleep(Duration::from_millis(50));
        }
        Served { child, port }
    }
}

impl Drop for Served {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The window, as a test has it: the forms asked, answered in turn.
struct Window<'a> {
    app: &'a App,
    asked: std::cell::RefCell<std::collections::VecDeque<Asked>>,
}

impl<'a> Window<'a> {
    fn new(app: &'a App) -> Self {
        app.set_window_shown();
        Window {
            app,
            asked: std::cell::RefCell::default(),
        }
    }

    /// The next form asked whose asker still waits, within `seconds`; one
    /// that only waits (rclone working) is waited out, as a user would.
    fn form(&self, seconds: u64) -> Asked {
        self.next(seconds, |asked| asked.form.waiting.is_none())
    }

    /// The next form asked that `wanted` takes, whose asker still waits.
    fn next(&self, seconds: u64, wanted: impl Fn(&Asked) -> bool) -> Asked {
        let deadline = Instant::now() + Duration::from_secs(seconds);
        loop {
            self.app.pump();
            self.asked.borrow_mut().extend(self.app.take_forms());
            let next = {
                let mut asked = self.asked.borrow_mut();
                asked.retain(|a| self.app.form_waits(a.id));
                let at = asked.iter().position(&wanted);
                at.and_then(|at| asked.remove(at))
            };
            if let Some(next) = next {
                return next;
            }
            assert!(
                Instant::now() < deadline,
                "no form was asked; said: {:?}",
                said(self.app)
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// Answer `asked` with `values` over what its fields hold.
    fn accept(&self, asked: &Asked, values: &[(&str, &str)]) {
        assert!(
            self.app
                .answer_form(asked.id, Some(Answer::Accepted(filled(asked, values))))
        );
    }

    fn back(&self, asked: &Asked) {
        assert!(
            self.app
                .answer_form(asked.id, Some(Answer::Back(filled(asked, &[]))))
        );
    }

    fn cancel(&self, asked: &Asked) {
        assert!(self.app.answer_form(asked.id, None));
    }
}

/// What a form's fields hold as it opened, `values` written over.
fn filled(asked: &Asked, values: &[(&str, &str)]) -> Values {
    let mut filled = Values::default();
    for field in &asked.form.fields {
        filled.set(field.key.clone(), field.value.clone());
    }
    for (key, value) in values {
        assert!(
            asked.form.fields.iter().any(|f| f.key == *key),
            "“{}” has no field {key}",
            asked.form.title
        );
        filled.set(*key, *value);
    }
    filled
}

/// What the app said since asked last.
fn said(app: &App) -> Vec<String> {
    app.take_notifications()
        .into_iter()
        .map(|n| n.text)
        .collect()
}

/// Until the app says something containing `words`, within `seconds`.
fn until_said(app: &App, seconds: u64, words: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(seconds);
    let mut heard = Vec::new();
    loop {
        app.pump();
        heard.extend(said(app));
        if let Some(found) = heard.iter().find(|s| s.contains(words)) {
            return found.clone();
        }
        assert!(
            Instant::now() < deadline,
            "never said “{words}”: {heard:?}; forms asked: {:?}",
            app.take_forms()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// A keyring of the test's own, in memory.
#[derive(Default)]
struct TestKeyring {
    own: Mutex<std::collections::HashMap<String, String>>,
}

impl Keyring for TestKeyring {
    fn own(&self, server: &str) -> Option<String> {
        self.own.lock().unwrap().get(server).cloned()
    }
    fn desktop(&self, _: &[(&str, &str)]) -> Option<String> {
        None
    }
    fn keep(&self, server: &str, _: &str, password: &str) -> anyhow::Result<()> {
        self.own
            .lock()
            .unwrap()
            .insert(server.to_owned(), password.to_owned());
        Ok(())
    }
    fn forget(&self, server: &str) -> anyhow::Result<()> {
        self.own.lock().unwrap().remove(server);
        Ok(())
    }
}

/// The app with the remote plugin, rclone given `config` alone, the
/// password of an encrypted one and servers' passwords kept in `keyring`.
fn app_with_rclone(base: &Path, config: &Path, keyring: Arc<TestKeyring>) -> Arc<App> {
    let app = App::new(AppConfig::ephemeral(base.join("app"))).unwrap();
    let servers: Arc<dyn Keyring> = keyring.clone();
    let rclone = Rclone {
        config: Some(config.to_path_buf()),
        askpass: Some(PathBuf::from(env!("CARGO_BIN_EXE_skwir"))),
        keyring: Some(keyring),
    };
    let remotes = Remotes::with(vec![Arc::new(rclone.clone())])
        .with_rclone(rclone)
        .with_keyring(servers);
    app.plugins.add(Arc::new(RemotePlugin::with(remotes)), &app);
    app
}

/// The section of `config` naming remote `name`, as `key = value` lines.
fn section(config: &Path, name: &str) -> Option<Vec<String>> {
    let text = std::fs::read_to_string(config).ok()?;
    let mut lines = text.lines();
    lines.find(|l| l.trim() == format!("[{name}]"))?;
    Some(
        lines
            .take_while(|l| !l.starts_with('['))
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(str::to_owned)
            .collect(),
    )
}

#[test]
fn an_rclone_remote_is_added_edited_tested_and_removed_through_its_forms() {
    if let Some(why) = Rclone::default().unavailable() {
        eprintln!("skipped: {why}");
        return;
    }
    let base = fixture("configs");
    let served_root = base.join("served");
    std::fs::create_dir_all(served_root.join("a folder")).unwrap();
    let secret = "it's s3cret, \"quoted\"";
    let served = Served::start("webdav", &served_root, &["--user", "me", "--pass", secret]);
    let config = base.join("rclone.conf");
    let app = app_with_rclone(&base, &config, Arc::default());
    let window = Window::new(&app);
    let url = format!("http://127.0.0.1:{}", served.port);

    // Add Server…, what kind of storage first: a long list, picked from.
    app.dispatch(&Invocation::new("remote.add_server")).unwrap();
    let kinds = window.form(30);
    let Kind::Pick { choices } = &kinds.form.fields[0].kind else {
        panic!("a pick of kinds: {:?}", kinds.form);
    };
    assert!(
        choices
            .iter()
            .any(|c| c.value == "webdav" && c.section == "Storage")
    );
    assert!(
        choices
            .iter()
            .any(|c| c.value == "crypt" && c.section == "On other remotes")
    );
    assert!(!choices.iter().any(|c| c.value == "memory"));
    window.accept(&kinds, &[("picked", "webdav")]);

    // Its options: the secret masked, the advanced folded.
    let options = window.form(30);
    let field = |key: &str| options.form.fields.iter().find(|f| f.key == key).unwrap();
    assert_eq!(field("pass").kind, Kind::Secret { kept: false });
    assert!(field("url").required);
    assert!(field("pacer_min_sleep").advanced);
    // A name rclone takes no remote by is refused, the form kept.
    window.accept(
        &options,
        &[
            ("remote.name", "my nas's"),
            ("url", &url),
            ("user", "me"),
            ("pass", secret),
        ],
    );
    let refused = window.form(30);
    assert!(
        refused
            .form
            .error
            .as_deref()
            .unwrap()
            .contains("letters, digits, spaces and _ - . + @ only")
    );
    assert_eq!(
        refused
            .form
            .fields
            .iter()
            .find(|f| f.key == "url")
            .unwrap()
            .value,
        url,
        "what was typed is kept"
    );
    assert_eq!(
        refused
            .form
            .fields
            .iter()
            .find(|f| f.key == "pass")
            .unwrap()
            .value,
        "",
        "never a secret"
    );
    window.accept(
        &refused,
        &[
            ("remote.name", "nas-1"),
            ("url", &url),
            ("user", "me"),
            ("pass", secret),
        ],
    );
    until_said(&app, 30, "“nas-1” is added");
    let kept = section(&config, "nas-1").expect("in rclone's configuration");
    assert!(kept.contains(&"type = webdav".to_owned()), "{kept:?}");
    assert!(kept.contains(&format!("url = {url}")), "{kept:?}");
    let pass = kept
        .iter()
        .find(|l| l.starts_with("pass = "))
        .expect("its password");
    assert!(
        !pass.contains("s3cret"),
        "kept obscured, as rclone keeps it"
    );
    assert!(
        !std::fs::read_to_string(&config)
            .unwrap()
            .contains("pacer_min_sleep"),
        "a default left as it is stays rclone's"
    );

    // Listed in the Network section and in Settings, by rclone's word.
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        app.pump();
        let place = app.registry.places.find(|p| p.title.starts_with("nas-1"));
        if let Some(place) = place {
            let titles: Vec<&str> = place.menu.iter().map(|i| i.title.as_str()).collect();
            assert!(titles.contains(&"Edit Server…"), "{titles:?}");
            assert!(titles.contains(&"Remove Server…"), "{titles:?}");
            assert!(
                !titles.contains(&"Sign In Again…"),
                "WebDAV signs in with its password"
            );
            break;
        }
        assert!(Instant::now() < deadline, "never listed");
        std::thread::sleep(Duration::from_millis(50));
    }
    let record = record_of(&app, "nas-1", 30);
    assert_eq!(
        record["kind"], "webdav · in rclone's configuration",
        "{record}"
    );
    assert_eq!(record["actions"][0]["label"], "Test");

    // Edited: its secret never shown, kept unless another is typed; what
    // changed alone given.
    app.dispatch(&Invocation::with_args(
        "remote.edit_server",
        &serde_json::json!({ "id": "nas-1" }),
    ))
    .unwrap();
    let edit = window.form(30);
    assert_eq!(edit.form.title, "Edit “nas-1”");
    let pass_field = edit.form.fields.iter().find(|f| f.key == "pass").unwrap();
    assert_eq!(pass_field.kind, Kind::Secret { kept: true });
    assert_eq!(pass_field.value, "");
    window.accept(&edit, &[("pacer_min_sleep", "20ms")]);
    until_said(&app, 30, "“nas-1” is saved");
    let kept_now = section(&config, "nas-1").unwrap();
    assert!(
        kept_now.contains(&"pacer_min_sleep = 20ms".to_owned()),
        "{kept_now:?}"
    );
    assert!(kept_now.contains(pass), "the password as it was");

    // A wrong password: rclone cannot list it, the user goes back and
    // types the right one.
    app.dispatch(&Invocation::with_args(
        "remote.edit_server",
        &serde_json::json!({ "id": "nas-1" }),
    ))
    .unwrap();
    let edit = window.form(30);
    window.accept(&edit, &[("pass", "wrong")]);
    let failed = window.form(60);
    assert_eq!(failed.form.title, "“nas-1” Does Not Answer");
    assert!(failed.form.intro.contains("401"), "{}", failed.form.intro);
    window.back(&failed);
    let again = window.form(30);
    window.accept(&again, &[("pass", secret)]);
    until_said(&app, 60, "“nas-1” is saved");

    // Tested: it answers.
    app.dispatch(&Invocation::with_args(
        "remote.test_server",
        &serde_json::json!({ "id": "nas-1" }),
    ))
    .unwrap();
    until_said(&app, 30, "“nas-1” answers");

    // An agent may start the form, never with a secret.
    app.dispatch(&Invocation::with_args(
        "remote.add_server",
        &serde_json::json!({ "kind": "webdav", "options": { "pass": "given" } }),
    ))
    .unwrap();
    assert!(until_said(&app, 30, "was not added").contains("the user types it"));
    app.dispatch(&Invocation::with_args(
        "remote.add_server",
        &serde_json::json!({ "kind": "webdav", "name": "from-agent", "options": { "url": url } }),
    ))
    .unwrap();
    let prefilled = window.form(30);
    assert_eq!(
        prefilled
            .form
            .fields
            .iter()
            .find(|f| f.key == "url")
            .unwrap()
            .value,
        url
    );
    window.cancel(&prefilled);
    assert!(
        section(&config, "from-agent").is_none(),
        "nothing until the user adds it"
    );

    // Removed, once the user says so.
    app.dispatch(&Invocation::with_args(
        "remote.remove_server",
        &serde_json::json!({ "id": "nas-1" }),
    ))
    .unwrap();
    let asked = window.form(30);
    assert_eq!(asked.form.title, "Remove “nas-1”?");
    window.accept(&asked, &[]);
    until_said(&app, 30, "“nas-1” is removed");
    assert!(section(&config, "nas-1").is_none());
    drop(served);
    std::fs::remove_dir_all(&base).unwrap();
}

/// The window adds a remote of the served WebDAV at `url`, named `name`.
fn add_webdav(app: &App, window: &Window<'_>, name: &str, url: &str) {
    app.dispatch(&Invocation::with_args(
        "remote.add_server",
        &serde_json::json!({ "kind": "webdav" }),
    ))
    .unwrap();
    let options = window.form(30);
    window.accept(&options, &[("remote.name", name), ("url", url)]);
}

/// rclone keeps its remotes' secrets only obscured: encrypting its
/// configuration is offered once one is added, with a password typed
/// twice and kept in the keyring, from which Skwir opens it after; one the
/// keyring does not keep is asked of the window.
#[test]
fn rclone_s_configuration_is_encrypted_and_opened_with_the_keyring_s_password() {
    if let Some(why) = Rclone::default().unavailable() {
        eprintln!("skipped: {why}");
        return;
    }
    let base = fixture("encrypted");
    let served_root = base.join("served");
    std::fs::create_dir_all(&served_root).unwrap();
    let served = Served::start("webdav", &served_root, &[]);
    let config = base.join("rclone.conf");
    let keyring = Arc::new(TestKeyring::default());
    let app = app_with_rclone(&base, &config, Arc::clone(&keyring));
    let window = Window::new(&app);
    let url = format!("http://127.0.0.1:{}", served.port);
    add_webdav(&app, &window, "plain", &url);
    let deadline = Instant::now() + Duration::from_secs(30);
    let offered = loop {
        app.pump();
        if let Some(found) = app
            .take_notifications()
            .into_iter()
            .find(|n| n.text.contains("“plain” is added"))
        {
            break found;
        }
        assert!(Instant::now() < deadline, "never added");
        std::thread::sleep(Duration::from_millis(20));
    };
    let action = offered.action.expect("encrypting offered");
    assert_eq!(action.command.name.as_ref(), "remote.encrypt_rclone_config");

    // Typed twice: two that differ are refused.
    app.dispatch(&action.command).unwrap();
    let asked = window.form(30);
    window.accept(&asked, &[("password", "one"), ("again", "other")]);
    let again = window.form(30);
    assert_eq!(
        again.form.error.as_deref(),
        Some("The two passwords differ")
    );
    let password = "a pass phrase, it's \"mine\"";
    window.accept(
        &again,
        &[
            ("password", password),
            ("again", password),
            ("keep", "true"),
        ],
    );
    until_said(&app, 60, "rclone's configuration is encrypted");
    let text = std::fs::read_to_string(&config).unwrap();
    assert!(
        text.starts_with("# Encrypted rclone configuration File"),
        "{text}"
    );
    assert!(!text.contains("plain"), "nothing of it readable");
    assert_eq!(
        keyring.own("rclone:configuration").as_deref(),
        Some(password)
    );

    // Its remotes are still listed, opened with the keyring's password.
    app.dispatch(&Invocation::new("remote.refresh")).unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    while app
        .registry
        .places
        .find(|p| p.title.starts_with("plain"))
        .is_none()
    {
        app.pump();
        assert!(Instant::now() < deadline, "never listed");
        std::thread::sleep(Duration::from_millis(50));
    }
    // A form opens it the same way, asking nothing.
    app.dispatch(&Invocation::with_args(
        "remote.edit_server",
        &serde_json::json!({ "id": "plain" }),
    ))
    .unwrap();
    let edit = window.form(30);
    assert_eq!(edit.form.title, "Edit “plain”");
    window.cancel(&edit);
    assert!(app.take_secret_questions().is_empty());

    // Not kept, it is asked of the window.
    keyring.forget("rclone:configuration").unwrap();
    app.dispatch(&Invocation::with_args(
        "remote.edit_server",
        &serde_json::json!({ "id": "plain" }),
    ))
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    let question = loop {
        app.pump();
        if let Some(question) = app.take_secret_questions().pop() {
            break question;
        }
        assert!(Instant::now() < deadline, "the password was never asked");
        std::thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(question.title, "Password of rclone's configuration");
    assert!(app.answer_secret(question.id, Some(password.into())));
    let edit = window.form(30);
    assert_eq!(edit.form.title, "Edit “plain”");
    window.cancel(&edit);
    drop(served);
    std::fs::remove_dir_all(&base).unwrap();
}

/// A browser of the test's own, first on the `PATH` rclone is run with: it
/// notes the page it is asked to open, and opens nothing.
fn fake_browser(base: &Path) -> PathBuf {
    let bin = base.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let opened = base.join("opened.txt");
    let script = bin.join("xdg-open");
    std::fs::write(
        &script,
        format!("#!/bin/sh\necho \"$1\" >> '{}'\n", opened.display()),
    )
    .unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let path = std::env::var_os("PATH").unwrap_or_default();
    let mut paths = vec![bin];
    paths.extend(std::env::split_paths(&path));
    // SAFETY: nextest runs each test in a process of its own, and this
    // runs before the test starts an app or a program, so no other thread
    // reads the environment meanwhile.
    #[allow(unsafe_code)]
    unsafe {
        std::env::set_var("PATH", std::env::join_paths(paths).unwrap());
    }
    opened
}

/// A sign-in through the browser (Google Drive's): rclone's questions
/// are asked one form each, its browser opened by rclone itself (the
/// test's, which opens nothing), a form waiting with the sign-in's page to
/// open again; its Cancel stops the sign-in, and a remote half made is not
/// kept. Sign In Again asks for a new sign-in where one was made. Nothing
/// reaches Google: the page is rclone's own, on this machine, and no
/// browser visits it.
#[test]
fn a_browser_s_sign_in_waits_with_its_page_and_is_cancelled() {
    if let Some(why) = Rclone::default().unavailable() {
        eprintln!("skipped: {why}");
        return;
    }
    let base = fixture("sign-in");
    let opened = fake_browser(&base);
    let config = base.join("rclone.conf");
    let app = app_with_rclone(&base, &config, Arc::default());
    let window = Window::new(&app);

    app.dispatch(&Invocation::with_args(
        "remote.add_server",
        &serde_json::json!({ "kind": "drive" }),
    ))
    .unwrap();
    let options = window.form(30);
    assert!(
        !options.form.fields.iter().any(|f| f.key == "token"),
        "the sign-in fills its token"
    );
    window.accept(&options, &[("remote.name", "drive-1")]);
    // rclone's own questions, until the browser's: each answered as it
    // offers.
    let waiting = loop {
        let asked = window.next(60, |_| true);
        if asked.form.waiting.is_some() && asked.form.fields.iter().any(|f| f.kind == Kind::Link) {
            break asked;
        }
        if asked.form.waiting.is_none() {
            assert_eq!(asked.form.title, "Set Up “drive-1”");
            window.accept(&asked, &[]);
        }
    };
    let page = &waiting.form.fields[0].value;
    assert!(
        page.starts_with("http://127.0.0.1:53682/auth?state="),
        "{page}"
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    while !std::fs::read_to_string(&opened).is_ok_and(|o| o.trim() == page) {
        assert!(
            Instant::now() < deadline,
            "the browser was never asked to open it"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    window.cancel(&waiting);
    let deadline = Instant::now() + Duration::from_secs(30);
    while section(&config, "drive-1").is_some() {
        app.pump();
        assert!(Instant::now() < deadline, "a remote half made is kept");
        std::thread::sleep(Duration::from_millis(50));
    }

    // Signed in once (a token rclone keeps), it signs in again on asking.
    let token = r#"{"access_token":"none","token_type":"Bearer","refresh_token":"none","expiry":"2020-01-01T00:00:00Z"}"#;
    std::fs::write(
        &config,
        format!("[drive-2]\ntype = drive\nscope = drive\ntoken = {token}\n"),
    )
    .unwrap();
    app.dispatch(&Invocation::with_args(
        "remote.reconnect",
        &serde_json::json!({ "id": "drive-2" }),
    ))
    .unwrap();
    let waiting = loop {
        let asked = window.next(60, |_| true);
        if asked.form.waiting.is_some() && asked.form.fields.iter().any(|f| f.kind == Kind::Link) {
            break asked;
        }
        if asked.form.waiting.is_none() {
            window.accept(&asked, &[]);
        }
    };
    window.cancel(&waiting);
    std::thread::sleep(Duration::from_millis(500));
    let kept = section(&config, "drive-2").unwrap();
    assert!(
        kept.contains(&format!("token = {token}")),
        "a sign-in cancelled leaves the one there was: {kept:?}"
    );
    std::fs::remove_dir_all(&base).unwrap();
}

/// A remote Skwir ended in the middle of setting up (here, while its
/// browser's sign-in waits) stays in rclone's configuration, half made:
/// Skwir noted it first, so the next run says so where it is listed (its
/// line in Settings, its place drawn as not ready) and offers Finish
/// Setting Up… and Remove…, which removes it and the note.
#[test]
fn a_remote_left_half_made_is_said_so_and_removed() {
    if let Some(why) = Rclone::default().unavailable() {
        eprintln!("skipped: {why}");
        return;
    }
    let base = fixture("half-made");
    let _opened = fake_browser(&base);
    // Settings kept in files, as a run of Skwir keeps them.
    let app_in = |dir: &str, config: &Path| {
        let root = base.join(dir);
        let app = App::new(AppConfig {
            settings_path: Some(root.join("settings.toml")),
            data_path: Some(root.join("data.toml")),
            ..AppConfig::ephemeral(root.join("app"))
        })
        .unwrap();
        let rclone = Rclone {
            config: Some(config.to_path_buf()),
            askpass: Some(PathBuf::from(env!("CARGO_BIN_EXE_skwir"))),
            keyring: Some(Arc::new(TestKeyring::default())),
        };
        let remotes = Remotes::with(vec![Arc::new(rclone.clone())])
            .with_rclone(rclone)
            .with_keyring(Arc::new(TestKeyring::default()));
        app.plugins.add(Arc::new(RemotePlugin::with(remotes)), &app);
        app
    };
    let unfinished =
        |app: &App| -> Vec<String> { app.settings.get("remote.unfinished").unwrap_or_default() };
    let config = base.join("first/rclone.conf");
    std::fs::create_dir_all(base.join("first")).unwrap();
    let first = app_in("first", &config);
    let window = Window::new(&first);
    first
        .dispatch(&Invocation::with_args(
            "remote.add_server",
            &serde_json::json!({ "kind": "drive" }),
        ))
        .unwrap();
    let options = window.form(30);
    window.accept(&options, &[("remote.name", "half-made-1")]);
    let waiting = loop {
        let asked = window.next(60, |_| true);
        if asked.form.waiting.is_some() && asked.form.fields.iter().any(|f| f.kind == Kind::Link) {
            break asked;
        }
        if asked.form.waiting.is_none() {
            window.accept(&asked, &[]);
        }
    };
    assert_eq!(
        unfinished(&first),
        ["half-made-1"],
        "noted before its questions"
    );
    // What Skwir ending now leaves: its settings, rclone's configuration.
    let second_dir = base.join("second");
    std::fs::create_dir_all(&second_dir).unwrap();
    for name in ["settings.toml", "data.toml"] {
        let from = base.join("first").join(name);
        if from.exists() {
            std::fs::copy(&from, second_dir.join(name)).unwrap();
        }
    }
    let left = second_dir.join("rclone.conf");
    std::fs::copy(&config, &left).unwrap();
    assert!(
        section(&left, "half-made-1").is_some(),
        "half made in rclone's configuration"
    );
    // The first run's sign-in cancelled: its remote is not kept, nor noted.
    window.cancel(&waiting);
    let deadline = Instant::now() + Duration::from_secs(30);
    while section(&config, "half-made-1").is_some() || !unfinished(&first).is_empty() {
        first.pump();
        assert!(Instant::now() < deadline, "undone, its note forgotten");
        std::thread::sleep(Duration::from_millis(50));
    }
    drop(window);

    // The next run says so.
    let second = app_in("second", &left);
    assert_eq!(unfinished(&second), ["half-made-1"]);
    let record = record_of(&second, "half-made-1", 30);
    assert!(
        record["kind"]
            .as_str()
            .unwrap()
            .ends_with("not finished setting up"),
        "{record}"
    );
    assert_eq!(record["actions"][0]["label"], "Finish Setting Up…");
    assert_eq!(record["actions"][0]["command"], "remote.edit_server");
    assert_eq!(record["actions"][0]["args"]["id"], "half-made-1");
    let place = |app: &App| {
        app.registry
            .places
            .find(|p| p.key.as_deref() == Some("remote:rclone://half-made-1/"))
    };
    let deadline = Instant::now() + Duration::from_secs(30);
    while place(&second).is_none() {
        second.pump();
        assert!(Instant::now() < deadline, "its place shown");
        std::thread::sleep(Duration::from_millis(50));
    }
    let shown = place(&second).unwrap();
    assert!(
        shown
            .unavailable
            .as_deref()
            .is_some_and(|why| why.contains("not finished setting up")),
        "{shown:?}"
    );
    assert!(
        shown
            .menu
            .iter()
            .any(|item| item.title == "Finish Setting Up…")
    );
    // Removed, its note goes too.
    let window = Window::new(&second);
    second
        .dispatch(&Invocation::with_args(
            "remote.remove_server",
            &serde_json::json!({ "id": "half-made-1" }),
        ))
        .unwrap();
    let confirm = window.form(30);
    window.accept(&confirm, &[]);
    until_said(&second, 30, "“half-made-1” is removed");
    assert!(section(&left, "half-made-1").is_none());
    assert!(unfinished(&second).is_empty());
    let deadline = Instant::now() + Duration::from_secs(30);
    while server_records(&second)
        .iter()
        .any(|r| r["id"] == "half-made-1")
    {
        second.pump();
        assert!(Instant::now() < deadline, "no longer listed");
        std::thread::sleep(Duration::from_millis(50));
    }
    drop(window);
    drop(second);
    drop(first);
    std::fs::remove_dir_all(&base).unwrap();
}

/// A remote of rclone's whose name holds what a server's name cannot (a
/// space, an `@`, another alphabet: rclone takes them) opens by its
/// address all the same, the name escaped: listed, mounted, its files
/// shown, the address kept as it reads back. An alias of a folder of the
/// test's: nothing leaves the machine.
#[test]
fn a_remote_named_with_spaces_and_other_alphabets_opens_by_its_address() {
    if let Some(why) = Rclone::default().unavailable() {
        eprintln!("skipped: {why}");
        return;
    }
    let base = fixture("odd-names");
    let served = base.join("served");
    std::fs::create_dir_all(&served).unwrap();
    std::fs::write(served.join("it's here.txt"), "here").unwrap();
    let name = "Мои фото @nas";
    let config = base.join("rclone.conf");
    std::fs::write(
        &config,
        format!("[{name}]\ntype = alias\nremote = {}\n", served.display()),
    )
    .unwrap();
    let app = app_with_rclone(&base, &config, Arc::default());
    let record = record_of(&app, name, 30);
    assert_eq!(record["name"], name);
    let address = Address::rclone(name, b"/").unwrap().to_string();
    assert!(
        address.is_ascii() && !address.contains(' '),
        "escaped: {address}"
    );
    let pane = app.workspace.read().active_id();
    app.navigate(pane, &skwir_core::location::Location::parse(&address))
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(60);
    while !app.pane_location(pane).is_some_and(|l| l.is_path()) {
        app.pump();
        assert!(Instant::now() < deadline, "{address} never opened");
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(app.pump_until_complete(pane, Duration::from_secs(20)));
    let names: Vec<String> = {
        let ws = app.workspace.read();
        ws.pane(pane)
            .unwrap()
            .listing
            .iter()
            .map(|e| e.name_str().into_owned())
            .collect()
    };
    assert_eq!(names, ["it's here.txt"]);
    let shown = app.pane_location(pane).unwrap();
    assert_eq!(
        app.address_of(&shown).map(|a| a.to_string()),
        Some(address.clone()),
        "kept by its address"
    );
    let point = skwir_core::fs::reach::of(&shown.path().unwrap())
        .unwrap()
        .point
        .clone();
    app.dispatch(&Invocation::with_args(
        "remote.disconnect",
        &serde_json::json!({ "path": point }),
    ))
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    while skwir_core::fs::reach::of(&point).is_some_and(|r| r.point == point) {
        app.pump();
        assert!(Instant::now() < deadline, "let go of");
        std::thread::sleep(Duration::from_millis(50));
    }
    drop(app);
    std::fs::remove_dir_all(&base).unwrap();
}

/// A remote built on others: an encrypted one picks the remote (and the
/// folder of it) it stands on among those there are, and what is written
/// through it is encrypted there; a union stands on the remotes switched
/// on in its form.
#[test]
fn a_remote_stands_on_the_remotes_picked_in_its_form() {
    if let Some(why) = Rclone::default().unavailable() {
        eprintln!("skipped: {why}");
        return;
    }
    let base = fixture("on-remotes");
    let (one, two) = (base.join("one"), base.join("two"));
    std::fs::create_dir_all(one.join("vault")).unwrap();
    std::fs::create_dir_all(&two).unwrap();
    let config = base.join("rclone.conf");
    std::fs::write(
        &config,
        format!(
            "[one]\ntype = alias\nremote = {}\n\n[two]\ntype = alias\nremote = {}\n",
            one.display(),
            two.display()
        ),
    )
    .unwrap();
    let app = app_with_rclone(&base, &config, Arc::default());
    let window = Window::new(&app);

    app.dispatch(&Invocation::with_args(
        "remote.add_server",
        &serde_json::json!({ "kind": "crypt" }),
    ))
    .unwrap();
    let options = window.form(30);
    let stands_on = options
        .form
        .fields
        .iter()
        .find(|f| f.key == "remote")
        .unwrap();
    let Kind::Choice { choices, open } = &stands_on.kind else {
        panic!("a choice of remotes: {stands_on:?}");
    };
    assert!(open, "a folder of one may follow");
    let offered: Vec<&str> = choices.iter().map(|c| c.value.as_str()).collect();
    assert_eq!(offered, ["one:", "two:"]);
    window.accept(
        &options,
        &[
            ("remote.name", "secret-1"),
            ("remote", "one:vault"),
            ("password", "a pass phrase, it's long"),
        ],
    );
    until_said(&app, 30, "“secret-1” is added");
    let kept = section(&config, "secret-1").unwrap();
    assert!(kept.contains(&"remote = one:vault".to_owned()), "{kept:?}");
    // Written through it, it is encrypted where it stands.
    let file = base.join("hello.txt");
    std::fs::write(&file, "hello").unwrap();
    let status = Command::new("rclone")
        .arg("--config")
        .arg(&config)
        .arg("copyto")
        .arg(&file)
        .arg("secret-1:hello.txt")
        .status()
        .unwrap();
    assert!(status.success());
    let stored: Vec<String> = std::fs::read_dir(one.join("vault"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(stored.len(), 1);
    assert_ne!(stored[0], "hello.txt", "its name encrypted");
    let read = Command::new("rclone")
        .arg("--config")
        .arg(&config)
        .args(["cat", "secret-1:hello.txt"])
        .output()
        .unwrap();
    assert_eq!(read.stdout, b"hello");

    app.dispatch(&Invocation::with_args(
        "remote.add_server",
        &serde_json::json!({ "kind": "union" }),
    ))
    .unwrap();
    let options = window.form(30);
    let switches: Vec<&str> = options
        .form
        .fields
        .iter()
        .filter(|f| f.kind == Kind::Toggle && f.key.starts_with("upstream:"))
        .map(|f| f.title.as_str())
        .collect();
    assert_eq!(
        switches,
        ["Include “one”", "Include “secret-1”", "Include “two”"]
    );
    window.accept(
        &options,
        &[
            ("remote.name", "both"),
            ("upstream:one", "true"),
            ("upstream:two", "true"),
        ],
    );
    until_said(&app, 30, "“both” is added");
    let kept = section(&config, "both").unwrap();
    assert!(
        kept.contains(&"upstreams = one: two:".to_owned()),
        "{kept:?}"
    );
    std::fs::remove_dir_all(&base).unwrap();
}

/// Lets go of a FUSE mount the test made, should it fail midway: a mount
/// whose program is gone holds whatever looks into it.
struct Unmounted(PathBuf);

impl Drop for Unmounted {
    fn drop(&mut self) {
        if skwir_core::fs::reach::of(&self.0).is_some_and(|r| r.point == self.0) {
            let _ = Command::new("fusermount3").arg("-uz").arg(&self.0).status();
        }
    }
}

/// The mount `address` leads to once a pane shows it, mounted by the app.
fn mounted(app: &App, address: &str) -> PathBuf {
    let pane = app.workspace.read().active_id();
    app.navigate(pane, &skwir_core::location::Location::parse(address))
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        app.pump();
        if let Some(path) = app.pane_location(pane).and_then(|l| l.path()) {
            return skwir_core::fs::reach::of(&path).unwrap().point.clone();
        }
        assert!(
            Instant::now() < deadline,
            "{address} never led to its folder"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// What rclone says a mount's storage can do shapes it: a storage that
/// cannot say how full it is shows no size (rclone's mount makes one up),
/// one that can shows its own; a move within a mount is made on the
/// server (WebDAV's `MOVE`), its bytes never coming down and back up.
#[test]
fn what_rclone_says_a_storage_can_do_shapes_its_mount() {
    if let Some(why) = Rclone::default().unavailable() {
        eprintln!("skipped: {why}");
        return;
    }
    let base = fixture("features");
    let disk = base.join("disk");
    std::fs::create_dir_all(&disk).unwrap();
    let served_root = base.join("served");
    std::fs::create_dir_all(served_root.join("a folder")).unwrap();
    std::fs::create_dir_all(served_root.join("elsewhere")).unwrap();
    std::fs::write(served_root.join("a folder/large.bin"), vec![7u8; 16 << 20]).unwrap();
    let served = Served::start("webdav", &served_root, &["-vv"]);
    let config = base.join("rclone.conf");
    std::fs::write(
        &config,
        format!(
            "[mem]\ntype = memory\n\n[disk]\ntype = alias\nremote = {}\n\n[dav]\ntype = webdav\nurl = http://127.0.0.1:{}/\n",
            disk.display(),
            served.port
        ),
    )
    .unwrap();
    let app = app_with_rclone(&base, &config, Arc::default());
    let in_memory = mounted(&app, "rclone://mem/");
    let _left = Unmounted(in_memory.clone());
    let on_disk = mounted(&app, "rclone://disk/");
    let _left_too = Unmounted(on_disk.clone());
    let space = |point: &Path| {
        app.registry
            .places
            .find(|p| p.location == skwir_core::location::Location::from_path(point))
            .and_then(|p| p.space)
    };
    let deadline = Instant::now() + Duration::from_secs(30);
    while space(&on_disk).is_none() {
        app.pump();
        app.dispatch(&Invocation::new("remote.refresh")).unwrap();
        assert!(Instant::now() < deadline, "the disk's space was never said");
        std::thread::sleep(Duration::from_millis(200));
    }
    // Asked as often again: the storage in memory says none.
    for _ in 0..10 {
        app.pump();
        app.dispatch(&Invocation::new("remote.refresh")).unwrap();
        std::thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(space(&in_memory), None, "no size made up");
    assert!(
        skwir_platform::places::space(&in_memory).is_some(),
        "rclone's mount says one all the same"
    );

    // A move within the WebDAV mount: one MOVE on the server.
    let on_dav = mounted(&app, "rclone://dav/");
    let _left_dav = Unmounted(on_dav.clone());
    let plan = skwir_core::op::Plan::with_steps(
        "move",
        vec![Box::new(skwir_core::op::Move::new(
            skwir_core::op::EntryRef::from_path(on_dav.join("a folder")).unwrap(),
            on_dav.join("elsewhere"),
        )) as Box<dyn skwir_core::op::Operation>],
    );
    let done = app.executor.execute_blocking(&plan, app.lifetime());
    assert!(done.error.is_none(), "{:?}", done.error);
    assert_eq!(
        std::fs::metadata(served_root.join("elsewhere/a folder/large.bin"))
            .unwrap()
            .len(),
        16 << 20
    );
    let log = std::fs::read_to_string(served_root.with_extension("webdav.log")).unwrap();
    assert!(log.contains("MOVE"), "moved on the server");
    assert!(
        !log.lines()
            .any(|l| l.contains("PUT") && l.contains("large.bin")),
        "never sent up again"
    );
    for point in [&in_memory, &on_disk, &on_dav] {
        app.dispatch(&Invocation::with_args(
            "remote.disconnect",
            &serde_json::json!({ "path": point }),
        ))
        .unwrap();
    }
    let deadline = Instant::now() + Duration::from_secs(30);
    while [&in_memory, &on_disk, &on_dav]
        .iter()
        .any(|p| skwir_core::fs::reach::of(p).is_some_and(|r| r.point == **p))
    {
        app.pump();
        assert!(Instant::now() < deadline, "never let go of");
        std::thread::sleep(Duration::from_millis(100));
    }
    drop(served);
    std::fs::remove_dir_all(&base).unwrap();
}

/// An `s3://` address opens through the user's own AWS configuration
/// (credentials, region, endpoint; a profile where the address names
/// one), by rclone with nothing saved: its configuration stays as it was.
/// Against an S3 server of the test's own, and AWS files of its own.
#[test]
fn an_s3_address_opens_through_the_aws_configuration_keeping_nothing() {
    if let Some(why) = Rclone::default().unavailable() {
        eprintln!("skipped: {why}");
        return;
    }
    let base = fixture("s3");
    let data = base.join("served");
    std::fs::create_dir_all(data.join("bucket-one/sub")).unwrap();
    std::fs::write(data.join("bucket-one/sub/it's here.txt"), "hello").unwrap();
    let served = Served::start("s3", &data, &["--auth-key", "AKIDTEST,secret-of-the-test"]);
    let aws = base.join("aws");
    std::fs::create_dir_all(&aws).unwrap();
    let endpoint = format!("http://127.0.0.1:{}", served.port);
    std::fs::write(
        aws.join("config"),
        format!(
            "[default]\nregion = us-east-1\nendpoint_url = {endpoint}\n\n[profile work]\nregion = eu-west-3\nendpoint_url = {endpoint}\n"
        ),
    )
    .unwrap();
    std::fs::write(
        aws.join("credentials"),
        "[default]\naws_access_key_id = AKIDTEST\naws_secret_access_key = secret-of-the-test\n\n[work]\naws_access_key_id = AKIDTEST\naws_secret_access_key = secret-of-the-test\n",
    )
    .unwrap();
    // SAFETY: nextest runs each test in a process of its own, and this
    // runs before the test starts an app or a program, so no other thread
    // reads the environment meanwhile.
    #[allow(unsafe_code)]
    unsafe {
        std::env::set_var("AWS_CONFIG_FILE", aws.join("config"));
        std::env::set_var("AWS_SHARED_CREDENTIALS_FILE", aws.join("credentials"));
        std::env::remove_var("AWS_PROFILE");
    }
    let config = base.join("rclone.conf");
    std::fs::write(&config, "").unwrap();
    let app = app_with_rclone(&base, &config, Arc::default());
    let pane = app.workspace.read().active_id();
    let address = "s3://bucket-one/sub";
    app.navigate(pane, &skwir_core::location::Location::parse(address))
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(60);
    let folder = loop {
        app.pump();
        if let Some(path) = app.pane_location(pane).and_then(|l| l.path()) {
            break path;
        }
        assert!(
            Instant::now() < deadline,
            "the address never led to its folder"
        );
        std::thread::sleep(Duration::from_millis(50));
    };
    let point = skwir_core::fs::reach::of(&folder).unwrap().point.clone();
    let _left = Unmounted(point.clone());
    assert_eq!(
        std::fs::read_to_string(folder.join("it's here.txt")).unwrap(),
        "hello"
    );
    let shown = app.pane_location(pane).unwrap();
    assert_eq!(app.address_of(&shown).unwrap().to_string(), address);

    // By a profile of its own.
    let by_profile = mounted(&app, "s3://work@bucket-one/");
    let _left_too = Unmounted(by_profile.clone());
    assert!(by_profile.join("sub/it's here.txt").exists());
    assert_eq!(
        std::fs::read_to_string(&config).unwrap(),
        "",
        "nothing kept in rclone's configuration"
    );
    for point in [&point, &by_profile] {
        app.dispatch(&Invocation::with_args(
            "remote.disconnect",
            &serde_json::json!({ "path": point }),
        ))
        .unwrap();
    }
    let deadline = Instant::now() + Duration::from_secs(30);
    while [&point, &by_profile]
        .iter()
        .any(|p| skwir_core::fs::reach::of(p).is_some_and(|r| r.point == **p))
    {
        app.pump();
        assert!(Instant::now() < deadline, "never let go of");
        std::thread::sleep(Duration::from_millis(100));
    }
    drop(served);
    std::fs::remove_dir_all(&base).unwrap();
}

/// The records Settings lists of the servers, by their id.
fn server_records(app: &App) -> Vec<serde_json::Value> {
    let skwir_core::command::Outcome::Value(records) = app
        .dispatch(&Invocation::new("remote.server_records"))
        .unwrap()
    else {
        panic!("records");
    };
    records["records"].as_array().unwrap().clone()
}

/// Let go of what the app mounted of the server on `port`, before that
/// server stops: a mount whose server is gone holds whatever looks into
/// it, other tests' included (the runtime folder is the tests' shared
/// one). Only this test's server's: others mount there too.
fn let_go_of_mounts(app: &App, port: u16) {
    let this = format!("127.0.0.1:{port}");
    let ours = || -> Vec<PathBuf> {
        skwir_core::fs::reach::changed();
        skwir_core::fs::reach::all()
            .into_iter()
            .map(|r| r.point.clone())
            .filter(|p| {
                p.file_name()
                    .is_some_and(|n| n.to_string_lossy().contains(&this))
            })
            .collect()
    };
    // Until none is left: a connection still on its way mounts later.
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        app.pump();
        let points = ours();
        if points.is_empty() {
            return;
        }
        for point in points {
            app.dispatch(&Invocation::with_args(
                "remote.disconnect",
                &serde_json::json!({ "path": point }),
            ))
            .unwrap();
            while skwir_core::fs::reach::of(&point).is_some_and(|r| r.point == point) {
                app.pump();
                assert!(Instant::now() < deadline, "{} let go of", point.display());
                std::thread::sleep(Duration::from_millis(50));
            }
        }
        assert!(Instant::now() < deadline, "its mounts let go of");
    }
}

/// The record of the server `id`, once listed, within `seconds`.
fn record_of(app: &App, id: &str, seconds: u64) -> serde_json::Value {
    let deadline = Instant::now() + Duration::from_secs(seconds);
    loop {
        app.pump();
        if let Some(record) = server_records(app).into_iter().find(|r| r["id"] == id) {
            return record;
        }
        assert!(Instant::now() < deadline, "{id} never listed");
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// The Network section's places, by their titles.
fn network_titles(app: &App) -> Vec<String> {
    app.registry
        .places
        .all()
        .into_iter()
        .filter(|p| p.group == "network")
        .map(|p| p.title)
        .collect()
}

/// One Add Server for every kind: a server added by its address (a
/// WebDAV one here) is kept under the name given in Skwir's list, stands
/// in the Network section and in Settings' list of servers, never among
/// the bookmarks, and is opened; its password is asked by the connection,
/// never by the form. Edited and removed where it is kept; kept in KDE's
/// network folders when chosen, where Dolphin reads it; and a remote
/// folder bookmarked made a server by Keep as Server, its bookmark taken
/// out.
#[test]
fn a_server_added_by_its_address_is_kept_and_opened_at_once() {
    if let Some(why) = Rclone::default().unavailable() {
        eprintln!("skipped: {why}");
        return;
    }
    let base = fixture("by-address");
    let served_root = base.join("served");
    std::fs::create_dir_all(served_root.join("sub folder")).unwrap();
    std::fs::write(served_root.join("sub folder/hello.txt"), "hello").unwrap();
    let served = Served::start("webdav", &served_root, &[]);
    // The desktop's bookmarks and KDE's network folders of the test's own:
    // the tests' shared folders are written by others that run beside it.
    // SAFETY: nextest runs each test in a process of its own, and this
    // runs before the test starts an app, so no other thread reads the
    // environment meanwhile.
    #[allow(unsafe_code)]
    unsafe {
        std::env::set_var("XDG_CONFIG_HOME", base.join("config"));
        std::env::set_var("XDG_DATA_HOME", base.join("data"));
    }
    let config = base.join("rclone.conf");
    let keyring = Arc::new(TestKeyring::default());
    let app = app_with_rclone(&base, &config, Arc::clone(&keyring));
    app.plugins
        .add(Arc::new(skwir_plugin_places::PlacesPlugin::default()), &app);
    app.settings.set(
        "remote.servers_kept_in",
        toml::Value::String("skwir".into()),
    );
    let window = Window::new(&app);

    app.dispatch(&Invocation::new("remote.add_server")).unwrap();
    let kinds = window.form(30);
    let Kind::Pick { choices } = &kinds.form.fields[0].kind else {
        panic!("a pick of kinds");
    };
    assert_eq!(choices[0].value, "address:sftp", "servers by address first");
    window.accept(&kinds, &[("picked", "address:dav")]);
    let parts = window.form(30);
    assert!(parts.form.back, "Back to the kinds");
    assert!(
        parts.form.intro.contains("Skwir's list"),
        "{}",
        parts.form.intro
    );
    assert!(
        !parts
            .form
            .fields
            .iter()
            .any(skwir_core::form::Field::secret),
        "no password asked here: the connection asks it"
    );
    let port = served.port.to_string();
    window.accept(
        &parts,
        &[
            ("host", "127.0.0.1"),
            ("port", &port),
            ("path", "sub folder"),
            ("title", "It's my files"),
        ],
    );
    until_said(&app, 30, "“It's my files” is added");
    // Opened in the pane: the server's folder, mounted.
    let pane = app.workspace.read().active_id();
    let deadline = Instant::now() + Duration::from_secs(60);
    let folder = loop {
        app.pump();
        if let Some(path) = app.pane_location(pane).and_then(|l| l.path()) {
            break path;
        }
        assert!(Instant::now() < deadline, "never opened");
        std::thread::sleep(Duration::from_millis(50));
    };
    let point = skwir_core::fs::reach::of(&folder).unwrap().point.clone();
    let _left = Unmounted(point.clone());
    assert!(folder.join("hello.txt").exists());
    let id = format!("dav://127.0.0.1:{}/sub folder", served.port);
    let record = record_of(&app, &id, 30);
    assert_eq!(record["name"], "It's my files");
    assert!(
        record["kind"]
            .as_str()
            .unwrap()
            .ends_with("in Skwir's list"),
        "{record}"
    );
    assert_eq!(record["fixed"], false);
    let labels: Vec<&str> = record["actions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["label"].as_str().unwrap())
        .collect();
    assert_eq!(labels, ["Mount Options…"], "no Test: rclone's alone");
    // Mounted, its place in the Network section is the mount's, with the
    // server's own Edit and Remove.
    let mount = app
        .registry
        .places
        .all()
        .into_iter()
        .find(|p| p.group == "network" && p.location.path().as_deref() == Some(point.as_path()))
        .expect("its mount in the Network section");
    let titles: Vec<&str> = mount.menu.iter().map(|i| i.title.as_str()).collect();
    assert!(titles.contains(&"Edit Server…"), "{titles:?}");
    assert!(titles.contains(&"Remove Server…"), "{titles:?}");
    let bookmarks = base.join("config/gtk-3.0/bookmarks");
    assert!(
        !std::fs::read_to_string(&bookmarks).is_ok_and(|b| b.contains(&port)),
        "no bookmark of it"
    );
    assert!(
        std::fs::read_to_string(&config).map_or(true, |c| c.is_empty()),
        "nothing of rclone's"
    );
    app.dispatch(&Invocation::with_args(
        "remote.disconnect",
        &serde_json::json!({ "path": point }),
    ))
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    while skwir_core::fs::reach::of(&point).is_some_and(|r| r.point == point) {
        app.pump();
        assert!(Instant::now() < deadline, "never let go of");
        std::thread::sleep(Duration::from_millis(100));
    }
    let deadline = Instant::now() + Duration::from_secs(30);
    while !network_titles(&app).contains(&"It's my files".to_owned()) {
        app.pump();
        assert!(
            Instant::now() < deadline,
            "the server in the Network section: {:?}",
            network_titles(&app)
        );
        std::thread::sleep(Duration::from_millis(50));
    }

    // Edited in its form, filled in as kept: renamed, its folder changed.
    app.dispatch(&Invocation::with_args(
        "remote.edit_server",
        &serde_json::json!({ "id": id }),
    ))
    .unwrap();
    let edit = window.form(30);
    assert_eq!(edit.form.title, "Edit “It's my files”");
    let value = |key: &str| {
        edit.form
            .fields
            .iter()
            .find(|f| f.key == key)
            .unwrap()
            .value
            .clone()
    };
    assert_eq!(
        (value("host"), value("port"), value("path"), value("title")),
        (
            "127.0.0.1".to_owned(),
            port.clone(),
            "sub folder".to_owned(),
            "It's my files".to_owned()
        )
    );
    window.accept(&edit, &[("title", "Ünïcode files"), ("path", "")]);
    until_said(&app, 30, "“Ünïcode files” is saved");
    let moved = format!("dav://127.0.0.1:{}/", served.port);
    assert_eq!(record_of(&app, &moved, 30)["name"], "Ünïcode files");
    assert!(server_records(&app).iter().all(|r| r["id"] != id));

    // Kept in KDE's network folders, as chosen: a desktop entry KDE reads.
    app.settings
        .set("remote.servers_kept_in", toml::Value::String("kde".into()));
    app.dispatch(&Invocation::new("remote.add_server")).unwrap();
    let kinds = window.form(30);
    window.accept(&kinds, &[("picked", "address:dav")]);
    let parts = window.form(30);
    assert!(parts.form.intro.contains("KDE's network folders"));
    window.accept(
        &parts,
        &[
            ("host", "127.0.0.1"),
            ("port", &port),
            ("path", "sub folder"),
            ("title", "Shared"),
        ],
    );
    until_said(&app, 30, "“Shared” is added");
    // Opened as added: mounted before what follows, which takes less time
    // than a mount under load.
    let pane = app.workspace.read().active_id();
    let deadline = Instant::now() + Duration::from_secs(60);
    while !app
        .pane_location(pane)
        .and_then(|l| l.path())
        .is_some_and(|p| p.to_string_lossy().contains("sub folder"))
    {
        app.pump();
        assert!(Instant::now() < deadline, "“Shared” opened");
        std::thread::sleep(Duration::from_millis(50));
    }
    let entry = base.join("data/remoteview/Shared.desktop");
    let text = std::fs::read_to_string(&entry).unwrap();
    assert!(
        text.contains(&format!(
            "URL=webdav://127.0.0.1:{}/sub%20folder\n",
            served.port
        )),
        "{text}"
    );
    let record = record_of(&app, &id, 30);
    assert!(
        record["kind"]
            .as_str()
            .unwrap()
            .ends_with("in KDE's network folders"),
        "{record}"
    );
    // A password Skwir keeps for its server (the user's, as on a
    // connection's Remember) is forgotten from its line, and goes with the
    // last server that needs it.
    let shared = Address::parse(&id).unwrap();
    let key = passwords::key(&shared, passwords::user_of(&shared).as_deref());
    let remember = || {
        keyring.keep(&key, "Skwir: a test", "it's s3cret").unwrap();
        app.settings.set(
            "remote.remembered",
            toml::Value::Array(vec![toml::Value::String(key.clone())]),
        );
    };
    let remembered =
        |app: &App| -> Vec<String> { app.settings.get("remote.remembered").unwrap_or_default() };
    remember();
    let record = record_of(&app, &id, 30);
    let forget = record["actions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["label"] == "Forget Password")
        .unwrap_or_else(|| panic!("{record}"))
        .clone();
    assert_eq!(forget["args"]["server"], key.as_str());
    app.dispatch(&Invocation::with_args(
        forget["command"].as_str().unwrap(),
        &forget["args"],
    ))
    .unwrap();
    until_said(&app, 30, "The password is forgotten");
    assert!(keyring.own(&key).is_none() && remembered(&app).is_empty());
    let record = record_of(&app, &id, 30);
    assert!(
        record["actions"]
            .as_array()
            .unwrap()
            .iter()
            .all(|a| a["label"] != "Forget Password"),
        "{record}"
    );
    remember();

    // Removed where it is kept, once confirmed; its password stays, which
    // “Ünïcode files” needs, on the same server as the same user.
    app.dispatch(&Invocation::with_args(
        "remote.remove_server",
        &serde_json::json!({ "id": id }),
    ))
    .unwrap();
    let confirm = window.form(30);
    assert_eq!(confirm.form.title, "Remove “Shared”?");
    assert!(
        !confirm.form.intro.contains("password"),
        "{}",
        confirm.form.intro
    );
    window.accept(&confirm, &[]);
    until_said(&app, 30, "“Shared” is removed");
    assert!(!entry.exists(), "its desktop entry gone");
    assert_eq!(keyring.own(&key).as_deref(), Some("it's s3cret"));

    // A remote folder bookmarked (another file manager's) made a server:
    // offered in its place's menu, its bookmark then taken out.
    let other = format!("dav://127.0.0.1:{}/other", served.port);
    std::fs::create_dir_all(bookmarks.parent().unwrap()).unwrap();
    std::fs::write(&bookmarks, format!("{other} It's bookmarked\n")).unwrap();
    let skwir_core::command::Outcome::Value(menu) = app
        .dispatch(&Invocation::with_args(
            "remote.keep_as_server_menu",
            &serde_json::json!({ "location": other }),
        ))
        .unwrap()
    else {
        panic!("a menu");
    };
    let item = &menu["items"][0];
    assert_eq!(item["title"], "Keep as Server");
    app.dispatch(&Invocation::with_args(
        "remote.keep_as_server",
        &serde_json::json!({ "address": other }),
    ))
    .unwrap();
    until_said(
        &app,
        30,
        "“It's bookmarked” is a server of the Network section",
    );
    let deadline = Instant::now() + Duration::from_secs(30);
    while std::fs::read_to_string(&bookmarks).is_ok_and(|b| b.contains("/other")) {
        app.pump();
        assert!(Instant::now() < deadline, "its bookmark never went");
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(record_of(&app, &other, 30)["name"], "It's bookmarked");
    // A server already: offered no more.
    let skwir_core::command::Outcome::Value(menu) = app
        .dispatch(&Invocation::with_args(
            "remote.keep_as_server_menu",
            &serde_json::json!({ "location": other }),
        ))
        .unwrap()
    else {
        panic!("a menu");
    };
    assert_eq!(menu["items"], serde_json::json!([]));

    // The last server on it removed, its password goes too, said first.
    for (address, name, last) in [
        (&other, "It's bookmarked", false),
        (&moved, "Ünïcode files", true),
    ] {
        app.dispatch(&Invocation::with_args(
            "remote.remove_server",
            &serde_json::json!({ "id": address }),
        ))
        .unwrap();
        let confirm = window.form(30);
        assert_eq!(confirm.form.title, format!("Remove “{name}”?"));
        assert_eq!(
            confirm
                .form
                .intro
                .contains("the password it keeps for it in the keyring"),
            last,
            "{}",
            confirm.form.intro
        );
        window.accept(&confirm, &[]);
        until_said(&app, 30, &format!("“{name}” is removed"));
    }
    assert!(
        keyring.own(&key).is_none(),
        "forgotten with its last server"
    );
    assert!(remembered(&app).is_empty());
    let_go_of_mounts(&app, served.port);
    drop(served);
    std::fs::remove_dir_all(&base).unwrap();
}

/// A server found nearby is added as Nearby's Add as Server… asks
/// (`remote.add_server` given its kind and parts, never a password): its
/// form opens filled in with the name and address it announced, and the
/// server added is kept and opened.
#[test]
fn a_server_given_its_parts_is_added_from_a_form_filled_in() {
    if let Some(why) = Rclone::default().unavailable() {
        eprintln!("skipped: {why}");
        return;
    }
    let base = fixture("given-parts");
    let served_root = base.join("served");
    std::fs::create_dir_all(served_root.join("dav")).unwrap();
    std::fs::write(served_root.join("dav/hello.txt"), "hello").unwrap();
    let served = Served::start("webdav", &served_root, &[]);
    // SAFETY: nextest runs each test in a process of its own, and this
    // runs before the test starts an app, so no other thread reads the
    // environment meanwhile.
    #[allow(unsafe_code)]
    unsafe {
        std::env::set_var("XDG_CONFIG_HOME", base.join("config"));
        std::env::set_var("XDG_DATA_HOME", base.join("data"));
    }
    let app = app_with_rclone(&base, &base.join("rclone.conf"), Arc::default());
    app.settings.set(
        "remote.servers_kept_in",
        toml::Value::String("skwir".into()),
    );
    let window = Window::new(&app);
    let port = served.port.to_string();
    app.dispatch(&Invocation::with_args(
        "remote.add_server",
        &serde_json::json!({
            "kind": "address:dav",
            "options": { "host": "127.0.0.1", "port": port, "path": "dav", "title": "Test Files" },
        }),
    ))
    .unwrap();
    let parts = window.form(30);
    let value = |key: &str| {
        parts
            .form
            .fields
            .iter()
            .find(|f| f.key == key)
            .map(|f| f.value.clone())
    };
    assert_eq!(
        (value("host"), value("port"), value("path"), value("title")),
        (
            Some("127.0.0.1".to_owned()),
            Some(port.clone()),
            Some("dav".to_owned()),
            Some("Test Files".to_owned())
        ),
        "filled in"
    );
    window.accept(&parts, &[]);
    until_said(&app, 30, "“Test Files” is added");
    let address = format!("dav://127.0.0.1:{port}/dav");
    assert_eq!(record_of(&app, &address, 30)["name"], "Test Files");
    // Opened as added: let go of once mounted, before its server stops.
    let pane = app.workspace.read().active_id();
    let deadline = Instant::now() + Duration::from_secs(60);
    while !app.pane_location(pane).is_some_and(|l| l.is_path()) {
        app.pump();
        assert!(Instant::now() < deadline, "opened as added");
        std::thread::sleep(Duration::from_millis(50));
    }
    let_go_of_mounts(&app, served.port);
    drop(app);
    drop(served);
    std::fs::remove_dir_all(&base).unwrap();
}

/// The arguments of the rclone mounting `point`, read from the system's
/// list of processes: the one whose line names the point.
fn rclone_mounting(point: &Path) -> Vec<String> {
    for entry in std::fs::read_dir("/proc").unwrap().flatten() {
        let Ok(line) = std::fs::read(entry.path().join("cmdline")) else {
            continue;
        };
        let args: Vec<String> = line
            .split(|b| *b == 0)
            .map(|a| String::from_utf8_lossy(a).into_owned())
            .collect();
        if args.iter().any(|a| a == "mount") && args.iter().any(|a| Path::new(a) == point) {
            return args;
        }
    }
    Vec::new()
}

/// Mount options chosen for a server in its form are kept in the plugin's
/// data and given to rclone when it next connects: read only, nothing
/// written there; its file cache as chosen, after Skwir's own.
#[test]
fn mount_options_chosen_for_a_server_shape_its_next_mount() {
    if let Some(why) = Rclone::default().unavailable() {
        eprintln!("skipped: {why}");
        return;
    }
    let base = fixture("options");
    let served_root = base.join("served");
    std::fs::create_dir_all(&served_root).unwrap();
    std::fs::write(served_root.join("kept.txt"), "kept").unwrap();
    let served = Served::start("webdav", &served_root, &[]);
    let config = base.join("rclone.conf");
    std::fs::write(
        &config,
        format!(
            "[dav]\ntype = webdav\nurl = http://127.0.0.1:{}/\n",
            served.port
        ),
    )
    .unwrap();
    let app = app_with_rclone(&base, &config, Arc::default());
    let window = Window::new(&app);
    app.dispatch(&Invocation::with_args(
        "remote.mount_options",
        &serde_json::json!({ "address": "rclone://dav/" }),
    ))
    .unwrap();
    let asked = window.form(30);
    assert!(
        !asked.form.fields.iter().any(|f| f.key == "compression"),
        "ssh's alone"
    );
    window.accept(&asked, &[("read_only", "true"), ("bandwidth", "fast")]);
    let refused = window.form(30);
    assert!(refused.form.error.as_deref().unwrap().contains("rate"));
    window.accept(
        &refused,
        &[
            ("read_only", "true"),
            ("cache", "full"),
            ("bandwidth", "10M"),
        ],
    );
    until_said(&app, 30, "when it next connects");

    let point = mounted(&app, "rclone://dav/");
    let _left = Unmounted(point.clone());
    assert_eq!(
        std::fs::read_to_string(point.join("kept.txt")).unwrap(),
        "kept"
    );
    let refused = std::fs::write(point.join("new.txt"), "no").unwrap_err();
    assert_eq!(
        refused.raw_os_error(),
        Some(30),
        "read only (EROFS): {refused}"
    );
    let args = rclone_mounting(&point);
    let cache = args.iter().rposition(|a| a == "--vfs-cache-mode").unwrap();
    assert_eq!(args[cache + 1], "full", "the user's, after Skwir's own");
    let limit = args.iter().position(|a| a == "--bwlimit").unwrap();
    assert_eq!(args[limit + 1], "10M");
    app.dispatch(&Invocation::with_args(
        "remote.disconnect",
        &serde_json::json!({ "path": point }),
    ))
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    while skwir_core::fs::reach::of(&point).is_some_and(|r| r.point == point) {
        app.pump();
        assert!(Instant::now() < deadline, "never let go of");
        std::thread::sleep(Duration::from_millis(100));
    }
    drop(served);
    std::fs::remove_dir_all(&base).unwrap();
}

/// A command asking a form says so where no window is there to fill it
/// (a script's, an agent's call before the window is up): never nothing.
#[test]
fn a_form_asked_with_no_window_says_it_needs_one() {
    let base = fixture("no-window");
    let app = app_with_rclone(&base, &base.join("rclone.conf"), Arc::default());
    let refused = app
        .dispatch(&Invocation::new("remote.add_server"))
        .unwrap_err();
    assert!(
        format!("{refused:#}").contains("open it first"),
        "{refused:#}"
    );
    std::fs::remove_dir_all(&base).unwrap();
}
