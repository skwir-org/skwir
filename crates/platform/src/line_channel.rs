//! The one way a session manager talks to its worker process: JSON lines
//! both ways, one request per line written in one buffer, replies read by a
//! thread of their own into a bounded channel the manager selects on.
//!
//! The manager thread owns the channel and with it the child: dropping it
//! kills the worker's process group and reaps it (`ChildGuard`), so nothing
//! here ever waits on the UI thread. The worker's standard error goes
//! nowhere: what it has to say travels as replies.

use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStdin, Command, Stdio};

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
        let mut child = ChildGuard(command.spawn().with_context(|| format!("start {name}"))?);
        let input = child.0.stdin.take().context("worker stdin")?;
        #[cfg(unix)]
        {
            // A wedged worker must not trap its manager in a full request
            // pipe: cancellation still needs to reach ChildGuard and reap it.
            let flags = rustix::fs::fcntl_getfl(&input)?;
            rustix::fs::fcntl_setfl(&input, flags | rustix::fs::OFlags::NONBLOCK)?;
        }
        let output = child.0.stdout.take().context("worker stdout")?;
        let (tx, replies) = crossbeam_channel::bounded(2);
        std::thread::Builder::new()
            .name(format!("{name}-replies"))
            .spawn(move || {
                let mut reader = BufReader::new(output);
                loop {
                    let mut bytes = Vec::new();
                    let read = Read::by_ref(&mut reader)
                        .take((max_reply + 1) as u64)
                        .read_until(b'\n', &mut bytes);
                    if matches!(read, Ok(0)) {
                        break;
                    }
                    let result = (|| {
                        read?;
                        anyhow::ensure!(
                            bytes.len() <= max_reply && bytes.last() == Some(&b'\n'),
                            "worker reply exceeds limit"
                        );
                        Ok(serde_json::from_slice(&bytes)?)
                    })();
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
        let mut line = serde_json::to_vec(request)?;
        line.push(b'\n');
        self.input
            .write_all(&line)
            .context("worker is not reading its requests")
    }

    pub(crate) fn replies(&self) -> &Receiver<anyhow::Result<Reply>> {
        &self.replies
    }

    pub(crate) fn child(&mut self) -> &mut Child {
        &mut self.child.0
    }
}
