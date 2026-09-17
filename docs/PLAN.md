# bibli — Implementation Plan

Experimental file manager in Rust. GPU-accelerated UI, tiny core, everything else a plugin,
designed from the first commit to be driven by humans, scripts, and AI agents alike.

Principles (non-negotiable, every phase is checked against them):

1. Always responsive: no blocking work on the UI thread, ever. Everything streams, everything cancels.
2. Light: idle RSS and startup time are budgeted and measured in CI.
3. Stateless and idempotent: no daemon, no database. One throwaway cache. Re-applying a plan is a no-op.
4. Harness the environment: reuse OS thumbnails, icons, MIME db, bookmarks, indexes, installed tools.
5. Core = only what two plugins must agree on. Everything else is a plugin with a runtime toggle.
6. Commands are data. Plans before apply. Stable entry identity. Metadata carries provenance.

Durations are relative sizes for one developer working with an AI assistant, not commitments.
Each phase has a **Done when** gate. Do not start the next phase before the gate passes.

Status: Phase 0 and Phase 1 done (2026-09-18). Next: Phase 2 (minimal daily driver on GPUI).

---

## Phase 0 — Foundations and toolkit spike (~2 weeks)

Goal: a repo that can measure itself, and a toolkit decision backed by numbers.

- Cargo workspace: `crates/core`, `crates/platform`, `crates/ui`, `crates/plugins/*`, `crates/bench`.
- Nix flake dev shell (this is a NixOS machine), `just` recipes, CI (lint, test, bench on Linux).
- Benchmark harness generating synthetic trees on demand (never committed):
  flat 100k-entry dir, 1M-file deep tree, dir of 10k large images.
- Written performance budgets, enforced by `bench` as CI regression checks:
  - cold start to first frame < 100 ms
  - 100k-entry directory: first rows painted < 16 ms after listing starts, scroll at display refresh rate
  - idle RSS < 100 MB with default plugins, identical with AI plugins disabled vs not compiled
- Toolkit spike: the same demo in **GPUI** and **Floem**: a virtualized 100k-row list with lazy
  thumbnails, on Wayland (Hyprland), on the Intel iGPU and the NVIDIA eGPU. Measure first paint,
  frame time while scrolling, RSS, and test external drag-and-drop and IME once.
- ADR-001 records the toolkit choice and the numbers. ADR-000 records the principles above.

**Done when:** budgets are documented, `bench` runs in CI, the ADR is written, and the winning
spike renders 100k rows at refresh rate on both GPUs.

---

## Phase 1 — Core kernel (~4 weeks)

Goal: `crates/core` is complete enough that plugins can be written against it, with no UI.

- **Entry model.** `EntryId` = (device, inode) on Unix, file id on Windows. Streaming listing via
  `getdents` batches (rustix), `d_type` used to avoid stat, lazy stat on demand. Entries carry a
  metadata bag: typed fields with **provenance** (Filesystem, User, Plugin(name), Model(name, confidence)).
- **Watching.** `notify`-based, per open location, coalesced, feeds the same entry stream.
- **Locations.** `LocationProvider` trait: string → entry stream. Only the path provider ships in core.
- **Commands as data.** `Command` = serde struct with typed args, registered by name, dispatched by
  the core. Keymap and palette are consumers of the registry. Nothing in a command captures UI state.
- **Operations and plans.** `Operation` trait with `apply` and `inverse`. `Plan` = ordered list of
  operations referencing `EntryId`s, with a dry-run `diff()`. Executor runs plans in the background
  with progress and cancellation and writes an undo journal. Re-applying an applied plan is a no-op.
  Built-in ops: copy (reflink when possible), move, rename (atomic exchange for swaps), trash, delete, mkdir.
- **Registries.** Views, panels, sidebar contributions, context-menu contributions, settings schema.
- **Plugin lifecycle.** `Plugin` trait: `register(&mut Registry)`, `enable`, `disable`. Compile-time
  crates, runtime toggle in settings. Plugins depend only on core, never on each other.
- **Cache and settings.** Content-addressed cache dir, deletable at any time. Plugins get a namespaced
  slot in cache and in settings, nothing else.
- **Work scheduler.** One priority queue: viewport-visible work first, everything cancellable.
  Provider calls are batched, async, timed; slow providers are surfaced.
- Tests: property tests that `inverse(apply(op))` restores the tree; idempotency of plans; listing
  benchmarks against the synthetic trees.

**Done when:** a headless test drives a plan (list, select, rename, trash, undo) through commands only,
and listing the 1M-file tree streams without ever blocking the caller.

---

## Phase 2 — Minimal daily-driver (~4 weeks)

Goal: replace the file manager you use every day, even if ugly.

- `crates/ui` on the chosen toolkit: window, layout with panes, theme tokens, command palette,
  keymap, address bar (paths only for now), status/progress area. **AccessKit wired in from the start.**
- Plugins: `view-list`, `places` (XDG user dirs, GTK bookmarks, udisks2 volumes over D-Bus),
  `ops-basic` (the built-in operations exposed as commands with keybindings), `open-with` (xdg-mime),
  `undo` (journal UI).
- `crates/platform` Linux: shared-mime-info via the `xdg-mime` crate, XDG icon theme lookup,
  freedesktop trash, **read-only** freedesktop thumbnail cache lookup.
- Keyboard-first: every action reachable from the palette; mouse works but is secondary.
- Dogfooding starts here: use it daily, file issues.

**Done when:** you have used it as your only file manager for one week, budgets still pass,
and every action taken during that week went through a `Command`.

---

## Phase 3 — Visual richness and performance hardening (~4 weeks)

Goal: it looks and feels like the fast, GPU-native app it is.

- **Thumbnail pipeline.** `ThumbnailProvider` trait keyed by MIME. Freedesktop cache read first,
  generation on miss written back with spec-compliant naming (other apps benefit). Discovery of
  installed `*.thumbnailer` files. Decode on a rayon pool, viewport-priority, cancellable, GPU atlas upload.
- Plugins: `view-grid` (gallery), `view-columns` (Miller columns), `preview` (image, syntax-highlighted
  text, PDF via pdfium, video first frame; spacebar peek).
- Drag-and-drop: internal first, then external on Wayland; measured against the spike findings.
- Perf pass: profile with the synthetic trees, fix every budget regression, add flame-graph recipe.

**Done when:** the 10k-image directory scrolls at refresh rate with thumbnails appearing in
visible-first order, and all three views pass the budgets.

---

## Phase 4 — Power features, each as a plugin (~6 weeks)

Goal: the UX bets. Order is by leverage; each is independently shippable and toggleable.

1. `query` — query language (`*.pdf modified<7d size>10M`), a `LocationProvider`, so search results
   are a location. Saved queries = smart folders.
2. `search` — parallel subtree walk (`ignore` crate walker), results cached by directory mtime;
   uses `plocate` when present. Content search via the ripgrep crates.
3. `shelf` — collect entries from anywhere, apply one plan to all of them.
4. `rename-bulk` — patterns, regex, live preview; produces a plan.
5. `edit-as-buffer` — the directory listing as an editable text buffer, diff → plan (oil.nvim model).
6. `jump` — frecency jump reusing zoxide's db and recently-used.xbel when present; `nucleo` matching.
7. `shell` — open terminal here, mirror cwd, drop selection as arguments.
8. `tags` — xattrs using the XDG and Finder keys; tags are metadata with User provenance.
9. `view-dual` — orthodox two-pane mode over the same model.
10. `usage` — GPU treemap, btrfs-aware.
11. `timeline` — files by mtime as a scrubbable histogram.

By the end there are 10+ internal plugins. That count is the signal that the traits are ready to freeze.

**Done when:** every plugin above can be disabled at runtime with no trace in memory or UI, and
the trait surface has not changed for the last three plugins written.

---

## Phase 5 — Cross-platform (~6 weeks)

Goal: macOS and Windows builds that feel native in the ways that matter.

- `crates/platform` macOS: QuickLook thumbnail service, NSWorkspace icons and open-with, Trash,
  DiskArbitration volumes, Spotlight as a search provider, Finder tag xattr compatibility.
- `crates/platform` Windows: shell thumbnail cache, SHGetFileInfo icons, Recycle Bin, WM_DEVICECHANGE
  volumes, Windows Search as a provider, NTFS USN journal for fast subtree walks.
- File identity, path handling, and reflink/clone (APFS clonefile, ReFS block clone) abstracted in core.
- CI on all three OSes; budgets measured on each.
- Packaging: Nix flake and AppImage/Flatpak, dmg, msi.

**Done when:** the Phase 2 dogfood week is repeated on each OS, and the budgets pass on all three.

---

## Phase 6 — The extension boundary (~4 weeks)

Goal: open the plugin system beyond the repo, safely. Only now, because the traits are stable.

- **Permission layer.** Capabilities per plugin (read contents, write, network, spawn processes) and
  per-directory policies ("never leaves this machine" for `~/.ssh`, anything under a `.private` marker).
  Required before any model provider exists.
- **Level 2, external processes.** Small line-delimited protocol (LSP-style) for provider processes.
  Used first for thumbnailers and external tools we already call.
- **Level 1, WASM plugins.** wasmtime + component model for providers, commands, metadata. Not views.
- **CLI and MCP server.** Both are thin consumers of the command registry: `bibli cmd rename ...`,
  and an MCP server exposing commands and plans so an external agent (e.g. Claude Code) can drive the app.
- Plugin authoring docs and a template crate.

**Done when:** one existing internal plugin is re-implemented as WASM with no user-visible difference,
and an external agent completes a rename-and-move task over MCP that ends in a reviewable plan.

---

## Phase 7 — AI (~10 weeks)

Goal: agents that organize, rename, clean, tag, and summarize, always ending in a plan the user reviews.

1. `extract` — content extraction provider: text from PDFs/docs, OCR, EXIF, audio/video metadata.
   Cached by content hash. Uses installed tools where present. Benefits plain search immediately.
2. `cleaners` — deterministic: exact duplicates (hash), near-duplicate images (perceptual hash),
   empty dirs, stale downloads. No model. Each produces a plan.
3. `models` — `ModelProvider` trait: chat with tool calling, embeddings, image understanding,
   transcription. Implementations: local runtime (lazy-loaded), auto-detected Ollama, remote APIs
   behind explicit opt-in with a persistent "leaves your machine" indicator. Idle cost when disabled: zero.
4. `models-small` — embeddings, CLIP-style image tagging/search, OCR, document classifier.
   Run on CPU or iGPU. These do the volume; the LLM orchestrates.
5. `semantic` — scoped, on-demand, incremental subtree index in the throwaway cache; visible and
   pausable. Natural language becomes a `LocationProvider` in the same address bar.
6. `agent` — the LLM plugin. Tools = the command registry. Output = a `Plan`. Never executes.
   Prompts treat file names and contents as untrusted input. Flows: organize this folder,
   rename these consistently, what is safe to delete here, tag the selection, summarize in preview.
7. Metadata from models carries `Model(name, confidence)` provenance and renders as suggestions
   until confirmed, at which point it is promoted to User provenance and written to xattrs.
8. MCP client, so the agent can use external tools the user configures.

**Done when:** "organize this folder" on a messy Downloads produces a correct, undoable plan using
only local models, and disabling every AI plugin returns idle RSS to the Phase 2 baseline.

---

## Phase 8 — Polish and release (ongoing)

- Accessibility audit (screen reader on each OS), IME, high-DPI, themes.
- Docs, plugin gallery, first public release with an opinionated default plugin set enabled.

---

## Cross-cutting rules

- Every phase ends with a benchmark run and an ADR for any decision that closes a door.
- Features enter as plugins by default. Promotion into core requires the "two plugins must agree" argument.
- No plugin hook may run per frame or per row. Batches and streams only.
- Any AI-derived data is a suggestion until a human confirms it.

## Immediate next steps (first week)

1. Init the workspace, flake, CI, and `bench` with the synthetic-tree generator.
2. Write ADR-000 (principles and budgets).
3. Build the GPUI spike, then the Floem spike, run both on both GPUs, record numbers.
4. Write ADR-001 (toolkit decision).
