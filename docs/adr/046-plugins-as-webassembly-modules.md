# ADR-046: Plugins that run as WebAssembly modules

Status: accepted, 2026-10-07 (Phase 6, "Level 1, WASM plugins");
compiled modules cached the same day; Wasmtime 49 since 2026-10-08.
Built.

## Context

The plan's Level 1: "wasmtime and the component model for providers,
commands, metadata, not views". Gate: "one existing internal plugin
re-implemented as WASM with no user-visible difference". A module
reaches only what its imports give, on every system, without a sandbox.

## Decision

1. **The plugin world** (`crates/plugin-api/wit/plugin.wit`,
   `skwir:plugin@0.1.0`): Skwir provides `read` (a file of the call, by
   handle) and `log`; the plugin exports `call(method, params)` with the
   protocol's methods and JSON (ADR-045). The same plugin builds as a
   module or a program.
2. **No WASI.** Modules target `wasm32-unknown-unknown` with the world's
   bindings (`wit-bindgen` via `skwir_plugin_api::export!`) and become
   components on load (`wit-component`). Any other import is refused at
   instantiation: no files, clock, randomness, network or environment.
   A module manifest declaring `network` is refused.
3. **The helper `skwir-module-worker` runs it**, started like a program
   plugin (ADR-045), sandboxed where the system can (Wasmtime confines
   the module regardless); module imports become protocol asks.
   - **Wasmtime and Cranelift stay out of Skwir's process:** linked in,
     they cost 1.6 MiB idle for code used only when a module is asked
     (`just idle-pss` back to back, Wasmtime 38: own share 94.8 vs
     93.2 MiB, binary 41.0 vs 39.4 MiB).
   - **Compiled once, cached.** Cranelift compiles on the helper's first
     start into the plugin's cache folder (`<cache>/<id>/module`, its
     working folder and only writable one), named by the hash of the
     component and Wasmtime's version and settings, written then
     renamed, and mapped (not recompiled) later; anything else there is
     removed. Compiled code is trusted as native, so only that plugin's
     helper writes there.
   - **Bounded calls:** about twenty billion instructions of fuel and
     256 MiB of memory; one call at a time; a trapped module is
     re-instantiated for its next call.
4. **Gate plugin: the hex preview** (`crates/plugins-outside/hex-view`,
   id `hex-view`), its bytes read through Skwir.
   `crates/extend/tests/modules.rs` asks it and the built-in the same
   pages, searches and failed searches on synthetic files (a match
   across 64 KiB search chunks, an empty file, a name with an apostrophe
   and Unicode): every page is identical, field by field. With the
   built-in off, the module serves every file.
5. **Building:** `just plugins` builds the modules into `target/plugins`
   (`SKWIR_PLUGIN_PATH=$PWD/target/plugins`); the dev shell has the
   target. The helper ships in every package (`tools/package/programs`)
   and answers `--probe`.

Rejected: Wasmtime's own cache (its own folder, settings and cleaning
thread; Skwir has one cache).

## Consequences

- Packages carry no modules: they ship the built-in hex preview; the
  module is the gate's proof and an example.
- Modules are isolated twice: Wasmtime, and the system sandbox around
  the helper where there is one.
- One compilation per module version and Wasmtime version; the helper,
  ended after 30 s unasked, restarts in milliseconds. Deleting the cache
  costs one compilation.
- Typed component interfaces carry only the envelope; payloads are the
  protocol's JSON, so programs and modules cannot drift. A typed world
  is a later API version.
- Views stay native (`PaneView`, ADR-007): a module provides data.

## Amendments

### 2026-10-08: Wasmtime 49

Wasmtime 38 was the newest building with the minimum Rust then (1.88).
`just deny` found seventeen advisories against 38.0.4, among them an
out-of-bounds write in component string transcoding, a guest heap
access miscompiled into a sandbox escape on aarch64, and loads outside
the sandbox on x86-64. Now 49.0.2, raising the minimum Rust to 1.96.
(Amended 2026-10-28: Wasmtime 50.0.0, the minimum Rust 1.97.)
Instantiating now takes fuel, so each new store gets its own budget
before instantiation, then the call's.
