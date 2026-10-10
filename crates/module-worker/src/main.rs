//! A plugin's WebAssembly module, run in a process of its own (ADR-046):
//! bibli starts this helper as it starts a plugin that is a program
//! (ADR-045), isolated where it can be, and speaks the same protocol on
//! its standard input and output. The helper compiles the module (a core
//! module built with the plugin world's bindings is made a component
//! first), gives it bibli's two imports and nothing else (no files, no
//! clock, no network, nothing of the system's), and answers each call by
//! the module's `call` export; the module's `read` and `log` become the
//! protocol's, asked of bibli on the same pipes. A call has bounded work
//! ([`FUEL`]) and memory ([`MEMORY`]); a module that traps is instantiated
//! again for the next. Wasmtime and Cranelift live here, never in
//! bibli's own process.
//!
//! The module is compiled when the helper starts. With `--keep <folder>`
//! (the plugin's folder of bibli's cache, its working folder), it keeps
//! the module compiled there and loads it on the next start instead
//! ([`load`]): starting takes a few milliseconds where compiling took a
//! good part of a second.
//!
//! `bibli-module-worker <module> [--keep <folder>]`; `--probe` answers
//! whether it runs.

use std::io::{BufRead, BufReader, Write};

use bibli_plugin_api::protocol::{
    InitializeAnswer, LogParams, MESSAGE_LIMIT, Message, ReadAnswer, ReadParams, VERSION, method,
};
use serde_json::json;
use wasmtime::component::{Component, Linker, TypedFunc};
use wasmtime::{Config, Engine, Store, StoreContextMut, StoreLimits, StoreLimitsBuilder};

/// The work one call may do, in Wasmtime's fuel (about an instruction
/// each): seconds of work, never a loop without end.
const FUEL: u64 = 20_000_000_000;
/// The memory a module may grow to.
const MEMORY: usize = 256 * 1024 * 1024;

/// The interfaces of the plugin world (`crates/plugin-api/wit`).
const HOST: &str = "bibli:plugin/host@0.1.0";
const GUEST: &str = "bibli:plugin/guest@0.1.0";

fn main() {
    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    // `--build`, bibli asking which build of it this helper is, when a
    // call of it failed (`platform::process::explain`).
    if args.first().is_some_and(|arg| arg == "--build") {
        println!("{}", env!("BIBLI_BUILD"));
        return;
    }
    // `--probe`, bibli's settings asking whether this helper works here:
    // being started at all says its libraries are there.
    if args.first().is_some_and(|arg| arg == "--probe") {
        return;
    }
    let (Some(module), keep) = (args.first(), args.get(1..)) else {
        eprintln!("usage: bibli-module-worker <module> [--keep <folder>]");
        std::process::exit(2);
    };
    let keep = match keep {
        Some([]) | None => None,
        Some([flag, folder]) if flag == "--keep" => Some(std::path::Path::new(folder)),
        Some(_) => {
            eprintln!("usage: bibli-module-worker <module> [--keep <folder>]");
            std::process::exit(2);
        }
    };
    if let Err(error) = serve(std::path::Path::new(module), keep) {
        eprintln!("{error:#}");
        std::process::exit(1);
    }
}

/// The pipes to bibli: messages read from standard input, written to
/// standard output, one per line.
struct Wire {
    input: BufReader<std::io::Stdin>,
    next: u64,
    /// bibli stopped waiting for the call being answered.
    cancelled: bool,
}

impl Wire {
    fn next_message(&mut self) -> anyhow::Result<Option<Message>> {
        let mut line = Vec::new();
        let read = std::io::Read::take(&mut self.input, MESSAGE_LIMIT as u64)
            .read_until(b'\n', &mut line)?;
        if read == 0 {
            return Ok(None);
        }
        Ok(Some(serde_json::from_slice(&line)?))
    }

    fn send(message: &Message) -> std::io::Result<()> {
        let mut out = std::io::stdout().lock();
        out.write_all(&message.line())?;
        out.flush()
    }

    /// Ask bibli for bytes of a file of the call, as the module asks.
    fn read(&mut self, file: u32, offset: u64, length: u32) -> Result<Vec<u8>, String> {
        if self.cancelled {
            return Err("cancelled".into());
        }
        let id = self.next;
        self.next += 1;
        let params = ReadParams {
            file,
            offset,
            length,
        };
        Wire::send(&Message::request(id, method::READ, json!(params)))
            .map_err(|e| e.to_string())?;
        loop {
            let message = self
                .next_message()
                .map_err(|e| e.to_string())?
                .ok_or("bibli closed the channel")?;
            if message.is_response() && message.id == Some(id) {
                let answer: ReadAnswer =
                    serde_json::from_value(message.outcome()?).map_err(|e| e.to_string())?;
                return bibli_plugin_api::base64::decode(&answer.data)
                    .ok_or_else(|| "bytes that are no base64".to_owned());
            }
            if message.method.as_deref() == Some(method::CANCEL) {
                self.cancelled = true;
                return Err("cancelled".into());
            }
        }
    }
}

/// What a module's imports reach.
struct State {
    wire: Wire,
    limits: StoreLimits,
}

type Answer = TypedFunc<(String, String), (Result<String, String>,)>;

/// Compile `path` (or load it compiled from `keep`), then answer bibli's
/// calls until it closes standard input.
fn serve(path: &std::path::Path, keep: Option<&std::path::Path>) -> anyhow::Result<()> {
    let engine = engine()?;
    let component = load(&engine, path, keep)?;
    let mut linker: Linker<State> = Linker::new(&engine);
    let mut host = linker.instance(HOST)?;
    host.func_wrap(
        "read",
        |mut store: StoreContextMut<'_, State>, (file, offset, length): (u32, u64, u32)| {
            Ok((store.data_mut().wire.read(file, offset, length),))
        },
    )?;
    host.func_wrap(
        "log",
        |_: StoreContextMut<'_, State>, (level, message): (String, String)| {
            let params = LogParams { level, message };
            let _ = Wire::send(&Message::notification(method::LOG, json!(params)));
            Ok(())
        },
    )?;
    let mut store = Store::new(
        &engine,
        State {
            wire: Wire {
                input: BufReader::new(std::io::stdin()),
                next: 1,
                cancelled: false,
            },
            limits: limits(),
        },
    );
    store.limiter(|state| &mut state.limits);
    let mut answer = None;
    while let Some(message) = store.data_mut().wire.next_message()? {
        let Some(id) = message.id else {
            continue; // A notification between calls: a late cancel.
        };
        let name = message.method.unwrap_or_default();
        let params = message.params.unwrap_or(serde_json::Value::Null);
        store.data_mut().wire.cancelled = false;
        let reply = if name == method::INITIALIZE {
            Ok(json!(InitializeAnswer { api: VERSION }))
        } else {
            if answer.is_none() {
                // A store that held a trapped instance is not reused.
                store = Store::new(
                    &engine,
                    State {
                        wire: std::mem::replace(
                            &mut store.data_mut().wire,
                            Wire {
                                input: BufReader::new(std::io::stdin()),
                                next: 1,
                                cancelled: false,
                            },
                        ),
                        limits: limits(),
                    },
                );
                store.limiter(|state| &mut state.limits);
                // Instantiating runs the module's own code too, and takes
                // fuel (Wasmtime 49 counts it; 38 did not): a budget of its
                // own, the call's given after.
                store.set_fuel(FUEL)?;
                answer = Some(instantiate(&linker, &component, &mut store)?);
            }
            let call = answer.as_ref().expect("instantiated");
            store.set_fuel(FUEL)?;
            match call.call(&mut store, (name.clone(), params.to_string())) {
                Ok((reply,)) => {
                    reply.and_then(|text| serde_json::from_str(&text).map_err(|e| e.to_string()))
                }
                Err(trap) => {
                    answer = None;
                    Err(format!("the plugin's module stopped: {trap:#}"))
                }
            }
        };
        Wire::send(&Message::answer(id, reply))?;
    }
    Ok(())
}

fn limits() -> StoreLimits {
    StoreLimitsBuilder::new()
        .memory_size(MEMORY)
        .instances(16)
        .tables(16)
        .memories(4)
        .build()
}

fn instantiate(
    linker: &Linker<State>,
    component: &Component,
    store: &mut Store<State>,
) -> anyhow::Result<Answer> {
    let instance = linker.instantiate(&mut *store, component)?;
    let guest = instance
        .get_export_index(&mut *store, None, GUEST)
        .ok_or_else(|| anyhow::anyhow!("the module exports no `{GUEST}`"))?;
    let call = instance
        .get_export_index(&mut *store, Some(&guest), "call")
        .ok_or_else(|| anyhow::anyhow!("the module exports no `call`"))?;
    Ok(instance.get_typed_func(&mut *store, call)?)
}

/// Whether `bytes` are a component rather than a core module: the
/// binary's layer, after its magic and version.
/// The engine every module runs in: fuel counted, components.
fn engine() -> anyhow::Result<Engine> {
    let mut config = Config::new();
    config.consume_fuel(true);
    config.wasm_component_model(true);
    Ok(Engine::new(&config)?)
}

/// The module at `path` as a component compiled for `engine`: loaded from
/// `keep` when it holds this very component compiled by this Wasmtime with
/// these settings, else compiled, and kept there for the next start.
fn load(
    engine: &Engine,
    path: &std::path::Path,
    keep: Option<&std::path::Path>,
) -> anyhow::Result<Component> {
    use anyhow::Context as _;
    let file = std::fs::read(path).with_context(|| format!("{}", path.display()))?;
    let component = is_component(&file);
    // A core module made a component, validated: only to compile it, never
    // to name what was kept of it, which a start loads as it was.
    let encoded = || -> anyhow::Result<Vec<u8>> {
        if component {
            return Ok(file.clone());
        }
        wit_component::ComponentEncoder::default()
            .module(&file)
            .and_then(|encoder| encoder.validate(true).encode())
            .context("a module built with the plugin world's bindings")
    };
    let Some(keep) = keep else {
        return Component::new(engine, &encoded()?)
            .map_err(anyhow::Error::from)
            .context("compiling the module");
    };
    let kept = keep.join(format!("{}.cwasm", compiled_name(engine, &file, component)));
    if kept.is_file() {
        // SAFETY: the file is native code Wasmtime runs as it finds it, so
        // it must be what `Component::serialize` wrote for this component.
        // It is: its name is the hash of the component's bytes and of the
        // engine's version and settings, it lies in the plugin's own folder
        // of the user's cache, which no module and no other plugin can
        // write (this helper is the only writer, `keep` below), and it is
        // written whole under another name before it takes this one, so it
        // never changes while mapped. Wasmtime refuses a file compiled by
        // another version or for other settings.
        #[allow(unsafe_code)]
        match unsafe { Component::deserialize_file(engine, &kept) } {
            Ok(component) => return Ok(component),
            Err(error) => eprintln!("compiled module not loaded, compiling it again: {error:#}"),
        }
    }
    let component = Component::new(engine, &encoded()?)
        .map_err(anyhow::Error::from)
        .context("compiling the module")?;
    if let Err(error) = save(&component, keep, &kept) {
        eprintln!("compiled module not kept: {error:#}");
    }
    Ok(component)
}

/// The name a compiled component is kept under: what it was compiled from
/// (the file as it is, and whether it was a component already) and by
/// what, so another module, another Wasmtime or other settings is another
/// name.
fn compiled_name(engine: &Engine, file: &[u8], component: bool) -> String {
    use std::hash::Hash as _;
    /// Feeds what Wasmtime hashes into BLAKE3.
    struct Blake3(blake3::Hasher);
    impl std::hash::Hasher for Blake3 {
        fn write(&mut self, bytes: &[u8]) {
            self.0.update(bytes);
        }
        fn finish(&self) -> u64 {
            0
        }
    }
    let mut hasher = Blake3(blake3::Hasher::new());
    hasher.0.update(&[u8::from(component)]);
    hasher.0.update(file);
    engine.precompile_compatibility_hash().hash(&mut hasher);
    hasher.0.finalize().to_hex().to_string()
}

/// Keep `component` compiled at `kept`, in `keep`, written under another
/// name first; everything else there goes: the module it replaces (an
/// older version of the plugin's, or one compiled by another Wasmtime),
/// and what a helper stopped while writing left.
fn save(
    component: &Component,
    keep: &std::path::Path,
    kept: &std::path::Path,
) -> anyhow::Result<()> {
    std::fs::create_dir_all(keep)?;
    let compiled = component.serialize()?;
    let writing = keep.join(format!(".writing-{}", std::process::id()));
    std::fs::write(&writing, compiled)?;
    std::fs::rename(&writing, kept)?;
    for old in std::fs::read_dir(keep)?.flatten() {
        if old.path() != kept {
            let _ = std::fs::remove_file(old.path());
        }
    }
    Ok(())
}

fn is_component(bytes: &[u8]) -> bool {
    bytes.len() >= 8 && bytes[..4] == *b"\0asm" && bytes[6..8] == [1, 0]
}

#[cfg(test)]
mod tests {
    /// The files `keep` holds.
    fn kept(keep: &std::path::Path) -> Vec<std::path::PathBuf> {
        std::fs::read_dir(keep)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect()
    }

    #[test]
    fn a_module_compiled_once_is_loaded_the_next_time() {
        let tmp = tempfile::tempdir().unwrap();
        let module = tmp.path().join("plugin.wasm");
        // The smallest component there is: its header.
        std::fs::write(&module, b"\0asm\x0d\x00\x01\x00").unwrap();
        let keep = tmp.path().join("kept");
        let engine = super::engine().unwrap();
        super::load(&engine, &module, Some(&keep)).unwrap();
        let first = kept(&keep);
        assert_eq!(first.len(), 1, "{first:?}");
        assert!(first[0].extension().is_some_and(|e| e == "cwasm"));
        let written = std::fs::metadata(&first[0]).unwrap().modified().unwrap();
        // Started again: loaded, not compiled and written again.
        super::load(&engine, &module, Some(&keep)).unwrap();
        assert_eq!(kept(&keep), first);
        assert_eq!(
            std::fs::metadata(&first[0]).unwrap().modified().unwrap(),
            written
        );
        // A file that is not compiled code is compiled again, never run.
        std::fs::write(&first[0], b"not compiled code").unwrap();
        super::load(&engine, &module, Some(&keep)).unwrap();
        assert!(std::fs::metadata(&first[0]).unwrap().len() > 100);
        // The plugin's module changed: its compiled form replaces the
        // old one, and what a stopped write left goes too.
        std::fs::write(keep.join(".writing-1"), b"half").unwrap();
        let mut changed = b"\0asm\x0d\x00\x01\x00".to_vec();
        changed.extend([0, 5, 4, b'n', b'o', b't', b'e']); // a custom section
        std::fs::write(&module, changed).unwrap();
        super::load(&engine, &module, Some(&keep)).unwrap();
        let second = kept(&keep);
        assert_eq!(second.len(), 1, "{second:?}");
        assert_ne!(second, first);
    }

    #[test]
    fn a_component_is_told_from_a_core_module() {
        assert!(super::is_component(b"\0asm\x0d\x00\x01\x00"));
        assert!(!super::is_component(b"\0asm\x01\x00\x00\x00"));
        assert!(!super::is_component(b"\0as"));
    }
}
