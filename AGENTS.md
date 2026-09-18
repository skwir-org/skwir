# Working on bibli

bibli is an experimental file manager: Rust, GPU-accelerated UI (gpui), a tiny
core, everything else a plugin, designed from the first commit to be driven by
humans, scripts, and AI agents alike. This file is the context every agent
(human or model) needs before touching the code. Read it fully once.

Start here: `docs/PLAN.md` (phases, gates, current status and recorded
deviations), `docs/adr/` (decisions that closed a door; ADR-000 is the
principles and budgets), then the crate you are working in.

## Principles (ADR-000; binding)

1. **Always responsive.** No blocking work on the UI thread, ever. Listings
   stream; every provider call is async, batched, and cancellable; visible work
   has priority. The UI sleeps until the core wakes it (`App::set_waker`); never
   run a `request_animation_frame` loop.
2. **Light.** Idle memory and startup are budgeted (ADR-000, revised in ADR-002:
   PSS, not RSS). `just bench-check` must stay green. Budgets are tightened when
   the reference improves and never loosened without an ADR.
3. **Stateless and idempotent.** No daemon, no database. One deletable cache
   (`bibli_core::cache`). Re-applying an applied plan is a no-op.
4. **Harness the environment.** Reuse OS thumbnails, icon themes, the MIME
   database, bookmarks, zoxide, recently-used, installed tools (`ffmpeg`,
   `plocate`, `$EDITOR`, `$TERMINAL`) before building our own.
5. **Small core.** Only what two plugins must agree on lives in `bibli-core`.
   Everything else is a plugin with a runtime toggle.
6. **Agent-ready.** Commands are serializable data with JSON schemas. Mutations
   are plans reviewed before apply. Entries have stable identity (device +
   inode). Metadata carries provenance. AI output is a suggestion until a human
   confirms it.

## Engineering rules

- **Fix structurally, not with band-aids.** When a symptom appears a second
  time, or a fix needs hand-maintained constants to stay correct, an invariant
  is being violated. Find it, make the violation impossible, document the rule
  where the next author will read it, and remove the workarounds. Example: the
  grid reflowed a frame late after every panel toggle; the fix was moving
  size-dependent layout into the element's prepaint (`crates/ui/src/view.rs`),
  not patching each toggle.
- **UI layout rule.** Never derive layout from a measured size in `render`.
  Size-dependent layout lives in an element's `prepaint`, where the bounds are
  real (`crates/ui/src/elements/`). The only permitted later reads of prepaint
  values are for keyboard paging.
- **Commands are the one surface.** Every user action is a command
  (`bibli_core::command`), so the keymap, palette, CLI, MCP server, and agents
  all share it. The UI handles `ui.*` names itself and dispatches the rest.
  Commands that mutate the filesystem return a `Plan`; the caller decides to
  apply it. User-initiated single actions apply immediately and are undoable;
  batch results and anything from a background task or an agent go through
  `App::propose` for review.
- **Operations have inverses**, a dry-run `check`, and idempotent semantics.
  Reference entries by `EntryRef` (id + path hint), never by path alone.
- **Plugins depend only on `bibli-core` (and `bibli-platform`), never on each
  other.** They talk through core-owned data (entries, metadata fields,
  selection, locations). Every contribution is tagged with the plugin id and
  withdrawn on disable; `crates/bibli/tests/plugins_toggle.rs` proves each
  plugin disables without a trace. Plugins register settings under their own
  `plugin_id.*` namespace and keep state in their own struct.
- **No plugin hook runs per frame or per row.** Providers (metadata,
  thumbnails, locations) work in batches and streams on the scheduler.
- **Provenance.** Metadata fields record where they came from
  (`Provenance::{Filesystem, User, Plugin, Model}`); model output is a
  suggestion until promoted to `User`.
- **Toolkit isolation.** gpui appears only in `bibli-ui` (and the binary).
  Views implement `PaneView` in `bibli-ui`; a toolkit-dependent `ui-api`
  crate is deferred until a third-party view exists.
- **Vendored gpui.** `vendor/gpui` is gpui 0.2.2 from crates.io plus our
  patches, wired through `[patch.crates-io]`; each change is a diff in
  `vendor/patches/` in Zed's layout, ready to upstream. Read
  `vendor/gpui/README.bibli.md` before touching it. gpui's API moves between
  releases: read the vendored source, not memory. Always set an explicit font
  family (the default stack costs 35-50 ms per frame when it fails to resolve).
- **Conventions.** Key bindings use gpui notation (`ctrl-shift-n`); `when` is
  a space-separated list of tokens that must all hold (`selection`, `single`,
  `dir`, `file`; `pane`/`address`/`palette` for context). Menu `order` groups
  by the hundreds digit (0xx open, 1xx edit, 2xx create, 3xx destructive,
  4xx misc); the UI draws separators between groups. `ui.*` commands are
  handled by the shell; a plugin may bind to them.
- **Unsafe.** `unsafe_code` is denied by lint; the one allowance (setting an
  env var before threads exist, `crates/bibli/src/gpu.rs`) carries an
  `#[allow]` with the justification. Do likewise or don't.
- **Decisions.** Anything that closes a door gets an ADR in `docs/adr/`.
  Every phase updates the status paragraph in `docs/PLAN.md`, including
  deviations from the plan, honestly.

## Layout

```
crates/core        bibli-core: entries, locations, commands, ops/plans, registries,
                   plugin host, scheduler, cache, settings, thumbs, metadata
crates/platform    per-OS adapters: MIME, icons, thumbnails, places, open, terminal
crates/ui          the gpui shell: views, panels, prompts, keymap, elements
crates/bibli       the binary: wires plugins and starts the UI
crates/plugins/*   one crate per plugin (see docs/PLAN.md Phase 2 and 4)
crates/bench       synthetic trees and the performance regression check
vendor/gpui        patched gpui 0.2.2 (see vendor/gpui/README.bibli.md)
spikes/            the Phase 0 toolkit spikes, kept runnable
docs/              PLAN.md, adr/
```

## Build, test, lint

The dev environment is the Nix flake (`flake.nix`, loaded by direnv). Inside it:

```
just build          cargo build --workspace
just test           cargo nextest run --workspace --no-tests=pass
just lint           cargo fmt --check + clippy --all-targets -D warnings
just bench-check    generate bench-data/ on demand and enforce budgets
just run [dir]      run the release binary
just flame [dir]    profile with samply
```

CI runs lint, test, and bench-check; all three must pass before a commit is
considered done. Clippy runs pedantic with the allowances listed in the root
`Cargo.toml`; do not add allowances in code without a reason in a comment.

Tests that need a real filesystem feature (trash, xattrs) use a temp dir under
`~/.cache/bibli-tests/`, never `/tmp` (tmpfs) and never real user files.
`bench-data/` is generated by `bibli-bench gen` and is git-ignored.

## Verifying UI changes

- Run the real app on `bench-data/images10k` (grid) and `bench-data/flat100k`
  (list); scroll; check idle CPU returns to ~0% (`top -p $(pgrep -x bibli)`).
- Drive it without touching the user's session: Hyprland
  `hyprctl dispatch 'hl.dsp.send_shortcut({mods="CTRL", key="2", window="class:bibli"})'`
  sends keys to the window; `grim` captures a screenshot (`-s 0.5` for the whole
  layout). For drags, a virtual pointer on the wlr protocol is the only way
  (the dragtool used in Phase 3 lives in the session scratchpad; recreate it
  from `wayland-protocols-wlr` if needed).
- Measure: first frame from the shell's log line, memory from
  `/proc/<pid>/smaps_rollup` (PSS), corrections/traces from `RUST_LOG=bibli_ui=trace`.

## Shell hygiene (learned the hard way)

- Never put backticks inside a double-quoted `git commit -m "..."`; bash runs
  them. Write the message to a file and use `git commit -F`.
- `pkill -f <pattern>` matches the shell running it when the pattern appears
  in your own command line; use `pkill -x name` or look up pids via
  `hyprctl clients -j`.
- Chain verification with `&&` before a commit so a failing lint or test
  cannot be committed.

## Working as several agents

- Scaffold crates and register workspace dependencies/aliases before parallel
  work starts, so nobody edits the root `Cargo.toml` concurrently.
- Each agent owns its crates only. Plugin agents never edit `crates/core`,
  `crates/platform`, `crates/ui`, or `crates/bibli`; they work around gaps
  locally and report the exact signature they wished for. The integrator
  applies core changes.
- Do not commit from a subagent; the integrator verifies (lint, tests,
  bench-check) and commits per track.
- Reports state what was verified and how, measurements with units, and gaps.
  Never describe something as working that was not exercised.
