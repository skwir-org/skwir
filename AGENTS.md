# Working on bibli

bibli is an experimental file manager: Rust, GPU-accelerated UI (gpui), a tiny
core, everything else a plugin, designed from the first commit to be driven by
humans, scripts, and AI agents alike. This file is the context every agent
(human or model) needs before touching the code. Read it fully once.

Start here: `docs/PLAN.md` (phases, gates, current status and recorded
deviations), `docs/adr/` (decisions that closed a door; ADR-000 is the
principles and budgets), then the crate you are working in.

`docs/PLAN.md` includes future designs as well as implemented work. Check the
current source before assuming a planned crate, command, or API exists.
`CLAUDE.md` includes this file; keep shared contributor guidance here.

## Privacy and fixtures

- Never add personally identifying information to the repository, including
  tests, documentation, screenshots, captured logs, and commit messages.
  Paths and filenames from a bug report are private input, not test fixtures.
- Reproduce bugs with synthetic names that preserve the relevant properties
  (apostrophes, whitespace, Unicode, nesting, etc.). Use temporary directories
  or generic example paths; never hard-code a real user's home directory.
- Review the staged diff and new artifacts for personal data before committing.
  Do not copy raw session output into repository files without sanitizing it.

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
- **Toolbars.** Reuse command-backed buttons and bundled symbolic SVGs in
  `crates/ui/src/toolbar.rs`. Responsive rows select variants by intrinsic
  measurement in `elements/adaptive.rs` prepaint; do not maintain pixel
  breakpoints beside changing labels. Tooltips, focus styles and keyboard
  activation belong to the shared button. Pane actions capture their target
  during rendering and need pane-specific element IDs. Popovers restore focus
  when dismissed; retain a focused button's handle when it becomes disabled.
  GPUI already emits Enter/Space clicks on key-up: do not dispatch again on
  key-down.
  Preview types share their frame in `preview.rs` and zoom controls in
  `preview_toolbar.rs`; keep document-only tools in `reader.rs`. Preview menus
  belong to the selected preview and close when its content is replaced by a
  different file or its presentation closes.
  Anchor popovers to their visible trigger through `elements/popover.rs`;
  measure anchors in the current prepaint, including after responsive reflow.
  GPUI cannot nest deferred draws: keep peek in the normal overlay tree and
  defer its popovers, below modal dialogs.
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
- **Locks and re-entry.** Snapshot the inputs needed under a lock, then drop
  the guard before dispatching another command, navigating, invoking a plugin,
  or doing I/O. These calls can acquire the same workspace or registry locks.
- **Pane ownership.** Resolve the target `PaneId` when work starts and carry it
  through commands and results. Do not redirect a delayed action to whichever
  pane happens to be active when it finishes. Layout callbacks that use shell
  state must restore the intended pane with `Shell::with_pane`.
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
- **Unsafe.** The workspace sets `unsafe_code = "warn"`; `just lint` promotes
  warnings to errors. Any exception needs a narrowly scoped `#[allow]` with a
  justification, as in `crates/bibli/src/gpu.rs` (setting an environment
  variable before threads exist). Do not weaken the workspace lint.
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
crates/pdf-worker  optional native PDF reader process (runtime-loaded PDFium)
crates/poppler-worker optional native PDF reader process (linked poppler-glib)
crates/media-worker optional audio/video player process (linked GStreamer)
crates/archive-worker optional archive reader process (linked libarchive)
crates/plugins/*   one crate per plugin (see docs/PLAN.md Phase 2 and 4)
crates/bench       synthetic trees and the performance regression check
vendor/gpui        patched gpui 0.2.2 (see vendor/gpui/README.bibli.md)
spikes/            the Phase 0 toolkit spikes, kept runnable
docs/              PLAN.md (plan and current status), STATUS-LOG.md, adr/,
                   invariants/ (binding area rules)
```

## Implementation entry points

- `crates/bibli/src/main.rs` wires plugins; `crates/ui/src/shell.rs` routes
  commands and maintains pane UI state. Built-in list/grid/columns views are
  in `crates/ui/src/views/`, not separate plugin crates.
- Plugin views register owner-tagged `ViewSpec::named` descriptors; their GPUI
  renderers implement `PaneView` in `bibli-ui` and are resolved in
  `views::named_renderer`. Do not hard-code plugin views into the toolbar or
  palette. Reconcile registration identity, including disable/re-enable between
  frames, and fall back to List when the active view disappears. Publish data,
  advance the descriptor revision, then wake the app; never query plugin data
  on every frame. See ADR-007 for the Usage cache and watcher lifecycle.
- New plugins need the workspace dependency alias, the binary dependency and
  registration, and an entry in `crates/bibli/tests/plugins_toggle.rs`.
  Extend that test's snapshot when adding a provider scheme or a new kind of
  contribution; its provider-scheme list is explicit.
- `Plugin::register(&App)` runs again on re-enable. `App::unregister_owner`
  withdraws registered contributions; `Plugin::on_disable` must cancel work
  and release resources held outside registries. Test disable and re-enable,
  including in-flight work when the plugin starts background tasks.
- Integration tests involving several plugins belong in `crates/bibli/tests/`
  (see `timeline_query.rs`), keeping plugin crates independent of each other.

## Background work and streamed state

- `CommandRegistry::dispatch` executes on the caller's thread; dispatching a
  command does not move it to a worker. Likewise, `LocationProvider::open` and
  `OpenLocation::hydrate` are synchronous entry points: enqueue expensive work
  and return promptly. `pump_until_complete` and executor blocking methods are
  for headless tests/tools, never UI callbacks.
- Send listing events through `OpenContext::channel` / `EventSink`, which wake
  the UI after enqueueing. Other background results must call `App::wake` or a
  captured `waker_handle` after publishing. `Complete` ends the initial listing;
  updates and removals may still arrive afterward.
- The scheduler skips cancelled jobs before they start; running jobs must
  check their token themselves. Use child tokens for work tied to the app or
  an open location, and cancel superseded requests. Only accept results for the
  current request (e.g. the generation check in `crates/ui/src/preview.rs`);
  cancellation alone does not prevent stale results.
- Pane-owned background work links to `Pane::location_lifetime`; navigation,
  Back and pane close cancel it independently of the next UI render. Replace
  locations through `Pane::replace_open` so no navigation path misses this.
  Directory open/stat/watch setup belongs on workers too: a sleeping disk can
  block any of those calls. Opening failures arrive as listing errors.
- Usage installs non-recursive watches as its cancellable scanner visits each
  directory, before reading it. Never delegate a whole recursive walk to a
  watcher API: it cannot observe our request cancellation. Cache only completed
  breakdowns, and invalidate data collected without reliable watch coverage.
- Scope filesystem invalidation and fallback deadlines to affected subscriptions.
  Cache eviction is not a content change; an unrelated event must not rescan every
  pane. Keep queued/running state separate from delayed visible progress so quick
  cache hits do not flash a busy indicator.
- Progressive Usage snapshots are lower bounds, explicitly marked incomplete.
  Keep partial trees out of the completed cache, bound the visible snapshot,
  and preserve entry identity as blocks reorder. Once a scan has finished,
  retain that chart through subsequent refreshes and replace it atomically;
  a finished result can still be incomplete because of read errors. Automatic
  revalidation must not reintroduce unknown blocks or busy flashes. See ADR-007.
- Entries arrive before metadata. Treat missing fields as unknown, request
  hydration in batches, and react to `Updated` events instead of stat-ing each
  row during rendering. For virtual locations, use `OpenLocation::path_of` or
  `App::selected_refs`; a query/shelf location is not a filesystem directory.

## Settings

- Declare preferences in `SettingSpec`; the settings dialog derives controls
  from their types and constraints. Choices and numeric limits belong to the
  owning plugin, not UI key-specific branches. Collections such as saved queries
  remain application data managed by their existing commands.
- UI drafts stay local until `settings.apply`. Submit only edited keys; validate
  the whole batch, save atomically on a worker, then publish live values. A failed
  write must retain both the previous configuration and the editable draft.
- Plugins that capture preferences at registration implement
  `on_settings_changed`; refresh only their affected providers. Read-through
  preferences need no hook. Lifecycle callbacks are serialized and must not
  recursively enable/disable plugins; registry and settings locks are released
  before invoking them. Never invoke lifecycle callbacks from a UI render.
- Never trace settings values; paths and shell preferences can be private.
  Exercise saves against an isolated config directory, never the user's file.

## Area invariants (binding, read before working in the area)

The rules for the larger subsystems live next to their designs so this file
stays readable. Each is as binding as this file; read the one for the area you
are about to touch, in full:

- `docs/invariants/reader.md`: PDF readers (PDFium and Poppler workers,
  continuous scrolling, page coordinates, atlas eviction).
- `docs/invariants/media.md`: video and audio playback (GStreamer worker,
  frame slots, generations, fullscreen controls, Blade atlas retirement).
- `docs/invariants/previews.md`: structured previews (inspections, archives,
  HTML reading mode, EPUB and email, code folds).

## Paths and generated queries

- Build copy operations with `Copy::from_settings` so every command, shelf,
  clipboard and drop path captures the same tag-preservation preference in
  the serialized plan. Never read live preferences during apply or redo.
  Copy tags before restoring read-only permissions and publish the staged
  copy only after metadata succeeds; failed copies must not look complete.

- Keep paths as `Path` / `PathBuf` and process arguments separate for as long
  as possible. Shell quoting, query quoting, and URI encoding are different
  grammars; a display string is not an escaped representation of any of them.
- When another plugin needs a search, dispatch `query.search` with separate
  `expr`, `root`, and `pane` arguments (see `timeline.select`). The query plugin
  owns literal escaping in `lang::quote`; do not interpolate raw paths into
  `in:` expressions or add a dependency on the query plugin to reuse helpers.
  Handle a disabled command/provider as a normal error.
- Query date ranges include the whole end date. A half-open histogram bucket
  `[start, end)` needs `modified>=start modified<end`. Calendar boundaries in
  the query parser and timeline plugin are UTC; UI labels use the system time
  zone. Test boundary instants explicitly when changing date behavior.

## Build, test, lint

The dev environment is the Nix flake (`flake.nix`, loaded by direnv). Inside it:

```
just build          cargo build (default members: no native workers)
just workers        build the Poppler, GStreamer and libarchive workers
just test           cargo nextest run (default members)
just test-all       every crate, native workers included (what CI runs)
just lint           cargo fmt --check + clippy --all-targets -D warnings
just lint-all       the same over the whole workspace (what CI runs)
just bench-check    generate bench-data/ on demand and enforce budgets
just run [dir]      run the release binary
just flame [dir]    profile with samply
```

Without an active dev shell, use `nix develop --command just <recipe>`, as CI
does. `spikes/` is a separate workspace and is not covered by the root checks.

CI runs lint-all, test-all, poppler-check and bench-check; all must pass before
a commit is considered done. The native workers are outside `default-members`
so the default build and tests need no Poppler, GStreamer or libarchive;
run the `-all` recipes (inside `nix develop`) before committing worker changes. Clippy runs pedantic with the allowances listed in the root
`Cargo.toml`; do not add allowances in code without a reason in a comment.

Tests that need a real filesystem feature (trash, xattrs) use a temp dir under
`~/.cache/bibli-tests/`, never `/tmp` (tmpfs) and never real user files.
`bench-data/` is generated by `bibli-bench gen` and is git-ignored.

- Use `AppConfig::ephemeral` for headless tests so settings and cache writes
  stay isolated. Await listing completion or events with a deadline rather
  than relying on fixed sleeps; `crates/core/tests/common/mod.rs` has examples.
- Test the user-facing command path for cross-plugin bugs, not just the helper
  that formats a string. Include synthetic special-character paths and assert
  the returned entries, not merely that parsing succeeds.
- For operation property tests, verify undo restores the original tree and
  redo restores the applied state, including partial failures. Applied steps
  can cancel each other out (e.g. two swaps); a nonzero applied count does not
  imply a different final tree. Investigate and preserve reproducible failure
  cases; a passing randomized rerun does not resolve a previous failure.
- `just bench-check` enforces the microsecond budgets in
  `crates/bench/budgets.toml`. It does not validate UI startup, frame times, or
  idle PSS; those require the real-app measurements below and ADR-002.

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
- `target/` grows without bound: cargo never garbage-collects stale
  dependency artifacts, and one day of parallel agents left 141 GB there on a
  root filesystem that also hosts `/tmp`. Check `du -sh target` when builds
  start failing with ENOSPC and remove `target/debug` (rebuilt by the next
  test run). Real-window checks must delete their fixture trees under `/tmp`.

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
