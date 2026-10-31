//! Tree-sitter grammars run in this helper, never in skwir's own process
//! (ADR-110): a grammar is code (a shared library compiled from C, or a
//! WebAssembly module), and a crash or a hostile input to its scanner
//! takes this process, not the window. skwir starts it isolated where the
//! system can, the grammars' folders readable, and hands it the texts to
//! parse as files in its working folder (`skwir_core::syntax`).
//!
//! It is told every grammar found once, loads one when a text first needs
//! it (native through the system's loader, WebAssembly on Wasmtime),
//! colors each text whole with `tree-sitter-highlight`, injected languages
//! by the other grammars, and keeps per document only the spans of each
//! line, and its folds and outline when the grammar's queries say them:
//! never the text, nor its tree.
//!
//! `skwir-tree-sitter-worker`; `--probe` answers whether it runs, `--build`
//! which build of skwir it is.

mod grammars;
mod text;

use std::io::{BufReader, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use skwir_core::document::Tagged;
use skwir_core::syntax::{MAX_REPLY, MAX_REQUEST, Reply, Request};

fn main() {
    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    // `--build`, skwir asking which build of it this helper is, when a
    // call of it failed (`platform::process::explain`).
    if args.first().is_some_and(|arg| arg == "--build") {
        println!("{}", env!("SKWIR_BUILD"));
        return;
    }
    // `--probe`, skwir's settings asking whether this helper works here:
    // being started at all says its libraries are there; grammars are
    // opened only when a text needs them.
    if args.first().is_some_and(|arg| arg == "--probe") {
        return;
    }
    if !args.is_empty() {
        eprintln!("usage: skwir-tree-sitter-worker");
        std::process::exit(2);
    }
    if let Err(error) = serve() {
        // The manager reads replies only: what ended the helper is said as
        // one, then the helper exits, which the manager notices too.
        let reply = Reply::Error(format!("{error:#}"));
        let _ = send(&mut std::io::stdout().lock(), 0, &reply);
        std::process::exit(1);
    }
}

/// Greet, then answer requests from standard input until skwir closes it.
/// Each request gets one reply, in order; a request that fails, or panics,
/// is that request's `Error` and the helper goes on. A line that is too
/// long or does not parse ends the helper: what followed it cannot be
/// told apart.
fn serve() -> anyhow::Result<()> {
    // Cancels read but not answered yet: what runs now gives up while one
    // waits behind it. A count, not a flag, so that a `Cancel` answered
    // never clears one read after it.
    let cancels = Arc::new(AtomicUsize::new(0));
    let (lines, requests) = std::sync::mpsc::channel();
    let counted = Arc::clone(&cancels);
    // Requests are read ahead on a thread of their own, so a parse running
    // sees a `Cancel` that follows it.
    std::thread::Builder::new()
        .name("requests".into())
        .spawn(move || {
            let mut input = BufReader::new(std::io::stdin());
            while let Some(request) =
                skwir_core::jsonl::read::<Tagged<Request>>(&mut input, MAX_REQUEST)
            {
                if let Ok(Tagged {
                    body: Request::Cancel,
                    ..
                }) = &request
                {
                    counted.fetch_add(1, Ordering::SeqCst);
                }
                let failed = request.is_err();
                if lines.send(request).is_err() || failed {
                    break;
                }
            }
        })?;
    let mut output = std::io::stdout().lock();
    send(
        &mut output,
        0,
        &Reply::Ready {
            version: env!("SKWIR_TREE_SITTER_VERSION").to_owned(),
            abi: (
                u32::try_from(tree_sitter::MIN_COMPATIBLE_LANGUAGE_VERSION)?,
                u32::try_from(tree_sitter::LANGUAGE_VERSION)?,
            ),
            wasm: true,
        },
    )?;
    let mut worker = text::Worker::new(Arc::clone(&cancels));
    while let Ok(request) = requests.recv() {
        let Tagged { id, body } = request?;
        let reply = match body {
            Request::Cancel => {
                cancels.fetch_sub(1, Ordering::SeqCst);
                Reply::Error("cancelled".into())
            }
            request => answer(&mut worker, request),
        };
        send(&mut output, id, &reply)?;
    }
    Ok(())
}

/// The reply to `request`: its answer, or its `Error`, a panic's too. A
/// panic leaves the parser in a state not known: it is made again.
fn answer(worker: &mut text::Worker, request: Request) -> Reply {
    let handled = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| worker.handle(request)));
    match handled {
        Ok(Ok(reply)) => reply,
        Ok(Err(error)) => Reply::Error(format!("{error:#}")),
        Err(panic) => {
            worker.recover();
            let said = panic
                .downcast_ref::<&str>()
                .map(|said| (*said).to_owned())
                .or_else(|| panic.downcast_ref::<String>().cloned())
                .unwrap_or_default();
            Reply::Error(format!("the tree-sitter helper failed: {said}"))
        }
    }
}

/// One reply line, newline included, within `MAX_REPLY`.
fn send(output: &mut impl Write, id: u64, reply: &Reply) -> anyhow::Result<()> {
    let mut line = skwir_core::jsonl::encode(&Tagged { id, body: reply })?;
    if line.len() > MAX_REPLY {
        line = skwir_core::jsonl::encode(&Tagged {
            id,
            body: Reply::Error("the answer exceeds the reply limit".into()),
        })?;
    }
    output.write_all(&line)?;
    output.flush()?;
    Ok(())
}
