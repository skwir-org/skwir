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
//! What parses (a text opened, a grammar checked) runs on a thread of its
//! own, one request at a time; what reads what is kept (colors, folds, an
//! outline) is answered at once, beside a parse that runs. Replies come as
//! each is ready, each tagged with its request's id.
//!
//! `skwir-tree-sitter-worker`; `--probe` answers whether it runs, `--build`
//! which build of skwir it is.

mod grammars;
mod text;

use std::collections::HashMap;
use std::io::{BufReader, Write};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

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
    // `--cache <folder>`: where compiled WebAssembly grammars are kept
    // (skwir's cache); without it, each is compiled at each start.
    let cache = match args.as_slice() {
        [] => None,
        [flag, folder] if flag == "--cache" => Some(std::path::PathBuf::from(folder)),
        _ => {
            eprintln!("usage: skwir-tree-sitter-worker [--cache <folder>]");
            std::process::exit(2);
        }
    };
    if let Err(error) = serve(cache) {
        // The manager reads replies only: what ended the helper is said as
        // one, then the helper exits, which the manager notices too.
        let reply = Reply::Error(format!("{error:#}"));
        let _ = send(0, &reply);
        std::process::exit(1);
    }
}

/// A parse asked and not answered yet: its id, what it asks, and the flag
/// a `Cancel` of it sets.
type Parse = (u64, Request, Arc<AtomicUsize>);

/// Greet, then answer requests from standard input until skwir closes it.
/// Each request gets one reply; a request that fails, or panics, is that
/// request's `Error` and the helper goes on. A line that is too long or
/// does not parse ends the helper: what followed it cannot be told apart.
fn serve(cache: Option<std::path::PathBuf>) -> anyhow::Result<()> {
    send(
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
    let kept = text::Kept::default();
    // The parses read and not answered, by id: a `Cancel` sets its flag,
    // whether it runs or waits.
    let flags: Arc<Mutex<HashMap<u64, Arc<AtomicUsize>>>> = Arc::default();
    let (parses, asked) = std::sync::mpsc::channel::<Parse>();
    let parsing = {
        let (kept, flags) = (kept.clone(), Arc::clone(&flags));
        std::thread::Builder::new()
            .name("parses".into())
            .spawn(move || {
                let mut worker = text::Worker::new(kept, cache);
                for (id, request, cancel) in asked {
                    let reply = if cancel.load(Ordering::SeqCst) == 0 {
                        let (reply, panicked) = answer(|| worker.handle(request, &cancel));
                        if panicked {
                            // A parser that may be in the middle of something.
                            worker.recover();
                        }
                        reply
                    } else {
                        Reply::Error("cancelled".into())
                    };
                    flags
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .remove(&id);
                    if send(id, &reply).is_err() {
                        break;
                    }
                }
            })?
    };
    let mut input = BufReader::new(std::io::stdin());
    while let Some(request) = skwir_core::jsonl::read::<Tagged<Request>>(&mut input, MAX_REQUEST) {
        let Tagged { id, body } = request?;
        match body {
            Request::Cancel { request } => {
                if let Some(flag) = flags
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .get(&request)
                {
                    flag.store(1, Ordering::SeqCst);
                }
                send(id, &Reply::Done)?;
            }
            read @ (Request::Colors { .. } | Request::Folds { .. } | Request::Outline { .. }) => {
                send(id, &answer(|| kept.read(read)).0)?;
            }
            parse => {
                let flag = Arc::new(AtomicUsize::new(0));
                flags
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .insert(id, Arc::clone(&flag));
                if parses.send((id, parse, flag)).is_err() {
                    anyhow::bail!("the parses' thread ended");
                }
            }
        }
    }
    // Standard input closed: skwir let go of the helper. A parse that
    // runs ends with the process.
    drop(parses);
    drop(parsing);
    Ok(())
}

/// The reply `handle` gives: its answer, or its `Error`, a panic's too;
/// and whether it panicked, which leaves what it used in a state not
/// known.
fn answer(handle: impl FnOnce() -> anyhow::Result<Reply>) -> (Reply, bool) {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(handle)) {
        Ok(Ok(reply)) => (reply, false),
        Ok(Err(error)) => (Reply::Error(format!("{error:#}")), false),
        Err(panic) => {
            let said = panic
                .downcast_ref::<&str>()
                .map(|said| (*said).to_owned())
                .or_else(|| panic.downcast_ref::<String>().cloned())
                .unwrap_or_default();
            (
                Reply::Error(format!("the tree-sitter helper failed: {said}")),
                true,
            )
        }
    }
}

/// One reply line, newline included, within `MAX_REPLY`, written whole:
/// standard output is locked while it is, so replies from two threads
/// never mix.
fn send(id: u64, reply: &Reply) -> anyhow::Result<()> {
    let mut line = skwir_core::jsonl::encode(&Tagged { id, body: reply })?;
    if line.len() > MAX_REPLY {
        line = skwir_core::jsonl::encode(&Tagged {
            id,
            body: Reply::Error("the answer exceeds the reply limit".into()),
        })?;
    }
    let mut output = std::io::stdout().lock();
    output.write_all(&line)?;
    output.flush()?;
    Ok(())
}
