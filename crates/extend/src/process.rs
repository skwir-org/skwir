//! A plugin that runs as a program (Level 2, ADR-045): started isolated
//! when first asked (`sandbox::plugin`), the protocol on its pipes, one
//! call at a time, its `read`s answered from the call's files; ended once
//! idle for [`IDLE`], when it is turned off, and with bibli (`bwrap`'s
//! `--die-with-parent`). A program that stops, or that stops answering
//! (`Broken`), is ended and started again at the next call, not before
//! [`AGAIN`] after one that could not start; turned off, it is never
//! started again by a call still under way.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use bibli_core::permissions::{Capabilities, Capability};
use bibli_plugin_api::manifest::Capability as Granted;
use bibli_plugin_api::protocol::{
    InitializeAnswer, InitializeParams, LogParams, MESSAGE_LIMIT, Message, OLDEST, ReadAnswer,
    ReadParams, VERSION, method,
};
use crossbeam_channel::{Receiver, RecvTimeoutError, Sender};
use parking_lot::Mutex;
use serde_json::{Value, json};

use crate::call::{Call, Transport};

/// How long a program waits unasked before it is ended.
const IDLE: Duration = Duration::from_secs(30);
/// How long an answer is waited for.
const ANSWER: Duration = Duration::from_secs(30);
/// How long a program has to say it is ready.
const START: Duration = Duration::from_secs(10);
/// How long after a program could not start it is not tried again.
const AGAIN: Duration = Duration::from_secs(10);
/// Messages from a program not taken yet, at most: a program that writes
/// between calls waits for the next one, its output never piling up here.
const WAITING: usize = 1024;
/// Messages to a program not written yet, at most: one that stops reading
/// its input has its call fail when the next one finds no room.
const OUTGOING: usize = 16;

/// A program that failed as a program, not an answer of its: it did not
/// answer in time, stopped, or broke the protocol. It is ended, and the
/// plugin turned off when it does so again and again (`Runner::call`).
#[derive(Debug)]
pub(crate) struct Broken(pub String);

impl std::fmt::Display for Broken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Broken {}

/// Whether a program may run where nothing can isolate it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Isolation {
    /// Never: a plugin's own program reaches what its process reaches.
    Required,
    /// Isolated where the system can, plainly where not: bibli's helper
    /// running a module, which reaches only bibli's imports anyway.
    WhereAvailable,
}

pub(crate) struct Program {
    id: &'static str,
    path: PathBuf,
    args: Vec<String>,
    folder: PathBuf,
    granted: Capabilities,
    isolation: Isolation,
    /// A folder it works in from one start to the next, in place of a
    /// fresh one each time: a module's helper keeps the module compiled
    /// there.
    kept: Option<PathBuf>,
    running: Arc<Mutex<Option<Running>>>,
    failed: Mutex<Option<Instant>>,
    /// Turned off (`stop`): a call still under way ends, and none starts
    /// the program again.
    stopped: Arc<AtomicBool>,
    /// How long an answer is waited for: `ANSWER`, shorter in tests.
    patience: Duration,
}

/// A program started, its pipes and its working folder. Dropped however
/// it is, it ends its program and reaps it: never a zombie.
struct Running {
    child: Child,
    /// Lines for its input, written by a thread of its own: a program
    /// that stops reading holds that thread, never a call.
    stdin: Option<Sender<Vec<u8>>>,
    messages: Receiver<Message>,
    next: u64,
    /// Each call says it was asked here, for the watch that ends it idle.
    used: Sender<()>,
    /// Its plugin's (`Program::stopped`).
    stopped: Arc<AtomicBool>,
    _work: Work,
}

impl Drop for Running {
    fn drop(&mut self) {
        drop(self.stdin.take());
        if !matches!(self.child.try_wait(), Ok(Some(_))) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

/// The folder a program works in.
enum Work {
    /// Made for this start, deleted when it ends.
    Fresh(tempfile::TempDir),
    /// Kept from one start to the next (`Program::kept`).
    Kept(PathBuf),
}

impl Work {
    fn path(&self) -> &Path {
        match self {
            Work::Fresh(folder) => folder.path(),
            Work::Kept(folder) => folder,
        }
    }
}

impl Program {
    pub fn new(
        id: &'static str,
        program: PathBuf,
        args: Vec<String>,
        folder: PathBuf,
        granted: Capabilities,
        isolation: Isolation,
        kept: Option<PathBuf>,
    ) -> Program {
        Program {
            id,
            path: program,
            args,
            folder,
            granted,
            isolation,
            kept,
            running: Arc::default(),
            failed: Mutex::new(None),
            stopped: Arc::default(),
            patience: ANSWER,
        }
    }

    fn start(&self) -> anyhow::Result<Running> {
        let work = match &self.kept {
            Some(folder) => {
                std::fs::create_dir_all(folder)?;
                Work::Kept(folder.clone())
            }
            None => Work::Fresh(tempfile::Builder::new().prefix("bibli-plugin-").tempdir()?),
        };
        let isolated = bibli_platform::sandbox::plugin(
            &self.path,
            &self.folder,
            work.path(),
            self.granted.has(Capability::Network),
        );
        let mut command = match (isolated, self.isolation) {
            (Ok(command), _) => command,
            (Err(_), Isolation::WhereAvailable) => {
                let mut plain = std::process::Command::new(&self.path);
                plain.current_dir(work.path());
                plain
            }
            (Err(error), Isolation::Required) => return Err(error),
        };
        command
            .args(&self.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn()?;
        let (stdin, stdout, stderr) =
            (child.stdin.take(), child.stdout.take(), child.stderr.take());
        let (tx, messages) = crossbeam_channel::bounded(WAITING);
        let id = self.id;
        let stdin = match stdin {
            Some(mut pipe) => {
                let (lines, waiting) = crossbeam_channel::bounded::<Vec<u8>>(OUTGOING);
                std::thread::Builder::new()
                    .name(format!("plugin {id} input"))
                    .spawn(move || {
                        for line in waiting {
                            if pipe.write_all(&line).and_then(|()| pipe.flush()).is_err() {
                                return;
                            }
                        }
                    })?;
                Some(lines)
            }
            None => None,
        };
        if let Some(stdout) = stdout {
            std::thread::Builder::new()
                .name(format!("plugin {id}"))
                .spawn(move || {
                    let mut reader = BufReader::new(stdout);
                    while let Some(Ok(message)) =
                        bibli_core::jsonl::read::<Message>(&mut reader, MESSAGE_LIMIT)
                    {
                        if tx.send(message).is_err() {
                            break;
                        }
                    }
                })?;
        }
        if let Some(stderr) = stderr {
            std::thread::Builder::new()
                .name(format!("plugin {id} errors"))
                .spawn(move || {
                    // A line at most `LINE` long is held: a longer one is
                    // logged in pieces.
                    const LINE: u64 = 4096;
                    let mut errors = BufReader::new(stderr);
                    let mut line = Vec::new();
                    loop {
                        line.clear();
                        match (&mut errors).take(LINE).read_until(b'\n', &mut line) {
                            Ok(0) | Err(_) => return,
                            Ok(_) => {
                                let text = String::from_utf8_lossy(&line);
                                let text: String = text.trim_end().chars().take(500).collect();
                                tracing::debug!(plugin = id, "{text}");
                            }
                        }
                    }
                })?;
        }
        let (used, asked) = crossbeam_channel::unbounded();
        watch_idle(Arc::downgrade(&self.running), asked);
        let mut running = Running {
            child,
            stdin,
            messages,
            next: 1,
            used,
            stopped: Arc::clone(&self.stopped),
            _work: work,
        };
        let capabilities = self
            .granted
            .iter()
            .map(|capability| match capability {
                Capability::Read => Granted::Read,
                Capability::Write => Granted::Write,
                Capability::Network => Granted::Network,
                Capability::Programs => Granted::Programs,
            })
            .collect();
        let hello = InitializeParams {
            api: VERSION,
            capabilities,
        };
        let answer = running.exchange(
            method::INITIALIZE,
            json!(hello),
            &Call::new(
                Vec::new(),
                Capabilities::NONE,
                bibli_core::CancelToken::new(),
            ),
            START,
        );
        let answer: InitializeAnswer = match answer.and_then(|a| Ok(serde_json::from_value(a)?)) {
            Ok(answer) => answer,
            Err(error) => {
                running.end();
                return Err(error.context("the plugin did not start"));
            }
        };
        // One written for an older version runs as it was: it is never
        // asked what that version did not have.
        if !(OLDEST..=VERSION).contains(&answer.api) {
            running.end();
            anyhow::bail!(
                "the plugin speaks version {} of the protocol; bibli speaks {OLDEST} to {VERSION}",
                answer.api
            );
        }
        Ok(running)
    }
}

/// End the program `running` holds once it was not asked for [`IDLE`];
/// asked meanwhile (a call holding it), wait on. Ends with it.
fn watch_idle(running: Weak<Mutex<Option<Running>>>, asked: Receiver<()>) {
    let watch = move || {
        loop {
            match asked.recv_timeout(IDLE) {
                Ok(()) => {}
                Err(RecvTimeoutError::Timeout) => {
                    let Some(running) = running.upgrade() else {
                        return;
                    };
                    let Some(mut held) = running.try_lock() else {
                        continue;
                    };
                    if let Some(idle) = held.take() {
                        idle.end();
                    }
                    return;
                }
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
    };
    if let Err(error) = std::thread::Builder::new()
        .name("plugin idle".into())
        .spawn(watch)
    {
        tracing::warn!("a plugin's program is not ended when idle: {error}");
    }
}

impl Running {
    fn send(&mut self, message: &Message) -> anyhow::Result<()> {
        let stdin = self
            .stdin
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("the plugin is ending"))?;
        stdin
            .send_timeout(message.line(), ANSWER)
            .map_err(|_| Broken("the plugin does not read what it is asked".into()))?;
        Ok(())
    }

    /// Ask `method`, answering the program's own asks from `call`, until
    /// its answer, `patience` or the call given up.
    fn exchange(
        &mut self,
        name: &str,
        params: Value,
        call: &Call,
        patience: Duration,
    ) -> anyhow::Result<Value> {
        let id = self.next;
        self.next += 1;
        self.send(&Message::request(id, name, params))?;
        let deadline = Instant::now() + patience;
        loop {
            // Every turn, not only a quiet one: a program that keeps
            // writing (its log, its reads) is held to the deadline too.
            if self.stopped.load(Ordering::Relaxed) {
                anyhow::bail!("the plugin was turned off");
            }
            if call.cancelled() {
                let _ = self.send(&Message::notification(method::CANCEL, json!({ "id": id })));
                anyhow::bail!("cancelled");
            }
            if Instant::now() > deadline {
                return Err(Broken("the plugin did not answer in time".into()).into());
            }
            match self.messages.recv_timeout(Duration::from_millis(100)) {
                Ok(message) if message.is_response() => {
                    // An answer to a call given up before is no answer.
                    if message.id == Some(id) {
                        return message.outcome().map_err(anyhow::Error::msg);
                    }
                }
                Ok(message) => self.asked(message, call)?,
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    // Its output ended or said no message: whatever it
                    // does now is ended, and the next call starts it.
                    let _ = self.child.kill();
                    let _ = self.child.wait();
                    return Err(Broken("the plugin stopped".into()).into());
                }
            }
        }
    }

    /// Answer what the program asks while it answers: a read of a file of
    /// the call, a line for the log.
    fn asked(&mut self, message: Message, call: &Call) -> anyhow::Result<()> {
        match (message.id, message.method.as_deref()) {
            (Some(id), Some(method::READ)) => {
                let answer = message
                    .params
                    .ok_or_else(|| "no parameters".to_owned())
                    .and_then(|p| {
                        serde_json::from_value::<ReadParams>(p).map_err(|e| e.to_string())
                    })
                    .and_then(|p| call.read(p.file, p.offset, p.length))
                    .map(|bytes| {
                        json!(ReadAnswer {
                            data: bibli_plugin_api::base64::encode(&bytes),
                        })
                    });
                self.send(&Message::answer(id, answer))
            }
            (Some(id), Some(other)) => self.send(&Message::answer(
                id,
                Err(format!("bibli answers no `{other}`")),
            )),
            (None, Some(method::LOG)) => {
                if let Some(Ok(log)) = message.params.map(serde_json::from_value::<LogParams>) {
                    let line: String = log.message.chars().take(500).collect();
                    match log.level.as_str() {
                        "error" | "warn" => tracing::warn!("plugin: {line}"),
                        "info" => tracing::info!("plugin: {line}"),
                        _ => tracing::debug!("plugin: {line}"),
                    }
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// End the program: its input closed, it has a moment to end, then
    /// it is ended.
    fn end(mut self) {
        drop(self.stdin.take());
        let deadline = Instant::now() + Duration::from_secs(1);
        while Instant::now() < deadline {
            if matches!(self.child.try_wait(), Ok(Some(_))) {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Transport for Program {
    fn call(&self, name: &str, params: Value, call: &Call) -> anyhow::Result<Value> {
        // One call at a time: the others wait their turn, and one given up
        // meanwhile (a thumbnail scrolled away) leaves its worker at once
        // rather than when the program is free.
        let mut held = loop {
            if let Some(held) = self.running.try_lock_for(Duration::from_millis(50)) {
                break held;
            }
            if call.cancelled() {
                anyhow::bail!("cancelled");
            }
            if self.stopped.load(Ordering::Relaxed) {
                anyhow::bail!("the plugin was turned off");
            }
        };
        // Turned off while this call waited for the program: not started
        // again behind the user's back.
        if self.stopped.load(Ordering::Relaxed) {
            anyhow::bail!("the plugin was turned off");
        }
        if call.cancelled() {
            anyhow::bail!("cancelled");
        }
        if held.is_none() {
            if self.failed.lock().is_some_and(|at| at.elapsed() < AGAIN) {
                anyhow::bail!("the plugin could not start a moment ago");
            }
            match self.start() {
                Ok(started) => *held = Some(started),
                Err(error) => {
                    *self.failed.lock() = Some(Instant::now());
                    return Err(error);
                }
            }
        }
        let running = held.as_mut().expect("started");
        let answer = running.exchange(name, params, call, self.patience);
        let broken = answer.as_ref().is_err_and(anyhow::Error::is::<Broken>);
        if broken || matches!(running.child.try_wait(), Ok(Some(_))) {
            // It stopped, or stopped answering: ended, so that the next
            // call starts it afresh rather than wait on it again.
            if let Some(stopped) = held.take() {
                stopped.end();
            }
        } else {
            let _ = running.used.send(());
        }
        answer
    }

    fn stop(&self) {
        // First, so that a call under way gives up within a turn and lets
        // go of the program for this to end it.
        self.stopped.store(true, Ordering::Relaxed);
        if let Some(running) = self.running.lock().take() {
            running.end();
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    /// A program that says it is ready, then answers nothing ever.
    fn mute(folder: &Path) -> Program {
        let mut program = Program::new(
            "mute",
            PathBuf::from("/bin/sh"),
            vec![
                "-c".into(),
                r#"read l; echo '{"jsonrpc":"2.0","id":1,"result":{"api":1}}'; while read l; do :; done"#
                    .into(),
            ],
            folder.to_owned(),
            Capabilities::NONE,
            Isolation::WhereAvailable,
            None,
        );
        program.patience = Duration::from_millis(300);
        program
    }

    fn call() -> Call {
        Call::new(
            Vec::new(),
            Capabilities::NONE,
            bibli_core::CancelToken::new(),
        )
    }

    /// A program that stops answering is ended, to be started afresh by
    /// the next call; turned off, nothing starts it again.
    #[test]
    fn a_program_that_stops_answering_is_ended_and_one_turned_off_stays_off() {
        let folder = tempfile::tempdir().unwrap();
        let program = mute(folder.path());
        let error = program.call("field", json!({}), &call()).unwrap_err();
        assert!(error.is::<Broken>(), "{error:#}");
        assert!(
            program.running.lock().is_none(),
            "ended, not waited on again"
        );
        let error = program.call("field", json!({}), &call()).unwrap_err();
        assert!(
            error.is::<Broken>(),
            "started again, and broken again: {error:#}"
        );
        program.stop();
        let error = program.call("field", json!({}), &call()).unwrap_err();
        assert!(!error.is::<Broken>(), "off: {error:#}");
        assert!(program.running.lock().is_none(), "not started again");
    }
}
