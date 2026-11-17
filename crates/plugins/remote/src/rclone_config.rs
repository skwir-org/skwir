//! rclone's own configuration, read and changed from Skwir (ADR-086): the
//! remotes stay rclone's, in its configuration file, where `rclone` in a
//! terminal finds them too; Skwir keeps none. Every change goes through
//! rclone's remote control, which a short-lived `rclone rcd` serves on a
//! socket of the user's runtime folder (the user's alone, as a mount's,
//! `rclone_copy`) for as long as a form needs it: a secret travels in a
//! request's body, never on a command line, which every user of the
//! machine can read.
//!
//! What rclone knows of its kinds of storage (`config/providers`: each
//! option's help, default, examples, whether it is a secret, advanced)
//! makes the forms (`rclone_forms`): a kind rclone adds is offered with
//! no work here. Its configuration's own questions (a browser's sign-in,
//! a crypt's passwords) come as `Step::Question`, asked one at a time.
//!
//! A configuration kept encrypted opens with its password: the one the
//! keyring keeps for it (given in rclone's environment), else asked of the
//! window through rclone's `--password-command`.

use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde_json::{Value, json};
use skwir_core::cancel::CancelToken;
use skwir_core::words;
use skwir_core::words::Words;

use crate::connector::Ask;
use crate::mounter;

/// The keyring's key for the password of rclone's configuration, kept
/// there when the user asked (`passwords::Keyring`).
pub const KEYRING_KEY: &str = "rclone:configuration";

/// How long rclone may take to start serving.
const STARTING: Duration = Duration::from_secs(15);

/// A kind of storage rclone has (`config/providers`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Backend {
    /// Its type in the configuration: `s3`, `drive`.
    pub name: String,
    /// What rclone calls it in short: "Amazon S3", "Google Drive"; its
    /// description where it says none.
    pub title: String,
    /// What rclone says it is: "Amazon S3 Compliant Storage Providers…".
    pub description: String,
    pub options: Vec<Opt>,
}

/// One option of a [`Backend`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)] // rclone's own flags, each read alone
pub struct Opt {
    pub name: String,
    pub help: String,
    /// Its default, as rclone writes it.
    pub default: String,
    pub required: bool,
    pub advanced: bool,
    /// rclone keeps it obscured (`IsPassword`): a password.
    pub password: bool,
    /// It is a secret kept plain (`Sensitive`): an access key's secret.
    pub sensitive: bool,
    /// Only its examples may be chosen.
    pub exclusive: bool,
    /// Its type: `string`, `bool`, `int`, `SizeSuffix`, `Duration`…
    pub kind: String,
    pub examples: Vec<Example>,
    /// The providers it applies to (`AWS,Minio`, `!AWS`), empty for all.
    pub provider: String,
    /// Hidden from configuring (rclone's `Hide`).
    pub hidden: bool,
}

/// A value an [`Opt`] suggests.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Example {
    pub value: String,
    pub help: String,
    /// The providers it applies to, as `Opt::provider`.
    pub provider: String,
}

impl Opt {
    /// Whether what it holds is a secret, typed masked and never shown.
    pub fn secret(&self) -> bool {
        self.password || self.sensitive
    }

    /// Whether it applies to `provider` (rclone's own rule: a list of
    /// providers, or `!` and those it does not apply to).
    pub fn applies_to(&self, provider: &str) -> bool {
        applies(&self.provider, provider)
    }

    fn of(value: &Value) -> Opt {
        let text = |key: &str| value[key].as_str().unwrap_or_default().to_owned();
        let examples = value["Examples"]
            .as_array()
            .map(|examples| {
                examples
                    .iter()
                    .map(|e| Example {
                        value: e["Value"].as_str().unwrap_or_default().to_owned(),
                        help: e["Help"].as_str().unwrap_or_default().to_owned(),
                        provider: e["Provider"].as_str().unwrap_or_default().to_owned(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        Opt {
            name: text("Name"),
            help: text("Help"),
            default: text("DefaultStr"),
            required: value["Required"].as_bool().unwrap_or(false),
            advanced: value["Advanced"].as_bool().unwrap_or(false),
            password: value["IsPassword"].as_bool().unwrap_or(false),
            sensitive: value["Sensitive"].as_bool().unwrap_or(false),
            exclusive: value["Exclusive"].as_bool().unwrap_or(false),
            kind: text("Type"),
            examples,
            provider: text("Provider"),
            // rclone's OptionHideConfigurator (1) and OptionHideBoth (3).
            hidden: value["Hide"].as_i64().unwrap_or(0) & 1 != 0,
        }
    }
}

/// rclone's rule of which providers an option or an example applies to:
/// none said, all; `!a,b`, all but those; else those listed.
pub fn applies(rule: &str, provider: &str) -> bool {
    if rule.is_empty() {
        return true;
    }
    let (negated, list) = match rule.strip_prefix('!') {
        Some(list) => (true, list),
        None => (false, rule),
    };
    let listed = list.split(',').any(|p| p.trim() == provider);
    listed != negated
}

/// The backends `config/providers` answers, the hidden ones left out.
pub fn backends(answer: &Value) -> Vec<Backend> {
    answer["providers"]
        .as_array()
        .map(|providers| {
            providers
                .iter()
                .filter(|p| !p["Hide"].as_bool().unwrap_or(false))
                .map(|p| Backend {
                    name: p["Name"].as_str().unwrap_or_default().to_owned(),
                    title: p["Overview"]["Name"]
                        .as_str()
                        .filter(|t| !t.is_empty())
                        .or_else(|| p["Description"].as_str())
                        .unwrap_or_default()
                        .to_owned(),
                    description: p["Description"].as_str().unwrap_or_default().to_owned(),
                    options: p["Options"]
                        .as_array()
                        .map(|options| options.iter().map(Opt::of).collect())
                        .unwrap_or_default(),
                })
                .filter(|b| !b.name.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// Where a change of rclone's configuration is: done, or a question its
/// configuration asks (a browser's sign-in, a choice of drive).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    Done,
    Question {
        /// What the answer continues from.
        state: String,
        option: Opt,
        /// Why the last answer was refused.
        error: String,
    },
}

impl Step {
    /// What `config/create` or `config/update` answered.
    pub fn of(answer: &Value) -> Step {
        let state = answer["State"].as_str().unwrap_or_default();
        if state.is_empty() || answer["Option"].is_null() {
            return Step::Done;
        }
        Step::Question {
            state: state.to_owned(),
            option: Opt::of(&answer["Option"]),
            error: answer["Error"].as_str().unwrap_or_default().to_owned(),
        }
    }
}

/// Whether `name` may name a remote Skwir adds: what rclone takes
/// (`address::rclone_name`), which its address holds, escaped
/// (`rclone://my%20drive/`). Why not, where it may not.
pub fn name_refusal(name: &str) -> Option<Words> {
    if crate::address::rclone_name(name) {
        return None;
    }
    Some(if name.is_empty() {
        words!(remote_rclone_name_needed = "A remote needs a name")
    } else if name.starts_with(['-', ' ']) {
        words!(remote_rclone_name_start = "A remote's name starts with neither - nor a space")
    } else if name.ends_with(' ') {
        words!(remote_rclone_name_end = "A remote's name ends with no space")
    } else {
        words!(
            remote_rclone_name_letters =
                "A remote's name holds letters, digits, spaces and _ - . + @ only",
            note = "The signs after “spaces and” are typed as they are: underscore, hyphen, \
                    full stop, plus, at sign."
        )
    })
}

/// An `rclone rcd` serving the remote control on a socket of its own,
/// stopped when dropped.
pub struct Session {
    child: Child,
    /// Where its remote control answers (`rc`).
    at: crate::rc::At,
    /// What rclone said on its standard error, the last of it.
    said: Arc<Mutex<String>>,
    /// Where the configuration's password is asked, while it runs.
    _askpass: mounter::Askpass,
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        #[cfg(unix)]
        if let crate::rc::At::Socket(socket) = &self.at {
            let _ = std::fs::remove_file(socket);
        }
    }
}

/// The socket a session of this process serves on: named by the process,
/// so a session a process gone left behind can be told and stopped.
#[cfg(unix)]
fn session_socket() -> anyhow::Result<PathBuf> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let folder = crate::rclone_copy::socket_folder()?;
    Ok(folder.join(format!(
        "config-{}-{}.sock",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )))
}

/// Stop the sessions a Skwir gone left serving (it ended before it could):
/// each told to quit, its socket removed. On Windows, which serves on the
/// loopback, a session lives in Skwir's job and ends with it.
#[cfg(unix)]
fn sweep_left(cancel: &CancelToken) {
    let Some(folder) = crate::rclone_copy::sockets() else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(&folder) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(pid) = name
            .to_str()
            .and_then(|n| n.strip_prefix("config-"))
            .and_then(|n| n.split('-').next())
            .and_then(|pid| pid.parse::<u32>().ok())
        else {
            continue;
        };
        // Only once the system says that Skwir ended: one it cannot tell of
        // may still be serving.
        if pid == std::process::id() || skwir_platform::process::runs(pid) != Some(false) {
            continue;
        }
        let socket = entry.path();
        let _ = crate::rclone_copy::call(&socket, "core/quit", &json!({}), cancel);
        let _ = std::fs::remove_file(&socket);
    }
}

impl Session {
    /// Start serving with `rclone`, the configuration's password asked
    /// through `ask` where it is encrypted (`ConfigAsk`: the keyring's
    /// first).
    ///
    /// # Errors
    /// When rclone cannot start, or ends before it serves (a wrong
    /// password, a configuration it cannot read).
    pub fn start(
        rclone: &Path,
        with: &crate::rclone::Rclone,
        ask: &Arc<dyn Ask>,
        cancel: &CancelToken,
    ) -> anyhow::Result<Session> {
        #[cfg(unix)]
        let (answering, serving, login) = {
            sweep_left(cancel);
            let socket = session_socket()?;
            let _ = std::fs::remove_file(&socket);
            let serving = crate::rc::socket_serving(&socket);
            (
                Answering::At(crate::rc::At::Socket(socket)),
                serving,
                Vec::<(&str, String)>::new(),
            )
        };
        #[cfg(not(unix))]
        let (answering, serving, login) = {
            let login = crate::rc::Login::made()?;
            let (serving, env) = login.serving();
            (Answering::Said(login), serving, env)
        };
        let mut command = crate::rclone::command(rclone, with.config());
        command
            .args(["rcd", "--log-level", "NOTICE"])
            .args(serving)
            .envs(login)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .env_remove("RCLONE_CONFIG_PASS");
        // A configuration kept encrypted asks its password through Skwir,
        // run as rclone's password command.
        let askpass = {
            let program = with.askpass_program()?;
            // Its own token: dropped with the session, it ends that alone.
            let askpass = mounter::Askpass::serve_with(program.clone(), ask, &cancel.child())?;
            askpass.give(&mut command);
            command
                .arg("--password-command")
                .arg(crate::rclone::password_command(&program));
            askpass
        };
        let mut child = command.spawn()?;
        let said = Arc::new(Mutex::new(String::new()));
        if let Some(mut stderr) = child.stderr.take() {
            let said = Arc::clone(&said);
            std::thread::Builder::new()
                .name("skwir-rclone-said".into())
                .spawn(move || {
                    let mut chunk = [0u8; 4096];
                    while let Ok(n) = stderr.read(&mut chunk) {
                        if n == 0 {
                            break;
                        }
                        let mut said = said.lock();
                        said.push_str(&String::from_utf8_lossy(&chunk[..n]));
                        // The last of it is what tells.
                        if said.len() > 64 * 1024 {
                            let cut = said.len() - 32 * 1024;
                            let cut = (cut..said.len())
                                .find(|&i| said.is_char_boundary(i))
                                .unwrap_or(cut);
                            said.drain(..cut);
                        }
                    }
                })?;
        }
        let at = match wait_serving(&mut child, &answering, &said, cancel) {
            Ok(at) => at,
            // As a session dropped is let go of.
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                #[cfg(unix)]
                if let Answering::At(crate::rc::At::Socket(socket)) = &answering {
                    let _ = std::fs::remove_file(socket);
                }
                return Err(error);
            }
        };
        Ok(Session {
            child,
            at,
            said,
            _askpass: askpass,
        })
    }
}

/// Where a session's rclone is to answer: known before it starts (a socket
/// of the runtime folder, the user's alone), or the port of the loopback
/// it chose and says (`rc::served_port`), behind its login: one chosen
/// beforehand may be another program's by then, which would be told the
/// configuration.
enum Answering {
    #[cfg(unix)]
    At(crate::rc::At),
    #[cfg(not(unix))]
    Said(crate::rc::Login),
}

/// Where `child`, rclone serving a session, answers, once it does: what
/// it said so far (`said`) names its port on the loopback.
fn wait_serving(
    child: &mut Child,
    answering: &Answering,
    said: &Mutex<String>,
    cancel: &CancelToken,
) -> anyhow::Result<crate::rc::At> {
    let deadline = Instant::now() + STARTING;
    loop {
        let at = match answering {
            #[cfg(unix)]
            Answering::At(at) => Some(at.clone()),
            #[cfg(not(unix))]
            Answering::Said(login) => {
                crate::rc::served_port(said.lock().lines().map(str::to_owned))
                    .map(|port| login.at(port))
            }
        };
        if let Some(at) = at
            && at.answers()
        {
            return Ok(at);
        }
        // Ended: what it said is why.
        if child.try_wait().is_ok_and(|ended| ended.is_some()) {
            // What it said last is read whole first.
            std::thread::sleep(Duration::from_millis(50));
            anyhow::bail!(crate::rclone::words(&said.lock()));
        }
        anyhow::ensure!(!cancel.is_cancelled(), crate::connector::cancelled());
        anyhow::ensure!(
            Instant::now() < deadline,
            words!(remote_rclone_not_serving = "rclone did not start serving its configuration")
        );
        std::thread::sleep(Duration::from_millis(30));
    }
}

impl Session {
    /// What rclone said on its standard error so far.
    pub fn said(&self) -> String {
        self.said.lock().clone()
    }

    /// Call `method` with `args`.
    ///
    /// # Errors
    /// What rclone refused, in its words.
    pub fn call(&self, method: &str, args: &Value, cancel: &CancelToken) -> anyhow::Result<Value> {
        crate::rc::call(&self.at, method, args, cancel)
    }

    /// The kinds of storage rclone has.
    ///
    /// # Errors
    /// When rclone did not answer.
    pub fn backends(&self, cancel: &CancelToken) -> anyhow::Result<Vec<Backend>> {
        Ok(backends(&self.call(
            "config/providers",
            &json!({}),
            cancel,
        )?))
    }

    /// The remotes of the configuration, by name.
    ///
    /// # Errors
    /// When rclone did not answer.
    pub fn remotes(&self, cancel: &CancelToken) -> anyhow::Result<Vec<String>> {
        let answer = self.call("config/listremotes", &json!({}), cancel)?;
        Ok(answer["remotes"]
            .as_array()
            .map(|r| {
                r.iter()
                    .filter_map(|n| n.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default())
    }

    /// What the configuration holds of remote `name`, its type included
    /// (`type`); a password as rclone keeps it, obscured.
    ///
    /// # Errors
    /// When rclone did not answer.
    pub fn get(
        &self,
        name: &str,
        cancel: &CancelToken,
    ) -> anyhow::Result<serde_json::Map<String, Value>> {
        let answer = self.call("config/get", &json!({ "name": name }), cancel)?;
        Ok(answer.as_object().cloned().unwrap_or_default())
    }

    /// Add remote `name` of type `kind` with `parameters` (passwords plain:
    /// rclone obscures them), its configuration's questions returned.
    ///
    /// # Errors
    /// What rclone refused.
    pub fn create(
        &self,
        name: &str,
        kind: &str,
        parameters: &serde_json::Map<String, Value>,
        cancel: &CancelToken,
    ) -> anyhow::Result<Step> {
        let answer = self.call(
            "config/create",
            &json!({
                "name": name,
                "type": kind,
                "parameters": parameters,
                "opt": { "obscure": true, "nonInteractive": true },
            }),
            cancel,
        )?;
        Ok(Step::of(&answer))
    }

    /// Change `parameters` of remote `name` (passwords plain), its
    /// configuration's questions returned; with `all`, every question
    /// asked again (Reconnect).
    ///
    /// # Errors
    /// What rclone refused.
    pub fn update(
        &self,
        name: &str,
        parameters: &serde_json::Map<String, Value>,
        all: bool,
        cancel: &CancelToken,
    ) -> anyhow::Result<Step> {
        let answer = self.call(
            "config/update",
            &json!({
                "name": name,
                "parameters": parameters,
                "opt": { "obscure": true, "nonInteractive": true, "all": all },
            }),
            cancel,
        )?;
        Ok(Step::of(&answer))
    }

    /// Answer `result` to the question of remote `name` at `state`.
    ///
    /// # Errors
    /// What rclone refused.
    pub fn answer(
        &self,
        name: &str,
        state: &str,
        result: &str,
        cancel: &CancelToken,
    ) -> anyhow::Result<Step> {
        let answer = self.call(
            "config/update",
            &json!({
                "name": name,
                "parameters": {},
                "opt": {
                    "obscure": true,
                    "nonInteractive": true,
                    "continue": true,
                    "state": state,
                    "result": result,
                },
            }),
            cancel,
        )?;
        Ok(Step::of(&answer))
    }

    /// Remove remote `name` from the configuration.
    ///
    /// # Errors
    /// What rclone refused.
    pub fn delete(&self, name: &str, cancel: &CancelToken) -> anyhow::Result<()> {
        self.call("config/delete", &json!({ "name": name }), cancel)?;
        Ok(())
    }

    /// List the top of `fs` (a remote, `name:`, or one made of its parts),
    /// as connecting would: whether it answers.
    ///
    /// # Errors
    /// Why it did not, in rclone's words.
    pub fn list_top(&self, fs: &str, cancel: &CancelToken) -> anyhow::Result<()> {
        self.call(
            "operations/list",
            &json!({ "fs": fs, "remote": "", "opt": { "dirsOnly": true, "noModTime": true, "noMimeType": true } }),
            cancel,
        )?;
        Ok(())
    }

    /// Where a browser's sign-in waits, while one does
    /// (`config/oauthstatus`).
    pub fn sign_in_page(&self, cancel: &CancelToken) -> Option<String> {
        let answer = self.call("config/oauthstatus", &json!({}), cancel).ok()?;
        (answer["status"] == "running")
            .then(|| answer["authUrl"].as_str().map(str::to_owned))
            .flatten()
            .filter(|url| !url.is_empty())
    }

    /// Give up the browser's sign-in rclone waits for.
    pub fn stop_sign_in(&self, cancel: &CancelToken) {
        let _ = self.call("config/oauthstop", &json!({}), cancel);
    }
}

/// Where the configuration `with` names is (rclone's own where none).
///
/// # Errors
/// When rclone cannot say.
pub fn config_file(
    rclone: &Path,
    with: &crate::rclone::Rclone,
    cancel: &CancelToken,
) -> anyhow::Result<PathBuf> {
    if let Some(config) = with.config() {
        return Ok(config.to_path_buf());
    }
    let mut command = crate::rclone::command(rclone, None);
    command.args(["config", "file", "--ask-password=false"]);
    let out = skwir_platform::process::run(&mut command, cancel, Duration::from_secs(10))?;
    let out = String::from_utf8_lossy(&out);
    let path = out
        .lines()
        .rev()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .ok_or_else(|| {
            anyhow::anyhow!(words!(
                remote_rclone_config_unsaid = "rclone did not say where its configuration is"
            ))
        })?;
    Ok(PathBuf::from(path))
}

/// The line an encrypted configuration of rclone's starts with.
const ENCRYPTED: &str = "# Encrypted rclone configuration File";

/// Whether the configuration `with` names is encrypted: its first line
/// says so. One not made yet is not.
///
/// # Errors
/// When rclone cannot say where it is, or it cannot be read.
pub fn encrypted(
    rclone: &Path,
    with: &crate::rclone::Rclone,
    cancel: &CancelToken,
) -> anyhow::Result<bool> {
    let path = config_file(rclone, with, cancel)?;
    Ok(file_encrypted(&path)?)
}

/// Whether the configuration at `path` is encrypted: its first line says
/// so. One not made yet is not.
pub(crate) fn file_encrypted(path: &Path) -> std::io::Result<bool> {
    match std::fs::File::open(path) {
        Ok(file) => {
            use std::io::BufRead as _;
            let mut first = String::new();
            std::io::BufReader::new(file).read_line(&mut first)?;
            Ok(first.trim_end() == ENCRYPTED)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

/// An answer given before it is asked: the password a configuration is
/// encrypted with.
struct Given(String);

impl Ask for Given {
    fn secret(&self, _: &Words, _: &Words, _: &CancelToken) -> Option<String> {
        Some(self.0.clone())
    }
    fn confirm(&self, _: &Words, _: &Words, _: &CancelToken) -> bool {
        false
    }
}

/// Encrypt the configuration `with` names with `password`
/// (`rclone config encryption set`), given it through rclone's password
/// command, never a command line.
///
/// # Errors
/// What rclone said, failing.
pub fn encrypt(
    rclone: &Path,
    with: &crate::rclone::Rclone,
    password: &str,
    cancel: &CancelToken,
) -> anyhow::Result<()> {
    let program = with.askpass_program()?;
    let given: Arc<dyn Ask> = Arc::new(Given(password.to_owned()));
    let askpass = mounter::Askpass::serve_with(program.clone(), &given, &cancel.child())?;
    let mut command = crate::rclone::command(rclone, with.config());
    command
        .args(["config", "encryption", "set", "--password-command"])
        .arg(crate::rclone::password_command(&program))
        .env_remove("RCLONE_CONFIG_PASS");
    askpass.give(&mut command);
    let finished =
        skwir_platform::process::run_to_exit(&mut command, None, cancel, Duration::from_secs(60))?;
    drop(askpass);
    anyhow::ensure!(
        finished.status.success(),
        crate::rclone::words(&finished.stderr)
    );
    Ok(())
}

/// Where the password of rclone's configuration is asked: the keyring's,
/// where the user asked it kept (once: a wrong one is asked of the
/// window), then the window. It is what rclone asks run with Skwir as its
/// `--password-command`, which gives no prompt (`Ask::unprompted`). Every
/// other question goes to `inner`.
pub struct ConfigAsk {
    inner: Arc<dyn Ask>,
    keyring: Arc<dyn crate::passwords::Keyring>,
    tried: std::sync::atomic::AtomicBool,
}

impl ConfigAsk {
    pub fn new(inner: Arc<dyn Ask>, keyring: Arc<dyn crate::passwords::Keyring>) -> Self {
        ConfigAsk {
            inner,
            keyring,
            tried: std::sync::atomic::AtomicBool::new(false),
        }
    }
}

impl Ask for ConfigAsk {
    fn secret(&self, title: &Words, accept: &Words, cancel: &CancelToken) -> Option<String> {
        self.inner.secret(title, accept, cancel)
    }

    /// rclone's question, asked with no prompt: its configuration's
    /// password.
    fn unprompted(&self, _: &Words, cancel: &CancelToken) -> Option<String> {
        if !self.tried.swap(true, std::sync::atomic::Ordering::AcqRel)
            && let Some(kept) = self.keyring.own(KEYRING_KEY)
        {
            return Some(kept);
        }
        self.inner.secret(
            &words!(remote_rclone_config_password = "Password of rclone's configuration"),
            &words!(
                remote_rclone_config_open = "Open",
                note = "The button that gives the password of rclone's configuration, opening it."
            ),
            cancel,
        )
    }

    fn confirm(&self, title: &Words, detail: &Words, cancel: &CancelToken) -> bool {
        self.inner.confirm(title, detail, cancel)
    }

    fn password(
        &self,
        title: &Words,
        accept: &Words,
        cancel: &CancelToken,
    ) -> Option<crate::connector::Password> {
        self.inner.password(title, accept, cancel)
    }

    fn kept_password(&self) -> Option<String> {
        self.inner.kept_password()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The rclone a test runs: `SKWIR_TEST_RCLONE` (rclone's own Windows
    /// release, under Wine: `just test-windows`), else the one installed.
    fn rclone() -> Option<PathBuf> {
        std::env::var_os("SKWIR_TEST_RCLONE")
            .map(PathBuf::from)
            .or_else(|| skwir_core::os_path::find_program("rclone"))
    }

    /// Asks nothing: a configuration not encrypted needs no answer.
    struct Nobody;

    impl Ask for Nobody {
        fn secret(&self, _: &Words, _: &Words, _: &CancelToken) -> Option<String> {
            None
        }
        fn confirm(&self, _: &Words, _: &Words, _: &CancelToken) -> bool {
            false
        }
    }

    /// A session changes rclone's configuration through its remote control
    /// (a socket on Unix, the loopback behind a login on Windows): a remote
    /// made with its password obscured, read back, changed, removed. Under
    /// Wine with rclone's own Windows release (`just test-windows`), on
    /// Linux with the one installed; skipped, saying why, with none.
    #[test]
    fn a_session_changes_rclone_s_configuration_through_its_remote_control() {
        let Some(rclone) = rclone() else {
            eprintln!("skipped: rclone is not installed");
            return;
        };
        let folder = tempfile::tempdir().unwrap();
        let config = folder.path().join("rclone.conf");
        let with = crate::rclone::Rclone::with_config(config.clone());
        let ask: Arc<dyn Ask> = Arc::new(Nobody);
        let cancel = CancelToken::new();
        let session = Session::start(&rclone, &with, &ask, &cancel).unwrap();
        assert!(
            session
                .backends(&cancel)
                .unwrap()
                .iter()
                .any(|b| b.name == "webdav")
        );
        let mut parameters = serde_json::Map::new();
        parameters.insert("url".into(), json!("http://127.0.0.1:1/"));
        parameters.insert("pass".into(), json!("it's s3cret"));
        let step = session
            .create("nas-1", "webdav", &parameters, &cancel)
            .unwrap();
        assert_eq!(step, Step::Done);
        assert_eq!(session.remotes(&cancel).unwrap(), ["nas-1"]);
        let held = session.get("nas-1", &cancel).unwrap();
        assert_eq!(held["type"], "webdav");
        assert_ne!(held["pass"], "it's s3cret", "kept obscured");
        let mut changed = serde_json::Map::new();
        changed.insert("url".into(), json!("http://127.0.0.1:2/"));
        session.update("nas-1", &changed, false, &cancel).unwrap();
        assert_eq!(
            session.get("nas-1", &cancel).unwrap()["url"],
            "http://127.0.0.1:2/"
        );
        session.delete("nas-1", &cancel).unwrap();
        assert!(session.remotes(&cancel).unwrap().is_empty());
        drop(session);
        let text = std::fs::read_to_string(&config).unwrap_or_default();
        assert!(!text.contains("s3cret"), "{text}");
    }

    /// An rclone a test started, stopped however the test ends.
    struct Serving(Child);

    impl Drop for Serving {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    /// On the loopback, rclone's remote control serves on a port of its
    /// own choosing, which it says, and takes the login made for it alone:
    /// another is refused.
    #[test]
    fn a_remote_control_on_the_loopback_refuses_another_login() {
        let Some(rclone) = rclone() else {
            eprintln!("skipped: rclone is not installed");
            return;
        };
        let folder = tempfile::tempdir().unwrap();
        let made = crate::rc::Login::made().unwrap();
        let (serving, login) = made.serving();
        let mut serving = Serving(
            crate::rclone::command(&rclone, Some(&folder.path().join("rclone.conf")))
                .args(["rcd", "--log-level", "NOTICE"])
                .args(serving)
                .envs(login)
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap(),
        );
        // It says its port as `Session::start` reads it; what follows is
        // read on, so rclone never writes to a pipe let go of.
        let stderr = serving.0.stderr.take().unwrap();
        let (port_tx, port_rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for line in
                std::io::BufRead::lines(std::io::BufReader::new(stderr)).map_while(Result::ok)
            {
                if let Some(port) = crate::rc::served_port(std::iter::once(line)) {
                    let _ = port_tx.send(port);
                }
            }
        });
        let port = port_rx
            .recv_timeout(Duration::from_secs(60))
            .expect("rclone says where it serves");
        let at = made.at(port);
        let deadline = Instant::now() + Duration::from_secs(60);
        while !at.answers() {
            assert!(Instant::now() < deadline, "rclone never served");
            std::thread::sleep(Duration::from_millis(100));
        }
        let cancel = CancelToken::new();
        let version = crate::rc::call(&at, "core/version", &json!({}), &cancel).unwrap();
        assert!(version["version"].as_str().unwrap().starts_with('v'));
        #[allow(irrefutable_let_patterns)] // a socket too, on Unix
        let crate::rc::At::Loopback { port, user, .. } = at.clone() else {
            panic!("a loopback");
        };
        let other = crate::rc::At::Loopback {
            port,
            user,
            pass: "not its own".into(),
        };
        let refused = crate::rc::call(&other, "core/version", &json!({}), &cancel).unwrap_err();
        assert!(
            format!("{refused:#}").contains("login was refused"),
            "{refused:#}"
        );
        let _ = crate::rc::call(&at, "core/quit", &json!({}), &cancel);
    }

    #[test]
    fn an_option_applies_to_the_providers_rclone_says() {
        assert!(applies("", "AWS"));
        assert!(applies("AWS,Minio", "Minio"));
        assert!(!applies("AWS,Minio", "Ceph"));
        assert!(applies("!AWS,Minio", "Ceph"));
        assert!(!applies("!AWS,Minio", "AWS"));
    }

    #[test]
    fn rclone_s_questions_are_steps() {
        assert_eq!(
            Step::of(&json!({ "State": "", "Option": null })),
            Step::Done
        );
        let step = Step::of(&json!({
            "State": "*oauth-islocal,teamdrive,,",
            "Option": {
                "Name": "config_is_local",
                "Help": "Use web browser to automatically authenticate rclone with remote?",
                "DefaultStr": "true",
                "Type": "bool",
                "Exclusive": true,
                "Examples": [{ "Value": "true", "Help": "Yes" }, { "Value": "false", "Help": "No" }],
            },
            "Error": "",
        }));
        let Step::Question { state, option, .. } = step else {
            panic!("a question");
        };
        assert_eq!(state, "*oauth-islocal,teamdrive,,");
        assert_eq!(option.name, "config_is_local");
        assert!(option.exclusive);
        assert_eq!(option.examples.len(), 2);
    }

    /// A remote is named as rclone names one (its `config create` took
    /// each of these, and refused the others, 1.75.2), and its address
    /// holds the name, escaped, reading back to it.
    #[test]
    fn a_remote_is_named_as_rclone_takes_it_and_its_address_holds_it() {
        for taken in [
            "photos-2024.nas+old_one",
            "my drive",
            "x  y",
            "me@nas",
            "@at",
            "photos-é",
            "Фото",
            "写真",
            ".dot",
            "2024",
        ] {
            assert_eq!(name_refusal(taken), None, "{taken}");
            let address = crate::address::Address::rclone(taken, b"/Photos/it's 2024").unwrap();
            let text = address.to_string();
            let written = text["rclone://".len()..].split('/').next().unwrap();
            assert!(
                written.is_ascii() && !written.contains(['@', ' ']),
                "{text}"
            );
            let back = crate::address::Address::parse(&text).unwrap();
            assert_eq!(
                (back.host.as_str(), back.path.as_slice()),
                (taken, b"/Photos/it's 2024".as_slice())
            );
            assert_eq!(back, address, "{text}");
        }
        assert_eq!(
            crate::address::Address::rclone("my drive", b"/")
                .unwrap()
                .to_string(),
            "rclone://my%20drive/"
        );
        for refused in [
            "", "a/b", "a:b", "-a", " lead", "trail ", "a%b", "a'b", "a,b",
        ] {
            assert!(name_refusal(refused).is_some(), "{refused}");
            assert!(
                crate::address::Address::rclone(refused, b"/").is_err(),
                "{refused}"
            );
        }
        // Typed, an `@` is the name's: an rclone remote has no user.
        let typed = crate::address::Address::parse("rclone://me@nas/x").unwrap();
        assert_eq!(
            (typed.host.as_str(), typed.user.as_deref()),
            ("me@nas", None)
        );
    }

    #[test]
    fn the_backends_are_read_from_what_rclone_describes() {
        let answer = json!({ "providers": [
            { "Name": "s3", "Description": "Amazon S3", "Hide": false, "Options": [
                { "Name": "provider", "Help": "Choose your S3 provider.", "DefaultStr": "",
                  "Examples": [{ "Value": "AWS", "Help": "Amazon Web Services (AWS) S3" }] },
                { "Name": "secret_access_key", "Sensitive": true, "Type": "string" },
                { "Name": "region", "Provider": "AWS,Minio", "Type": "string" },
                { "Name": "chunk_size", "Advanced": true, "DefaultStr": "5Mi", "Type": "SizeSuffix" },
                { "Name": "description", "Hide": 3, "Advanced": true },
            ] },
            { "Name": "hidden", "Hide": true, "Options": [] },
        ] });
        let backends = backends(&answer);
        assert_eq!(backends.len(), 1);
        let s3 = &backends[0];
        assert_eq!(s3.options[1].name, "secret_access_key");
        assert!(s3.options[1].secret());
        assert!(s3.options[2].applies_to("Minio"));
        assert!(!s3.options[2].applies_to("Ceph"));
        assert!(s3.options[3].advanced);
        assert_eq!(s3.options[3].default, "5Mi");
        assert!(s3.options[4].hidden);
    }
}
