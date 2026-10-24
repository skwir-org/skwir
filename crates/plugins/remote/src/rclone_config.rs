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

/// Whether `name` may name a remote Skwir adds: rclone takes more
/// (spaces, `@`, letters of any alphabet), but its address must hold it
/// (`rclone://name/`): ASCII letters, digits and `_ - . +`, starting with
/// neither `-` nor `.`. Why not, where it may not.
pub fn name_refusal(name: &str) -> Option<&'static str> {
    if name.is_empty() {
        return Some("A remote needs a name");
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "_-.+".contains(c))
    {
        return Some("A remote's name holds letters a–z, digits and _ - . + only");
    }
    if name.starts_with(['-', '.']) {
        return Some("A remote's name starts with neither - nor .");
    }
    None
}

/// An `rclone rcd` serving the remote control on a socket of its own,
/// stopped when dropped.
pub struct Session {
    child: Child,
    socket: PathBuf,
    /// What rclone said on its standard error, the last of it.
    said: Arc<Mutex<String>>,
    /// Where the configuration's password is asked, while it runs.
    _askpass: mounter::Askpass,
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.socket);
    }
}

/// The socket a session of this process serves on: named by the process,
/// so a session a process gone left behind can be told and stopped.
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
/// each told to quit, its socket removed.
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
        if pid == std::process::id() || skwir_platform::process::runs(pid) == Some(true) {
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
        sweep_left(cancel);
        let socket = session_socket()?;
        let _ = std::fs::remove_file(&socket);
        let mut at = std::ffi::OsString::from("unix://");
        at.push(&socket);
        let mut command = crate::rclone::command(rclone, with.config());
        command
            .args(["rcd", "--rc-no-auth", "--log-level", "NOTICE", "--rc-addr"])
            .arg(at)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        // A configuration kept encrypted asks its password through Skwir,
        // run as rclone's password command.
        let program = with.askpass_program()?;
        // Its own token: dropped with the session, it ends that alone.
        let askpass = mounter::Askpass::serve_with(program.clone(), ask, &cancel.child())?;
        askpass.give(&mut command);
        command
            .arg("--password-command")
            .arg(program)
            .env_remove("RCLONE_CONFIG_PASS");
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
        let session = Session {
            child,
            socket,
            said,
            _askpass: askpass,
        };
        session.wait_serving(cancel)?;
        Ok(session)
    }

    fn wait_serving(&self, cancel: &CancelToken) -> anyhow::Result<()> {
        let deadline = Instant::now() + STARTING;
        loop {
            if std::os::unix::net::UnixStream::connect(&self.socket).is_ok() {
                return Ok(());
            }
            // Ended: what it said is why.
            if !self.socket.exists() && skwir_platform::process::runs(self.child.id()) != Some(true)
            {
                anyhow::bail!("{}", crate::rclone::words(&self.said.lock()));
            }
            anyhow::ensure!(!cancel.is_cancelled(), "cancelled");
            anyhow::ensure!(
                Instant::now() < deadline,
                "rclone did not start serving its configuration"
            );
            std::thread::sleep(Duration::from_millis(30));
        }
    }

    /// What rclone said on its standard error so far.
    pub fn said(&self) -> String {
        self.said.lock().clone()
    }

    /// Call `method` with `args`.
    ///
    /// # Errors
    /// What rclone refused, in its words.
    pub fn call(&self, method: &str, args: &Value, cancel: &CancelToken) -> anyhow::Result<Value> {
        crate::rclone_copy::call(&self.socket, method, args, cancel)
    }

    /// The kinds of storage rclone has.
    ///
    /// # Errors
    /// When rclone did not answer.
    pub fn backends(&self, cancel: &CancelToken) -> anyhow::Result<Vec<Backend>> {
        Ok(backends(&self.call("config/providers", &json!({}), cancel)?))
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
        .ok_or_else(|| anyhow::anyhow!("rclone did not say where its configuration is"))?;
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
    match std::fs::File::open(&path) {
        Ok(file) => {
            use std::io::BufRead as _;
            let mut first = String::new();
            std::io::BufReader::new(file).read_line(&mut first)?;
            Ok(first.trim_end() == ENCRYPTED)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

/// An answer given before it is asked: the password a configuration is
/// encrypted with.
struct Given(String);

impl Ask for Given {
    fn secret(&self, _: &str, _: &str, _: &CancelToken) -> Option<String> {
        Some(self.0.clone())
    }
    fn confirm(&self, _: &str, _: &str, _: &CancelToken) -> bool {
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
        .arg(program)
        .env_remove("RCLONE_CONFIG_PASS");
    askpass.give(&mut command);
    let finished =
        skwir_platform::process::run_to_exit(&mut command, None, cancel, Duration::from_secs(60))?;
    drop(askpass);
    anyhow::ensure!(
        finished.status.success(),
        "{}",
        crate::rclone::words(&finished.stderr)
    );
    Ok(())
}

/// What rclone's own question asks, run as its `--password-command`
/// (`skwir` given no prompt): its configuration's password.
pub const PROMPT: &str = "Password";

/// Where the password of rclone's configuration is asked: the keyring's,
/// where the user asked it kept (once: a wrong one is asked of the
/// window), then the window. Every other question goes to `inner`.
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
    fn secret(&self, title: &str, accept: &str, cancel: &CancelToken) -> Option<String> {
        if title != PROMPT {
            return self.inner.secret(title, accept, cancel);
        }
        if !self.tried.swap(true, std::sync::atomic::Ordering::AcqRel)
            && let Some(kept) = self.keyring.own(KEYRING_KEY)
        {
            return Some(kept);
        }
        self.inner
            .secret("Password of rclone's configuration", "Open", cancel)
    }

    fn confirm(&self, title: &str, detail: &str, cancel: &CancelToken) -> bool {
        self.inner.confirm(title, detail, cancel)
    }

    fn password(
        &self,
        title: &str,
        accept: &str,
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
        assert_eq!(Step::of(&json!({ "State": "", "Option": null })), Step::Done);
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

    #[test]
    fn a_remote_is_named_as_its_address_holds_it() {
        assert_eq!(name_refusal("photos-2024.nas+old_one"), None);
        for refused in ["", "a/b", "a:b", "-a", ".a", "my photos", "me@nas", "photos-é"] {
            assert!(name_refusal(refused).is_some(), "{refused}");
            if !refused.is_empty() && !refused.contains([':', '/']) {
                // What Skwir refuses is what its address cannot hold: none,
                // or one naming another (a user at a server).
                let address = crate::address::Address::parse(&format!("rclone://{refused}/"));
                assert!(address.is_err() || address.unwrap().host != refused, "{refused}");
            }
        }
        assert!(crate::address::Address::parse("rclone://photos-2024.nas+old_one/").is_ok());
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
