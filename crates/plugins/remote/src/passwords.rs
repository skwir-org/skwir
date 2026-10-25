//! A server's password kept in the desktop's keyring, when the user asked
//! (ADR-082). A connection's questions for it are answered from the
//! keyring first: Skwir's own, then (Linux) one GVfs or Nautilus kept under
//! libsecret's network schema, read only; the window is asked where none
//! is kept, or the one given was refused (asked again), and offers to
//! remember the answer, kept once the connection served. Never a key's
//! passphrase, a code or a configuration's password: those are secrets
//! asked each time (`Ask::secret`).

use std::sync::Arc;

use parking_lot::Mutex;
use skwir_core::cancel::CancelToken;

use crate::address::{Address, Scheme};
use crate::connector::{Ask, Password};

/// Where Skwir's own passwords are in the keyring
/// (`skwir_platform::secrets`): one record per server, by its key.
const SETTING: &str = "remote.passwords";
const FIELD: &str = "password";

/// The keyring as connections read and write it; a test's stands in.
pub trait Keyring: Send + Sync {
    /// Skwir's own password for the server keyed `server` ([`key`]).
    fn own(&self, server: &str) -> Option<String>;
    /// One the desktop keeps under libsecret's network schema, found by
    /// `named` ([`gvfs_named`]): GVfs's, Nautilus's.
    fn desktop(&self, named: &[(&str, &str)]) -> Option<String>;
    /// Keep `password` as Skwir's own for `server`, `label` what the
    /// keyring's own window shows.
    ///
    /// # Errors
    /// When no keyring answered, or it refused.
    fn keep(&self, server: &str, label: &str, password: &str) -> anyhow::Result<()>;
    /// Forget Skwir's own password for `server`.
    ///
    /// # Errors
    /// When no keyring answered, or it refused.
    fn forget(&self, server: &str) -> anyhow::Result<()>;
}

/// The desktop's keyring (`skwir_platform::secrets`): the Secret Service,
/// the Keychain, the Credential Manager. Each call blocks until it answers
/// (an unlock it asks for included): on a worker.
pub struct Desktop;

impl Keyring for Desktop {
    fn own(&self, server: &str) -> Option<String> {
        match skwir_platform::secrets::lookup(at(server)) {
            Ok(found) => found.map(|secret| secret.expose().to_owned()),
            Err(error) => {
                tracing::debug!("the keyring was not read: {error:#}");
                None
            }
        }
    }

    fn desktop(&self, named: &[(&str, &str)]) -> Option<String> {
        match skwir_platform::secrets::lookup_network(named) {
            Ok(found) => found.map(|secret| secret.expose().to_owned()),
            Err(error) => {
                tracing::debug!("the keyring was not read: {error:#}");
                None
            }
        }
    }

    fn keep(&self, server: &str, label: &str, password: &str) -> anyhow::Result<()> {
        skwir_platform::secrets::store(at(server), label, password)
    }

    fn forget(&self, server: &str) -> anyhow::Result<()> {
        skwir_platform::secrets::delete(at(server))
    }
}

/// Skwir's own record of the server keyed `server`.
fn at(server: &str) -> skwir_platform::secrets::SecretRef<'_> {
    skwir_platform::secrets::SecretRef {
        setting: SETTING,
        record: server,
        field: FIELD,
    }
}

/// The user an address logs in as: its own, else this machine's (ssh's,
/// smbclient's and rclone's default). Reads the user database: on a
/// worker.
pub fn user_of(address: &Address) -> Option<String> {
    address.user.clone().or_else(local_user)
}

/// This machine's user's name.
fn local_user() -> Option<String> {
    skwir_platform::users::load();
    skwir_platform::users::me()
        .and_then(skwir_platform::users::user)
        .or_else(|| std::env::var("USER").ok())
        .or_else(|| std::env::var("USERNAME").ok())
}

/// The key of the server `address` is on, as `user` logs in there: its
/// scheme, domain, user, host and port where not the usual one
/// (`smb://WORK;me@nas`, `ftp://me@host:2121`). One password a server and
/// user, whatever the path or the share.
pub fn key(address: &Address, user: Option<&str>) -> String {
    let mut server = address.clone();
    server.user = user.map(ToOwned::to_owned);
    server.port = address.effective_port();
    server.path = b"/".to_vec();
    server.to_string().trim_end_matches('/').to_owned()
}

/// What the keyring's own window names a password of Skwir's by.
pub fn label(address: &Address, user: Option<&str>) -> String {
    let who = match user {
        Some(user) => format!("{user}@{}", address.host),
        None => address.host.clone(),
    };
    format!("Skwir: {who} ({})", address.scheme.title())
}

/// What a password GVfs kept for `address`, logged in as `user`, is found
/// by under libsecret's network schema: the protocol GVfs names (`sftp`,
/// `smb`, `http` for WebDAV, `ftp`), the server, the user, an SMB domain,
/// a port where not the usual one. `None` where GVfs keeps none (NFS,
/// AFP, rclone's own), or no user is known.
pub fn gvfs_named(address: &Address, user: Option<&str>) -> Option<Vec<(&'static str, String)>> {
    let protocol = match address.scheme {
        Scheme::Sftp => "sftp",
        Scheme::Smb => "smb",
        Scheme::Dav | Scheme::Davs => "http",
        Scheme::Ftp | Scheme::Ftps => "ftp",
        Scheme::Nfs | Scheme::Afp | Scheme::Rclone | Scheme::S3 => return None,
    };
    let mut named = vec![
        ("protocol", protocol.to_owned()),
        ("server", address.bare_host().to_owned()),
        ("user", user?.to_owned()),
    ];
    if let Some(domain) = &address.domain {
        named.push(("domain", domain.clone()));
    }
    // WebDAV's port GVfs keeps as it found it, said or not: not asked.
    if !matches!(address.scheme, Scheme::Dav | Scheme::Davs)
        && let Some(port) = address.effective_port()
    {
        named.push(("port", port.to_string()));
    }
    Some(named)
}

/// Whether `prompt`, what ssh asks through `SSH_ASKPASS`, is a server's
/// password: it names the login (`me@host's password:`, `(me@host)
/// Password:`). A key's passphrase, a code, and rclone's own question
/// (`Password`, for its configuration) are not.
pub fn is_server_password(prompt: &str) -> bool {
    let prompt = prompt.trim().trim_end_matches(':').trim().to_lowercase();
    prompt.ends_with("password") && prompt.contains('@')
}

/// What a connection was given for a server's password.
enum Given {
    /// Skwir's own, from the keyring.
    Own,
    /// GVfs's, from the keyring.
    Desktop,
    /// The user's answer.
    Asked(Password),
}

/// What a connection tried of the keyring, and what it gave last.
#[derive(Default)]
struct Tried {
    own: bool,
    /// Skwir's own was there, given: refused if the window was asked after.
    own_found: bool,
    desktop: bool,
    given: Option<Given>,
}

/// What [`Keyed::served`] did of the keyring.
#[derive(Debug, PartialEq, Eq)]
pub enum Noted {
    /// The user's answer is kept, by the server's key.
    Kept(String),
    /// Skwir's own, refused and answered otherwise, is forgotten.
    Forgot(String),
    /// The user asked for it to be kept, and the keyring refused: why.
    NotKept(String),
}

/// A connection's questions, a server's password answered from the
/// keyring first (module documentation); the rest as `inner` answers.
pub struct Keyed {
    inner: Arc<dyn Ask>,
    keyring: Arc<dyn Keyring>,
    address: Address,
    user: Option<String>,
    tried: Mutex<Tried>,
    /// What cancels the connection: a keyring that asks to be unlocked
    /// waits for its own prompt, which the user may leave unanswered.
    cancel: CancelToken,
}

impl Keyed {
    /// Questions about connecting to `address` (its server's top), asked
    /// of `inner` where the keyring answers none, until `cancel`.
    pub fn new(
        inner: Arc<dyn Ask>,
        keyring: Arc<dyn Keyring>,
        address: Address,
        cancel: CancelToken,
    ) -> Keyed {
        let user = user_of(&address);
        Keyed {
            inner,
            keyring,
            address,
            user,
            tried: Mutex::default(),
            cancel,
        }
    }

    /// What `ask` answers of the keyring, on a thread of its own, unless
    /// the connection is cancelled first: then `None`, and the keyring's
    /// prompt (an unlock it asked for) is left to the user, its answer to
    /// no one.
    fn heard<T: Send + 'static>(&self, ask: impl FnOnce() -> T + Send + 'static) -> Option<T> {
        skwir_platform::secrets::heard(&self.cancel, ask)
    }

    fn key(&self) -> String {
        key(&self.address, self.user.as_deref())
    }

    /// The next password the keyring holds for the server that was not
    /// given yet this connection: Skwir's own, then GVfs's.
    fn kept(&self) -> Option<String> {
        let mut tried = self.tried.lock();
        if !tried.own {
            tried.own = true;
            let (keyring, key) = (Arc::clone(&self.keyring), self.key());
            if let Some(own) = self.heard(move || keyring.own(&key)).flatten() {
                tried.own_found = true;
                tried.given = Some(Given::Own);
                return Some(own);
            }
        }
        if !tried.desktop {
            tried.desktop = true;
            let named = gvfs_named(&self.address, self.user.as_deref())?;
            let keyring = Arc::clone(&self.keyring);
            let theirs = self.heard(move || {
                let named: Vec<(&str, &str)> =
                    named.iter().map(|(k, v)| (*k, v.as_str())).collect();
                keyring.desktop(&named)
            });
            if let Some(theirs) = theirs.flatten() {
                tried.given = Some(Given::Desktop);
                return Some(theirs);
            }
        }
        None
    }

    /// The connection served: the answer the user asked to remember is
    /// kept; Skwir's own, refused and answered otherwise, is forgotten.
    /// Says what it did, for the plugin to note which servers it keeps a
    /// password of. Blocks on the keyring: on a worker.
    pub fn served(&self) -> Option<Noted> {
        let tried = std::mem::take(&mut *self.tried.lock());
        let key = self.key();
        match tried.given? {
            Given::Asked(answer) if answer.remember => {
                let label = label(&self.address, self.user.as_deref());
                let (keyring, kept) = (Arc::clone(&self.keyring), key.clone());
                let keeping = self.heard(move || keyring.keep(&kept, &label, &answer.text));
                match keeping.unwrap_or_else(|| Err(anyhow::anyhow!("Cancelled"))) {
                    Ok(()) => Some(Noted::Kept(key)),
                    Err(error) => {
                        tracing::warn!("the password was not kept: {error:#}");
                        // Its first words for the window ("No keyring
                        // answered"), the rest in the log.
                        let said = format!("{error:#}");
                        let first = said.split(": ").next().unwrap_or(&said);
                        Some(Noted::NotKept(first.to_lowercase()))
                    }
                }
            }
            Given::Asked(_) if tried.own_found => {
                let (keyring, forgot) = (Arc::clone(&self.keyring), key.clone());
                let forgotten = self
                    .heard(move || keyring.forget(&forgot))
                    .unwrap_or_else(|| Err(anyhow::anyhow!("Cancelled")));
                match forgotten {
                    Ok(()) => Some(Noted::Forgot(key)),
                    Err(error) => {
                        tracing::warn!("a refused password was not forgotten: {error:#}");
                        None
                    }
                }
            }
            _ => None,
        }
    }
}

impl Ask for Keyed {
    fn secret(&self, title: &str, accept: &str, cancel: &CancelToken) -> Option<String> {
        self.inner.secret(title, accept, cancel)
    }

    fn confirm(&self, title: &str, detail: &str, cancel: &CancelToken) -> bool {
        self.inner.confirm(title, detail, cancel)
    }

    fn password(&self, title: &str, accept: &str, cancel: &CancelToken) -> Option<Password> {
        if let Some(kept) = self.kept() {
            return Some(Password {
                text: kept,
                remember: false,
            });
        }
        // Cancelled while the keyring waited for its prompt: nothing more
        // is asked.
        if cancel.is_cancelled() || self.cancel.is_cancelled() {
            return None;
        }
        let answer = self.inner.password(title, accept, cancel)?;
        self.tried.lock().given = Some(Given::Asked(Password {
            text: answer.text.clone(),
            remember: answer.remember,
        }));
        Some(answer)
    }

    fn kept_password(&self) -> Option<String> {
        self.kept()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    /// A keyring in memory: Skwir's own by key, the desktop's by the
    /// attributes asked, and what was asked of the window.
    #[derive(Default)]
    struct Memory {
        own: parking_lot::Mutex<HashMap<String, String>>,
        desktop: parking_lot::Mutex<Vec<(Vec<(String, String)>, String)>>,
    }

    impl Keyring for Memory {
        fn own(&self, server: &str) -> Option<String> {
            self.own.lock().get(server).cloned()
        }
        fn desktop(&self, named: &[(&str, &str)]) -> Option<String> {
            self.desktop
                .lock()
                .iter()
                .find(|(theirs, _)| {
                    named
                        .iter()
                        .all(|(k, v)| theirs.iter().any(|(tk, tv)| tk == k && tv == v))
                })
                .map(|(_, secret)| secret.clone())
        }
        fn keep(&self, server: &str, _: &str, password: &str) -> anyhow::Result<()> {
            self.own
                .lock()
                .insert(server.to_owned(), password.to_owned());
            Ok(())
        }
        fn forget(&self, server: &str) -> anyhow::Result<()> {
            self.own.lock().remove(server);
            Ok(())
        }
    }

    /// The window: answers `typed`, remembering or not, counting.
    struct Window {
        typed: String,
        remember: bool,
        asked: parking_lot::Mutex<usize>,
    }

    impl Ask for Window {
        fn secret(&self, _: &str, _: &str, _: &CancelToken) -> Option<String> {
            Some("a secret".to_owned())
        }
        fn confirm(&self, _: &str, _: &str, _: &CancelToken) -> bool {
            true
        }
        fn password(&self, _: &str, _: &str, _: &CancelToken) -> Option<Password> {
            *self.asked.lock() += 1;
            Some(Password {
                text: self.typed.clone(),
                remember: self.remember,
            })
        }
    }

    fn window(typed: &str, remember: bool) -> Arc<Window> {
        Arc::new(Window {
            typed: typed.to_owned(),
            remember,
            asked: parking_lot::Mutex::new(0),
        })
    }

    /// A keyring that asks to be unlocked waits for its own prompt, which
    /// the user may leave open: the connection's Cancel ends the wait at
    /// once, and the window is not asked.
    #[test]
    fn cancel_ends_a_wait_on_a_keyring_s_prompt() {
        struct Prompting(crossbeam_channel::Receiver<()>);
        impl Keyring for Prompting {
            fn own(&self, _: &str) -> Option<String> {
                let _ = self.0.recv();
                None
            }
            fn desktop(&self, _: &[(&str, &str)]) -> Option<String> {
                let _ = self.0.recv();
                None
            }
            fn keep(&self, _: &str, _: &str, _: &str) -> anyhow::Result<()> {
                let _ = self.0.recv();
                Ok(())
            }
            fn forget(&self, _: &str) -> anyhow::Result<()> {
                Ok(())
            }
        }
        let (_never, waits) = crossbeam_channel::bounded(0);
        let typed = window("typed", true);
        let cancel = CancelToken::new();
        let address = Address::parse("smb://me@nas/photos").unwrap();
        let keyed = Keyed::new(
            typed.clone(),
            Arc::new(Prompting(waits)),
            address,
            cancel.clone(),
        );
        let began = std::time::Instant::now();
        let cancelling = cancel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(200));
            cancelling.cancel();
        });
        assert!(keyed.password("Password", "Connect", &cancel).is_none());
        assert!(began.elapsed() < std::time::Duration::from_secs(2));
        assert_eq!(*typed.asked.lock(), 0, "the window not asked");
    }

    /// A real keyring locked, its unlock prompt left open: the
    /// connection's Cancel ends the wait, the window never asked; the
    /// prompt is then dismissed. Run by `tools/test/locked-keyring.sh`.
    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "needs a Secret Service that prompts (tools/test/locked-keyring.sh)"]
    fn cancel_ends_a_wait_on_a_locked_keyring() {
        // The other keyrings' runs pick it by its name too: it needs one
        // that prompts, and touches none other.
        if std::env::var("SKWIR_LOCKED_KEYRING").as_deref() != Ok("1") {
            eprintln!("skipped: run by tools/test/locked-keyring.sh");
            return;
        }
        let xdotool = std::env::var("XDOTOOL").expect("xdotool, to dismiss the prompt");
        let address = Address::parse("smb://me@nas.test/photos").unwrap();
        let server = key(&address, Some("me"));
        Desktop.keep(&server, "skwir test", "kept").unwrap();
        // Locked, as a session's keyring is after the screen was locked.
        let bus = zbus::blocking::Connection::session().unwrap();
        let default =
            zbus::zvariant::ObjectPath::try_from("/org/freedesktop/secrets/aliases/default")
                .unwrap();
        bus.call_method(
            Some("org.freedesktop.secrets"),
            "/org/freedesktop/secrets",
            Some("org.freedesktop.Secret.Service"),
            "Lock",
            &(vec![default],),
        )
        .unwrap();
        let typed = window("typed", false);
        let cancel = CancelToken::new();
        let keyed = Keyed::new(typed.clone(), Arc::new(Desktop), address, cancel.clone());
        let cancelling = cancel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(2));
            cancelling.cancel();
        });
        let began = std::time::Instant::now();
        assert!(keyed.password("Password", "Connect", &cancel).is_none());
        assert!(began.elapsed() < std::time::Duration::from_secs(5));
        assert_eq!(*typed.asked.lock(), 0, "the window not asked");
        // Left open, the prompt is the keyring's: dismissed here.
        assert!(
            skwir_platform_answer_prompt(&xdotool),
            "its prompt was there"
        );
    }

    /// Escape on the keyring's prompt, as `skwir_platform`'s test answers it.
    #[cfg(target_os = "linux")]
    fn skwir_platform_answer_prompt(xdotool: &str) -> bool {
        let run = |args: &[&str]| {
            std::process::Command::new(xdotool)
                .args(args)
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        };
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while std::time::Instant::now() < deadline {
            if let Some(found) = run(&["search", "--onlyvisible", "--class", "prompter"])
                && let Some(window) = found.lines().last()
            {
                run(&["windowfocus", "--sync", window]);
                run(&["key", "Escape"]);
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
        false
    }

    /// The desktop's keyring, never the user's: run only inside a private
    /// D-Bus session with a keyring of its own (`private-keyring.sh`, as
    /// `just remote-check` runs it), which sets the guard below.
    #[test]
    #[ignore = "needs a private Secret Service (tools/test/private-keyring.sh)"]
    fn a_server_s_password_goes_in_and_out_of_the_desktop_s_keyring() {
        // A keyring of the test's own: a private Secret Service, or the
        // Credential Manager of Wine's scratch prefix (`just test-windows`).
        assert!(
            std::env::var("SKWIR_PRIVATE_KEYRING").as_deref() == Ok("1")
                || (cfg!(windows) && std::env::var_os("SKWIR_TEST_UNDER_WINE").is_some()),
            "refusing to touch a keyring that is not the test's own"
        );
        // A server no other test of the shared keyring names.
        let address = Address::parse("smb://WORK;me@plugin-nas.test/photos").unwrap();
        let key = key(&address, address.user.as_deref());
        Desktop
            .keep(&key, &label(&address, Some("me")), "s3cret with spaces")
            .unwrap();
        assert_eq!(Desktop.own(&key).as_deref(), Some("s3cret with spaces"));
        Desktop.forget(&key).unwrap();
        assert_eq!(Desktop.own(&key), None);
        assert_eq!(
            Desktop.desktop(&[
                ("protocol", "smb"),
                ("server", "plugin-nas.test"),
                ("user", "me")
            ]),
            None,
            "Skwir's own is no GVfs's"
        );
    }

    #[test]
    fn a_server_is_keyed_by_its_login_never_its_path() {
        let nas = Address::parse("smb://WORK;me@nas:445/photos/2024").unwrap();
        assert_eq!(key(&nas, nas.user.as_deref()), "smb://WORK;me@nas");
        let ftp = Address::parse("ftp://h:2121/pub").unwrap();
        assert_eq!(key(&ftp, Some("you")), "ftp://you@h:2121");
        assert_eq!(label(&nas, Some("me")), "Skwir: me@nas (SMB)");
    }

    #[test]
    fn gvfs_s_passwords_are_found_as_gvfs_keeps_them() {
        let named = |text: &str, user: &str| {
            gvfs_named(&Address::parse(text).unwrap(), Some(user))
                .unwrap()
                .into_iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            named("sftp://h:2222/x", "me"),
            ["protocol=sftp", "server=h", "user=me", "port=2222"]
        );
        assert_eq!(
            named("smb://WORK;me@nas/s", "me"),
            ["protocol=smb", "server=nas", "user=me", "domain=WORK"]
        );
        assert_eq!(
            named("davs://cloud:8443/", "me"),
            ["protocol=http", "server=cloud", "user=me"],
            "WebDAV's port is not asked"
        );
        assert_eq!(
            named("ftp://h:21/", "me"),
            ["protocol=ftp", "server=h", "user=me"]
        );
        assert!(gvfs_named(&Address::parse("nfs://h/x").unwrap(), Some("me")).is_none());
    }

    #[test]
    fn a_server_s_password_is_told_from_a_passphrase_or_a_code() {
        assert!(is_server_password("me@example.com's password:"));
        assert!(is_server_password("(me@example.com) Password: "));
        assert!(!is_server_password(
            "Enter passphrase for key '/home/u/.ssh/id_ed25519':"
        ));
        assert!(!is_server_password("Verification code:"));
        assert!(!is_server_password("Password"), "rclone's configuration's");
    }

    #[test]
    fn the_keyring_answers_first_and_keeps_what_the_user_remembers() {
        let keyring = Arc::new(Memory::default());
        let address = Address::parse("smb://me@nas/photos").unwrap();
        let typed = window("typed", true);
        let keyed = Keyed::new(
            typed.clone(),
            keyring.clone(),
            address.clone(),
            CancelToken::new(),
        );
        let cancel = CancelToken::new();
        // Nothing kept: asked, remembered once the connection served.
        assert_eq!(keyed.password("t", "a", &cancel).unwrap().text, "typed");
        assert_eq!(keyed.served(), Some(Noted::Kept("smb://me@nas".into())));
        assert_eq!(*typed.asked.lock(), 1);
        // Kept: given, nothing asked.
        let keyed = Keyed::new(
            typed.clone(),
            keyring.clone(),
            address.clone(),
            CancelToken::new(),
        );
        assert_eq!(keyed.kept_password().as_deref(), Some("typed"));
        assert_eq!(keyed.served(), None);
        assert_eq!(*typed.asked.lock(), 1);
        // Refused (asked again): the window, and, answered without
        // Remember, the old one forgotten.
        let other = window("other", false);
        let keyed = Keyed::new(
            other.clone(),
            keyring.clone(),
            address.clone(),
            CancelToken::new(),
        );
        assert_eq!(keyed.password("t", "a", &cancel).unwrap().text, "typed");
        assert_eq!(keyed.password("t", "a", &cancel).unwrap().text, "other");
        assert_eq!(keyed.served(), Some(Noted::Forgot("smb://me@nas".into())));
        assert!(keyring.own.lock().is_empty());
    }

    #[test]
    fn gvfs_s_password_is_given_and_never_changed() {
        let keyring = Arc::new(Memory::default());
        keyring.desktop.lock().push((
            vec![
                ("protocol".into(), "smb".into()),
                ("server".into(), "nas".into()),
                ("user".into(), "me".into()),
            ],
            "nautilus kept it".into(),
        ));
        let typed = window("typed", false);
        let address = Address::parse("smb://me@nas/photos").unwrap();
        let keyed = Keyed::new(typed.clone(), keyring.clone(), address, CancelToken::new());
        let cancel = CancelToken::new();
        assert_eq!(
            keyed.password("t", "a", &cancel).unwrap().text,
            "nautilus kept it"
        );
        assert_eq!(keyed.served(), None, "kept nowhere, forgotten nowhere");
        assert_eq!(*typed.asked.lock(), 0);
        assert_eq!(keyring.desktop.lock().len(), 1);
    }
}
