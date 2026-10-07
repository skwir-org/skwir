//! A plugin that runs as a program (Level 2, ADR-045): started isolated
//! when first asked (`sandbox::plugin`), the protocol on its pipes, one
//! call at a time, its `read`s answered from the call's files; ended once
//! idle for [`IDLE`], when it is turned off, and with bibli (`bwrap`'s
//! `--die-with-parent`). A program that stops is started again at the
//! next call, not before [`AGAIN`] after one that could not start.

use std::io::{BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Stdio};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use bibli_core::permissions::{Capabilities, Capability};
use bibli_plugin_api::manifest::Capability as Granted;
use bibli_plugin_api::protocol::{
    InitializeAnswer, InitializeParams, LogParams, MESSAGE_LIMIT, Message, ReadAnswer, ReadParams,
    VERSION, method,
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
    running: Arc<Mutex<Option<Running>>>,
    failed: Mutex<Option<Instant>>,
}

/// A program started, its pipes and its working folder.
struct Running {
    child: Child,
    stdin: Option<ChildStdin>,
    messages: Receiver<Message>,
    next: u64,
    /// Each call says it was asked here, for the watch that ends it idle.
    used: Sender<()>,
    _work: tempfile::TempDir,
}

impl Program {
    pub fn new(
        id: &'static str,
        program: PathBuf,
        args: Vec<String>,
        folder: PathBuf,
        granted: Capabilities,
        isolation: Isolation,
    ) -> Program {
        Program {
            id,
            path: program,
            args,
            folder,
            granted,
            isolation,
            running: Arc::default(),
            failed: Mutex::new(None),
        }
    }

    fn start(&self) -> anyhow::Result<Running> {
        let work = tempfile::Builder::new().prefix("bibli-plugin-").tempdir()?;
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
        let (tx, messages) = crossbeam_channel::unbounded();
        let id = self.id;
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
                    use std::io::BufRead;
                    for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                        let line: String = line.chars().take(500).collect();
                        tracing::debug!(plugin = id, "{line}");
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
        if answer.api != VERSION {
            running.end();
            anyhow::bail!(
                "the plugin speaks version {} of the protocol; bibli speaks {VERSION}",
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
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("the plugin is ending"))?;
        stdin.write_all(&message.line())?;
        stdin.flush()?;
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
            match self.messages.recv_timeout(Duration::from_millis(100)) {
                Ok(message) if message.is_response() => {
                    // An answer to a call given up before is no answer.
                    if message.id == Some(id) {
                        return message.outcome().map_err(anyhow::Error::msg);
                    }
                }
                Ok(message) => self.asked(message, call)?,
                Err(RecvTimeoutError::Timeout) => {
                    if call.cancelled() || Instant::now() > deadline {
                        let _ =
                            self.send(&Message::notification(method::CANCEL, json!({ "id": id })));
                        anyhow::bail!(if call.cancelled() {
                            "cancelled"
                        } else {
                            "the plugin did not answer in time"
                        });
                    }
                }
                Err(RecvTimeoutError::Disconnected) => {
                    // Its output ended or said no message: whatever it
                    // does now is ended, and the next call starts it.
                    let _ = self.child.kill();
                    let _ = self.child.wait();
                    anyhow::bail!("the plugin stopped")
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
        let mut held = self.running.lock();
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
        let answer = running.exchange(name, params, call, ANSWER);
        if matches!(running.child.try_wait(), Ok(Some(_))) {
            // It stopped: the next call starts it again.
            if let Some(stopped) = held.take() {
                stopped.end();
            }
        } else {
            let _ = running.used.send(());
        }
        answer
    }

    fn stop(&self) {
        if let Some(running) = self.running.lock().take() {
            running.end();
        }
    }
}
