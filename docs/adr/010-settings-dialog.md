# ADR-010: Settings apply immediately, from a schema that describes its controls

Status: accepted, 2026-09-20. Amended 2026-09-25 (a page per subject),
2026-09-27 (records, owners' checks, secrets), 2026-09-28 (the user's keys,
ADR-024). The page of a setting is its subject's, not its namespace's,
since ADR-072 (2026-10-19).

## Context

The first dialog listed every key in one column. A `SettingSpec` held a key,
description, default and constraints, so labels came from keys (`budget_mb`
became "budget mb"), every value was edited as text, and nothing was grouped.
Changes were a draft submitted with Save. Plugins could be toggled at runtime
(principle 5), but only tests called `PluginHost::set_enabled`.

## Decision

- **The schema describes the control.** `SettingSpec` gains a title,
  section, unit, step, choice labels, `restart_required` and a `SettingKind`
  (`toggle`, `choice`, `integer`, `number`, `text`, `path`, `command`,
  `data`), built through constructors. The owning plugin states all of it;
  the dialog, the palette and the schema agents read hold no key-specific
  knowledge. `data` is a collection (saved queries) kept in the settings file
  but managed by commands.
- **Changes apply immediately.** Switches and choices on click, text and
  numbers on Enter or blur, each a `settings.apply`: still the single atomic,
  validated, worker-side save. The core takes one save at a time; the dialog
  queues what arrives meanwhile and sends it as one batch, and waits the same
  way for another surface's save. A failed save restores the live value and
  shows the reason on the row. Invalid text is never sent. No Save or Cancel:
  closing (Escape, the close button, a click outside) loses nothing.
- **The dialog is an independent view.** `SettingsView` holds the `App`, not
  the shell, reconciles against `Settings::revision` and
  `PluginHost::revision` when the core wakes the UI, and closes through an
  event. It is the shell's last, non-deferred overlay: its dropdowns are
  deferred draws and GPUI cannot nest them.
- **Layout.** A category column (General, Appearance, Plugins, Keyboard, then
  each plugin with settings) and one search over settings, plugins and
  shortcuts. Pages are cards of rows: the control on the right, a revert
  button where the value differs from the default.
- **Plugins through commands.** `plugins.set_enabled` runs the lifecycle
  callbacks on a worker and persists the choice; `plugins.list` reports
  state. `Plugin::category` (browsing, files, previews, desktop) gives the
  Plugins page its headings. `Plugin::requirements` lists external tools with
  a probe; the dialog runs probes on a worker when it opens and shows what is
  missing and what that costs.
- **Settings are addressable.** `ui.settings` takes `{ key }` or `{ page }`;
  the palette lists every setting; a notification can name the setting that
  fixes it (`App::notify_about_setting`), and its toast links there.

Deferred: **a separate settings window**. GPUI supports it
(`WindowKind::Floating` sets the Wayland parent), but it costs a second GPU
surface and atlas while open, and a tiling compositor may tile it beside the
main window. The view does not know its host, so moving it is a small change
once it can be judged on the reference desktop.

## Consequences

- A change cannot be abandoned, only reverted: each modified row offers its
  default; General offers all defaults behind a second click.
- The Keyboard page is read-only: editing bindings needs a persisted keymap
  layer and conflict detection, which is a separate decision (ADR-024,
  below).
- The shell declares `ui.theme`, so headless tools do not see it.

## Amendments

### 2026-09-25: a page per subject

A namespace is a page, and every shell preference lived under `ui.*`, so
Appearance held how documents read and, briefly, how media plays. A
preference now goes where its subject is:

- About one plugin's content (an engine, the audio and video player): that
  plugin's namespace and page, even when the shell reads it (ADR-016).
- Applied by the shell to several plugins' content: a shell namespace with
  its own page. `reading.*` (`continuous`, `width`, `printed_pages`) is the
  Reading page, shared by the documents, EPUB, comics and Markdown plugins;
  switching one off must not take the others' preferences with it.
- `ui.*` is Appearance (how Skwir looks); `core.*` is General.

A moved key keeps the user's value: the owner calls
`Settings::rename(old, new)` before declaring it, which moves a stored value
when the new name has none; the next save drops the old name. (2026-10-13:
while no one uses skwir, keys are renamed outright, and nothing calls it.
2026-10-30: first called, for the books plugin's id, ADR-106; a plugin's
toggle follows it from its former ids, `Plugin::former_ids`.)

### 2026-09-27: records, owners' checks, secrets

Tile servers are a list of records with several fields, some secret; no kind
held that, and the dialog must not learn a key's meaning to draw it.

- **`SettingKind::Records`** (`SettingSpec::records`): a list of tables, each
  with an `id` and the owner's fields (`RecordField`: text, ranged integer,
  name/value pairs, secret), edited as JSON through `settings.apply`, so
  agents and the CLI edit it too.
- The dialog draws it generically (`crates/ui/src/settings_records.rs`): a
  row per record with the owner's action (`record_action`, a command given the
  record's id, such as a tile server's Test), Edit and Remove. One record is
  edited at a time, in place, its draft kept apart from rebuilt rows; Enter
  saves, Escape cancels.
- The order is the user's: drag a record by its handle into a gap (a line
  marks it), or Alt+Up/Alt+Down from one of its buttons; either saves. A list
  is user data: no Revert, and Reset all leaves it alone.
- **An owner's check** (`SettingSpec::validate_with`) runs after the kind's,
  in the dialog and in the save (a tile address needs `{z}`, `{x}`, `{y}`).
- **A secret never reaches the settings file.** Its field holds only
  `SECRET_STORED` or nothing; the save refuses anything else. The secret goes
  to the desktop keyring (`skwir_platform::secrets`, oo7, which gpui already
  links; on a worker, since the keyring may ask to unlock), keyed by setting,
  record id and field. The dialog validates the list, stores a new secret,
  then saves the list; forgetting a secret or removing a record deletes it
  from the keyring. Readers (map-tiles) read it on their own threads and keep
  it in memory only.
- Keyring tests use `tools/test/private-keyring.sh`: a private D-Bus session
  that activates nothing, a gnome-keyring in a temporary directory, the
  command run only once that keyring owns the Secret Service name. A test
  that needs it refuses to run without the variable the script sets.

### 2026-09-28: the user's keys (ADR-024)

The Keyboard page is editable: `keyboard.bindings`, ADR-024's persisted
layer, a list of records the core declares, edited with the records editor
above the shortcut list, which marks the user's own. The `keyboard`
namespace is this page, as `core` is General. Since 2026-10-13 the list
itself is edited, and the records editor is behind Edit as records
(ADR-024's amendment of that date); a setting's own check sees the value
stored beside the new one.
