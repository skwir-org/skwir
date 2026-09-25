# ADR-010: Settings apply immediately, from a schema that describes its controls

Status: accepted, 2026-09-20

## Context

The first settings dialog listed every declared key in one column. A
`SettingSpec` held a key, a description, a default and constraints, so labels
were derived from keys (`budget_mb` became "budget mb"), every value was edited
as text behind the scenes, and nothing grouped the list. Changes were a draft
submitted with Save. Plugins could be toggled at runtime (principle 5) but no
surface did it: `PluginHost::set_enabled` was called by tests only.

## Decision

**The schema describes the control.** `SettingSpec` gains a title, section,
unit, step, choice labels, a `restart_required` flag and a `SettingKind`
(`toggle`, `choice`, `integer`, `number`, `text`, `path`, `command`, `data`),
built through constructors. The owning plugin states all of it; the dialog, the
palette and the JSON the schema serializes to for agents contain no
key-specific knowledge. `data` marks collections (saved queries) that live in
the settings file but are managed by commands, not editors.

**Changes apply immediately.** Switches and choices submit on click; text and
numbers on Enter or blur. Each change is a `settings.apply`, which stays the
single atomic, validated, worker-side save. The core still accepts one save at
a time; the dialog queues what arrives meanwhile and sends it as one batch, and
waits the same way when another surface holds the save. A failed save restores
the live value and reports the reason on the row. Invalid text is never sent.
There is no Save or Cancel; closing the dialog, by Escape, the close button or
a click outside, loses nothing.

**The dialog is an independent view.** `SettingsView` holds the `App`, not the
shell, reconciles against `Settings::revision` and `PluginHost::revision` when
the core wakes the UI, and closes through an event. It is hosted as the last,
non-deferred overlay of the shell, because its dropdowns are deferred draws and
GPUI cannot nest them. Layout: a category column (General, Appearance, Plugins,
Keyboard, then each plugin that declares settings) with one search field over
settings, plugins and shortcuts; pages are cards of rows with the control on
the right and a revert button on rows that differ from their default.

**Plugins are managed through commands.** `plugins.set_enabled` runs the
lifecycle callbacks on a worker and persists the choice; `plugins.list` reports
the state. Each plugin states its `Plugin::category` (browsing, files,
previews, desktop), and the Plugins page lists them under those headings.
`Plugin::requirements` lists external tools with a probe; the dialog
runs the probes on a worker when it opens and shows what is missing and what
that costs.

**Settings are addressable.** `ui.settings` takes `{ key }` or `{ page }`; the
palette lists every setting; a notification can name the setting that resolves
it (`App::notify_about_setting`) and the status bar links to it.

## Consequences

- A separate settings window was considered and deferred. GPUI supports it
  (`WindowKind::Floating` sets the Wayland parent), but it costs a second GPU
  surface and atlas while open, and a tiling compositor may tile it beside the
  main window. Because the view does not know its host, moving it is a small
  change to make once it can be judged on the reference desktop.
- Instant apply means a change cannot be abandoned, only reverted: every
  modified row offers its default, and General offers all defaults at once
  behind a second click.
- The Keyboard page is read-only. Editing bindings needs a persisted keymap
  layer and conflict detection, which is a separate decision.
- The theme preference (`ui.theme`) is declared by the shell, so headless
  tools do not see it.

## Addendum (2026-09-25): a page per subject

The shell's preferences all lived under `ui.*`, and the dialog shows a
namespace as a page, so Appearance held how documents read (continuous
scrolling, reading width, printed page numbers) and, for a day, how audio
and video play. A preference now goes where its subject is:

- About one plugin's content (an engine, the player of audio and video):
  that plugin's namespace and page, even when the shell reads it (ADR-016).
- Applied by the shell to several plugins' content: a namespace of the
  shell's own with a page of its own. `reading.*` (`continuous`, `width`,
  `printed_pages`) is the Reading page: the reader and reading mode serve
  the documents, EPUB, comics and Markdown plugins, and none of them
  switched off may take the others' preferences with it.
- `ui.*` is Appearance, only how bibli looks; `core.*` is General.

A moved key keeps the user's value: its owner calls
`Settings::rename(old, new)` before declaring it, which moves a stored
value when nothing is stored under the new name, and the next save drops
the old name from the file.
