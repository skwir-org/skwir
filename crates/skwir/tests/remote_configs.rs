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
use skwir_plugin_remote::connector::Connector;
use skwir_plugin_remote::passwords::Keyring;
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
/// password of an encrypted one kept in `keyring`.
fn app_with_rclone(base: &Path, config: &Path, keyring: Arc<TestKeyring>) -> Arc<App> {
    let app = App::new(AppConfig::ephemeral(base.join("app"))).unwrap();
    let rclone = Rclone {
        config: Some(config.to_path_buf()),
        askpass: Some(PathBuf::from(env!("CARGO_BIN_EXE_skwir"))),
        keyring: Some(keyring),
    };
    let remotes = Remotes::with(vec![Arc::new(rclone.clone())]).with_rclone(rclone);
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
    // A name rclone's address cannot hold is refused, the form kept.
    window.accept(
        &options,
        &[
            ("remote.name", "my nas"),
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
            .contains("letters a–z")
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
    let records = app
        .dispatch(&Invocation::new("remote.rclone_records"))
        .unwrap();
    let skwir_core::command::Outcome::Value(records) = records else {
        panic!("records");
    };
    assert_eq!(records["records"][0]["id"], "nas-1");
    assert_eq!(records["records"][0]["kind"], "webdav");

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
