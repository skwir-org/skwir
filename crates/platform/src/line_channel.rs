//! The one way a session manager talks to its worker process: JSON lines
//! both ways, one request per line written in one buffer, replies read by a
//! thread of their own into a bounded channel the manager selects on.
//!
//! The manager thread owns the channel and with it the child: dropping it
//! kills the worker's process group and reaps it (`ChildGuard`), so nothing
//! here ever waits on the UI thread. The worker's standard error goes
//! nowhere: what it has to say travels as replies.

use std::io::{BufReader, Write};
use std::process::{ChildStdin, Command, Stdio};

use anyhow::Context;
use crossbeam_channel::Receiver;
use serde::{Serialize, de::DeserializeOwned};

use crate::process::ChildGuard;

pub(crate) struct LineChannel<Reply> {
    child: ChildGuard,
    input: ChildStdin,
    replies: Receiver<anyhow::Result<Reply>>,
}

impl<Reply: DeserializeOwned + Send + 'static> LineChannel<Reply> {
    /// Start `command` in a process group of its own. A reply line longer
    /// than `max_reply` bytes, its newline included, ends the channel with an
    /// error; so does a line that does not parse. The end of the worker's
    /// output disconnects `replies`.
    pub(crate) fn spawn(
        command: &mut Command,
        max_reply: usize,
        name: &str,
    ) -> anyhow::Result<Self> {
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        let mut child = ChildGuard::new(command.spawn().with_context(|| format!("start {name}"))?);
        let input = child.child.stdin.take().context("worker stdin")?;
        #[cfg(unix)]
        {
            // A wedged worker must not trap its manager in a full request
            // pipe: cancellation still needs to reach ChildGuard and reap it.
            let flags = rustix::fs::fcntl_getfl(&input)?;
            rustix::fs::fcntl_setfl(&input, flags | rustix::fs::OFlags::NONBLOCK)?;
        }
        let output = child.child.stdout.take().context("worker stdout")?;
        let (tx, replies) = crossbeam_channel::bounded(2);
        std::thread::Builder::new()
            .name(format!("{name}-replies"))
            .spawn(move || {
                let mut reader = BufReader::new(output);
                while let Some(result) = skwir_core::jsonl::read(&mut reader, max_reply) {
                    let failed = result.is_err();
                    if tx.send(result).is_err() || failed {
                        break;
                    }
                }
            })?;
        Ok(Self {
            child,
            input,
            replies,
        })
    }

    /// Write one request as one line in a single buffer, so a request is
    /// never left half written between two calls.
    pub(crate) fn send(&mut self, request: &impl Serialize) -> anyhow::Result<()> {
        let line = skwir_core::jsonl::encode(request)?;
        self.input
            .write_all(&line)
            .context("worker is not reading its requests")
    }

    pub(crate) fn replies(&self) -> &Receiver<anyhow::Result<Reply>> {
        &self.replies
    }

    /// Whether the worker ended with 127, the dynamic loader's word for a
    /// shared library it could not find: its runtime is missing, which a
    /// caller takes as "not installed" (an engine chosen automatically goes
    /// on to the next) rather than as the file's fault. Asked once the
    /// replies ended before a greeting; the worker that closed its output
    /// is given a moment to exit.
    pub(crate) fn missing_runtime(&mut self) -> bool {
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(200);
        loop {
            match self.child.child.try_wait() {
                Ok(Some(status)) => return status.code() == Some(127),
                Ok(None) if std::time::Instant::now() < deadline => {
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                _ => return false,
            }
        }
    }
}
