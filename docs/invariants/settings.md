# Settings invariants

Read this before declaring a setting or changing the settings dialog
(`crates/ui/src/settings.rs`, `settings_view.rs`, `controls.rs`) or
`skwir_core::settings`. It is binding, like `AGENTS.md`. ADR-010 holds
the reasoning.

## Declaring a setting

- Use the `SettingSpec` constructors: `toggle`, `choice`, `integer`,
  `number`, `text`, `path`, `command`; `records` for a list the user
  builds in the dialog (tile servers); `data` for what the user makes
  through commands rather than the dialog (saved searches, pinned tags),
  kept in the data file (ADR-057); `state` for what skwir keeps of the
  window (ADR-056).
- What only the owner can check goes in `validate_with`.
- The owner states the title, description, unit, step, choice labels and
  section, so the dialog, the palette and agents need no key-specific
  branches.
- The owner states the order too, by declaring in it: a page shows its
  settings as they were first declared (`SettingSpec::order`), what has
  no section first, then each section where its first setting was.
  Declared again, or by a plugin turned on again, a setting keeps its
  place. Never sort a page by key or title.
- Titles are sentence case, without the plugin's name. Descriptions say
  what changes for the user, not what the key is called, in sentences.
  Choice labels are a word or two; the description explains them.
  `crates/skwir/tests/settings.rs` checks the titles and the sentences
  of every plugin's settings.
- **Secrets** are a record's `secret` field only: they go to the desktop's
  keyring (`skwir_platform::secrets`, on a worker), and the settings file
  only says one is there. Never put a secret in any other kind of
  setting. Test the keyring only through
  `tools/test/private-keyring.sh`, never against the user's.
- Renaming or moving a key: call `Settings::rename(old, new)` before
  declaring it, so the user's value follows.

## Where a setting goes

The namespace is the page, and a preference goes where its subject is:

- One about a plugin's content (an engine, the audio and video player)
  is that plugin's, on its page, even when the shell reads it.
- One the shell applies to several plugins' content is a namespace of
  the shell's own, with its own page: `reading.*` (the reader, reading
  mode, the colors code is drawn in: shared by documents, books, comics,
  Markdown and the code view); `notes.*` (Notes, the core's: each store's
  Read and Write, declared by `App::register_notes_store`).
- `ui.*` is only how Skwir looks (Appearance). `core.*` is what it does
  (General): with files, at startup, with tabs and the split.

## The file is shared

- Another skwir, a hand edit or Home Manager (`programs.skwir`, merged
  at each activation) may change `settings.toml` while skwir runs. A save
  writes the keys this process changed (`Settings::set`, `apply`,
  `rename`) over the file as it stands, and takes in the rest; never the
  whole table it holds.
- What changes in the file, or in the data file, is taken in as it
  changes (`settings::follow` watches their folders;
  `Settings::reload`), under what this process changed and has not saved
  yet, and reaches its owners as a change made in the dialog does
  (`settings::changed`): a plugin turned on or off there is. A setting
  that needs a restart waits for one, as from the dialog.
- A file that is no TOML is never written over: the save fails and says
  why, and nothing is taken in from it until it is fixed.
- **State is not a setting (ADR-056).** What skwir keeps of the window as
  it was left (its layout, tabs, last folder, hints shown) is declared
  `SettingSpec::state` and kept in `state.toml` in the state folder
  (`folders::state`), never in the settings file.
- **What the user makes is not a setting (ADR-057).** What they make in
  skwir through its commands (saved searches, pinned tags, tags' colours
  and exclusive categories) is declared `SettingSpec::data` and kept in
  `data.toml` in the data folder (`folders::data`): theirs, so one that is
  no TOML is set aside, never written over. A key kept apart still in the
  settings file is read until skwir keeps its own, and moves out where
  that file can be written. Only what the dialog changes is a setting.
- The settings file may be read-only (Home Manager's link): a save writes
  through a link to where it leads, says when it cannot, and undoes the
  change; the dialog says so above every page. A change of state or of
  the user's data never touches the settings file.
- `settings.check` judges a file or a table as the dialog judges what is
  typed (`SettingSpec::check`); a new kind of setting is checked there
  too. A key kept apart (state, data) is a problem there: not a
  setting. Home Manager's module runs it while the configuration builds.

## The dialog

- It is a self-contained entity: it holds the `App`, never the shell,
  and closes through an event, so it can move to a window of its own.
- A row's title and description keep `TEXTS_MIN` of width whatever the
  control. A choice is segmented buttons only while they fit beside that
  (`elements::adaptive::Fit` measures both forms; no character counts),
  else a dropdown. A text field narrows before the texts do.
- Changes apply immediately, each through `settings.apply`: switches and
  choices on click, text and numbers on Enter or blur. The core accepts
  one save at a time, so the view queues later changes and sends them as
  one batch when the running save ends; it waits the same way for a save
  started elsewhere.
- A failed save restores the live value and shows the reason on the row.
  Invalid text never leaves its field.
- The view reconciles against `Settings::revision` and
  `PluginHost::revision` when the core wakes the UI; it never reads the
  schema per frame.
- It is not a deferred draw (its dropdowns are, and gpui cannot nest
  them); it stays the last overlay child of the shell root.
- `ui.settings` takes `{ "key": "plugin.option" }` or
  `{ "page": "plugins" }`. Report a problem whose remedy is a setting with
  `App::notify_about_setting` (or `Shell::set_status_about`): its toast
  links to it.

## Plugins and their requirements

- Plugins are toggled with `plugins.set_enabled`, which runs the
  lifecycle callbacks on a worker and persists the choice.
- `Plugin::requirements` lists external tools with a probe. Probes touch
  the filesystem: the dialog runs them on a worker when it opens, never
  during registration or rendering.
- **Helpers answer `--probe`.** One of Skwir's own helpers is probed by
  starting it with `--probe` (`process::Worker::requirement`, `probe`),
  the first thing every helper answers: it checks what it opens itself
  (PDFium's library, GStreamer's playback plugins) and exits 0 when it can
  work. A helper whose linked libraries are missing never starts, so it
  is not reported installed where only its program is. A new helper
  answers `--probe` too, with a test.
- **Helpers answer `--build`** with the build of Skwir they are
  (`SKWIR_BUILD`: the version, plus the commit in a release build, from
  `tools/build/identity.rs`, which the helper's build script includes).
  Skwir asks only after a call of the helper failed (`process::explain`)
  and then says, after the failure, that a helper of another build may be
  why. It never refuses one for it, since most calls work all the same. A
  new kind of call of a helper explains its failures this way where they
  are reported, never for a cancellation.
- Plugins that capture preferences at registration implement
  `on_settings_changed` and refresh only the affected providers.
  Read-through preferences need no hook.
- Lifecycle callbacks are serialized and must not enable or disable
  plugins recursively. Registry and settings locks are released before
  invoking them. Never invoke them from a UI render.

## Privacy

- Never trace settings values: paths and shell preferences can be
  private.
- Exercise saves against an isolated config directory, never the user's
  file.
