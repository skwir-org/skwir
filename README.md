# bibli

An experimental file manager. Fast no matter what, light on the machine,
stateless, and built to be driven by people, scripts, and AI agents alike.

bibli is written in Rust on a GPU-accelerated UI (gpui, the toolkit behind
Zed). It runs on Linux today; macOS and Windows are planned. It is early: the
plan is in `docs/PLAN.md`, and the status paragraph there says exactly what
works and what was left out.

## What it is like

- **A query is a place.** Type `*.pdf modified<7d size>10M` and the results
  are a location you can navigate to, come back to, and save as a smart folder.
- **Everything is a command.** Every action is a named command with a JSON
  schema, reachable from the keyboard, the palette, and, later, a CLI and an
  MCP server. An agent uses the same surface you do.
- **Changes are plans.** Batch operations, the external-editor flow, and
  anything an agent proposes appear as a reviewable list of steps before they
  run. Every operation has an inverse; undo works across the board.
- **Nothing is indexed behind your back.** No daemon, no database. bibli
  reuses the thumbnails, icons, MIME database, bookmarks, recent files, and
  tools already on your system, and keeps one cache you can delete at any
  time.
- **Views over one model.** List, grid gallery, Miller columns, dual pane,
  a disk-usage treemap, and a modification-time timeline are all views of the
  same listing.

Features today: streaming directory listings that stay responsive on 100k
entries; lazy metadata; thumbnails from the freedesktop cache with visible-first
generation; preview panel and spacebar peek; drag and drop in and out, on
Wayland; search with content matching; fuzzy jump from zoxide and recent
files; a shelf to collect items from anywhere; bulk rename with live preview;
edit the listing as text in your editor; tags stored as xattrs; run a shell
command on the selection; disk usage; timeline; two panes.

## Running it

You need a Rust toolchain and, on Linux, Wayland or X11 with Vulkan. The
repository ships a Nix flake with everything:

```
direnv allow        # or: nix develop
just run ~/Documents
```

Without Nix: `cargo run --release -p bibli -- <dir>`. The native libraries
needed are listed in `flake.nix`.

Useful keys: `ctrl-shift-p` command palette, `ctrl-l` address bar, `ctrl-f`
search, `ctrl-j` jump, `ctrl-1/2/3` list, grid, columns, `F3` preview, space
peek, `ctrl-shift-d` split, `ctrl-shift-u` disk usage, `ctrl-shift-l`
timeline, `ctrl-shift-b` add to shelf, `ctrl-f2` bulk rename, `ctrl-e` edit
as text, `ctrl-z` undo. Every command and its key is in the palette.

## Design in one paragraph

A small core (`crates/core`) owns what two plugins must agree on: entries
with stable identity and provenance-tagged metadata, locations and streaming
listings, commands, operations and plans with an undo journal, registries,
and a priority scheduler. Everything else is a plugin (`crates/plugins/*`)
that can be switched off at runtime. Platform adapters (`crates/platform`)
harness the OS. The UI (`crates/ui`) is a thin shell over the core and the
only crate that knows the toolkit. Performance budgets are measured by
`crates/bench` and enforced in CI.

## Contributing

Read `AGENTS.md` first: it holds the principles, the engineering rules, the
build and verification workflow, and the conventions, for humans and agents.
Decisions that closed a door are recorded in `docs/adr/`.

```
just lint && just test && just bench-check
```

## License

MIT or Apache-2.0, at your option.
