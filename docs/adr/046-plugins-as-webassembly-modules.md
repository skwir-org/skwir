# ADR-046: Plugins that run as WebAssembly modules

Status: accepted, 2026-10-07 (Phase 6, "Level 1, WASM plugins").

## Context

The plan's Level 1 is "wasmtime and the component model for providers,
commands, metadata, not views", and its gate is "one existing internal
plugin re-implemented as WASM with no user-visible difference". A
module can be held to less than a program can: it reaches nothing that
its imports do not give it, on every system, without a sandbox.

## Decision

1. **The plugin world** (`crates/plugin-api/wit/plugin.wit`,
   `bibli:plugin@0.1.0`): bibli imports `read` (a file of the call, by
   handle) and `log`; the plugin exports one `call(method, params)` whose
   methods and JSON are the protocol's (ADR-045). One vocabulary, two
   transports: the same plugin builds as a module or as a program.
2. **No WASI.** A module is built for `wasm32-unknown-unknown` with the
   world's bindings (`wit-bindgen`, through `bibli_plugin_api::export!`)
   and made a component when it is loaded (`wit-component`); a component
   that imports anything but the world's host is refused at
   instantiation. It has no files, no clock, no randomness, no network
   (a manifest that declares `network` with a module is refused), no
   environment.
3. **A helper of bibli's runs it**, `bibli-module-worker`, with
   **Wasmtime 38** (the newest that builds with bibli's minimum Rust,
   1.88): bibli starts the helper as it starts a plugin that is a
   program (ADR-045), isolated where the system can (and plainly where
   it cannot: the module is confined by Wasmtime all the same), and
   speaks the protocol with it; the module's imports become the
   protocol's asks. Wasmtime and Cranelift never enter bibli's own
   process: linked into the binary, they cost 1.6 MiB of idle memory
   (`just idle-pss` back to back, own share 94.8 against 93.2 MiB, the
   binary's 41.0 against 39.4) for code that runs only when a module is
   asked. Cranelift compiles the module when
   the helper starts. A call has bounded work (fuel, about twenty
   billion instructions) and memory (256 MiB); a module that traps is
   instantiated again for its next call. Calls are one at a time.
4. **The gate's plugin** is the hexadecimal preview
   (`crates/plugins-outside/preview-hex`, id `hex-view`): its own code,
   its bytes read through bibli. A test (`crates/extend/tests/modules.rs`)
   asks the built-in preview and the module the same pages, searches and
   wrong searches of synthetic files (a match across the 64 KiB chunks
   a search reads, an empty file, a name with an apostrophe and Unicode)
   and finds every page the same, field for field; with the built-in one
   off, the module is the view every file offers.
5. **Building.** `just plugins` builds the workspace's modules into
   `target/plugins`, a folder of installed plugins
   (`BIBLI_PLUGIN_PATH=$PWD/target/plugins`); the dev shell's toolchain
   has the target. The helper ships with bibli in every package, as the
   other helpers do (`tools/package/programs`), and answers `--probe`.

## Consequences

- The packages do not carry the modules: the built-in hexadecimal
  preview stays what they ship, and the module is the gate's proof and
  an example. A package that carries modules builds them with the
  toolchain's WebAssembly target.
- A module is isolated twice: by Wasmtime, and with its helper, by the
  system's sandbox where there is one.
- A module is compiled each time its helper starts, which is when it is
  first asked after 30 seconds unasked: a fraction of a second, which a
  cache of compiled modules would save.
- The component model's typed interfaces are used for the envelope
  only; the payloads are JSON, the protocol's, so a program and a module
  never drift apart. A typed world is a later version of the API.
- Views stay native (`PaneView`, ADR-007's reasons): a module provides
  data, which bibli draws.
