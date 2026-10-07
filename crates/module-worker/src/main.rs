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
//! `bibli-module-worker <module>`; `--probe` answers whether it runs.

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
    // `--probe`, bibli's settings asking whether this helper works here:
    // being started at all says its libraries are there.
    if args.first().is_some_and(|arg| arg == "--probe") {
        return;
    }
    let Some(module) = args.first() else {
        eprintln!("usage: bibli-module-worker <module>");
        std::process::exit(2);
    };
    if let Err(error) = serve(std::path::Path::new(module)) {
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

/// Compile `path`, then answer bibli's calls until it closes standard input.
fn serve(path: &std::path::Path) -> anyhow::Result<()> {
    use anyhow::Context as _;
    let mut config = Config::new();
    config.consume_fuel(true);
    config.wasm_component_model(true);
    let engine = Engine::new(&config)?;
    let bytes = std::fs::read(path).with_context(|| format!("{}", path.display()))?;
    let bytes = if is_component(&bytes) {
        bytes
    } else {
        wit_component::ComponentEncoder::default()
            .module(&bytes)
            .and_then(|encoder| encoder.validate(true).encode())
            .context("a module built with the plugin world's bindings")?
    };
    let component = Component::new(&engine, &bytes).context("compiling the module")?;
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
                answer = Some(instantiate(&linker, &component, &mut store)?);
            }
            let call = answer.as_ref().expect("instantiated");
            store.set_fuel(FUEL)?;
            match call.call(&mut store, (name.clone(), params.to_string())) {
                Ok((reply,)) => {
                    call.post_return(&mut store)?;
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
    instance.get_typed_func(&mut *store, call)
}

/// Whether `bytes` are a component rather than a core module: the
/// binary's layer, after its magic and version.
fn is_component(bytes: &[u8]) -> bool {
    bytes.len() >= 8 && bytes[..4] == *b"\0asm" && bytes[6..8] == [1, 0]
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_component_is_told_from_a_core_module() {
        assert!(super::is_component(b"\0asm\x0d\x00\x01\x00"));
        assert!(!super::is_component(b"\0asm\x01\x00\x00\x00"));
        assert!(!super::is_component(b"\0as"));
    }
}
