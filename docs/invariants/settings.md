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
- What only the owner can check goes in `validate_with`, which sees what
  the value is checked beside (`Beside`): the value stored beside the new
  one (`SettingSpec::parse_over`; the dialog and `settings.apply` pass
  it), or a file's value as it stands (`settings.check`). A check may
  spare what was kept as it is (a shortcut waiting for its plugin).
- The owner states the title, description, unit, step, choice labels and
  section, so the dialog, the palette and agents need no key-specific
  branches.
- A setting that applies only while a switch is on says so with
  `SettingSpec::under(switch)` and is declared right after it, in its
  section: the dialog draws it
  indented beneath the switch and unavailable (dimmed, announced as
  disabled, its changes refused) while the switch is off. Its owner
  still reads both: `under` is how it is shown, not what it means.
- The owner states the order too, by declaring in it: a section shows
  its settings as they were first declared (`SettingSpec::order`).
  Declared again, or by a plugin turned on again, a setting keeps its
  place. Never sort a page by key or title.
- **Essential or folded (ADR-072).** A setting is folded under its page's
  More settings unless its owner marks it `SettingSpec::essential`:
  people are divided on its value (the theme, dates, autoplay), it
  decides what other programs or machines see (where notes are kept,
  private folders), or it acts on files without asking (a drop). Not
  when Automatic suits nearly everyone (an engine), it tunes resources
  (memory, history), it is a detail of the look, or it is changed where
  it is used. `crates/skwir/tests/settings.rs` keeps them to four a page
  and twenty in all: a new essential takes another's place.
- Titles are sentence case, without the plugin's name. Descriptions say
  what changes for the user, not what the key is called, in sentences.
  **The first sentence stands alone**, in at most 30 words: a row shows
  it, and the whole description is its tooltip and what a screen reader
  hears (`settings::first_sentence`, ADR-072). Put what each choice does,
  examples and exceptions after it.
  Choice labels are a word or two; the description explains them.
  `crates/skwir/tests/settings.rs` checks the titles and the sentences
  of every plugin's settings.
- **Secrets** are a record's `secret` field only: they go to the desktop's
  keyring (`skwir_platform::secrets`, on a worker), and the settings file
  only says one is there. Never put a secret in any other kind of
  setting. Test the keyring only through
  `tools/test/private-keyring.sh`, never against the user's.
- Renaming or moving a key: while no one uses skwir, outright, carrying
  nothing of the old name. Once people do, call `Settings::rename(old,
  new)` before declaring it, so the user's value follows.

## Where a setting goes

The key is the owner's; the page is the subject's (ADR-072).

- **The key.** One about a plugin's content (an engine, the player of
  audio and video) is that plugin's, even when the shell reads it, so
  turning the plugin off takes it away. One the shell applies to several
  plugins' content is a namespace of the shell's own: `reading.*` (the
  reader, reading mode, the colors code is drawn in: shared by
  documents, books, comics, Markdown and the code view); `notes.*`
  (each store's Read and Write, the core's, declared by
  `App::register_notes_store`). `ui.*` is how Skwir looks, `core.*` what
  it does.
- **The page.** A setting names the page of its subject
  (`SettingSpec::page`, one of `skwir_core::settings::page`), whoever
  owns its key: the Tags plugin's settings stand on Tags and notes,
  beside where notes are kept; an engine stands with how its documents
  read, the terminal's shell with the program commands run in. One that
  names none is on its owner's page (`core` on General, `ui` on
  Appearance, `reading`, `notes`, `privacy`, `keyboard` on theirs), and
  a plugin from outside skwir has a page of its own. None of skwir's own
  plugins has one (`crates/skwir/tests/settings.rs`).
- **The section.** A page lists the order of its sections
  (`settings::BUILTIN`); a section it does not list follows, where its
  first setting was declared, and what has no section comes first. A new
  section of one of the dialog's pages goes in its list.

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
  no TOML is set aside, never written over. A key kept apart found in the
  settings file (a hand's edit, an earlier skwir's) is read until skwir
  keeps its own, which then replaces it whole: never merged with it, so a
  colour taken away stays away (`Settings::declare` reads the key from its
  file, a save sets it so). The settings file is never written for it;
  `settings.check` notes that it is no setting. Only what the dialog
  changes is a setting.
- The settings file may be read-only (Home Manager's link): a save writes
  through a link to where it leads, says when it cannot, and undoes the
  change; the dialog says so above every page. A change of state or of
  the user's data never touches the settings file.
- `settings.check` judges a file or a table as the dialog judges what is
  typed (`SettingSpec::check`, its owner's check given `Beside::File`); a
  new kind of setting is checked there too. A key kept apart (state,
  data) is noted there: not a setting, read only until skwir keeps its
  own. A shortcut to a command nothing has is a problem, unless it is
  named under a namespace nothing there registers (a plugin that is off,
  the window's own with no window): it waits for it. Home Manager's
  module runs it while the configuration builds.

## The dialog

- It is a self-contained entity: it holds the `App`, never the shell,
  and closes through an event, so it can move to a window of its own.
- A row's title and description keep `TEXTS_MIN` of width whatever the
  control. A choice is segmented buttons only while they fit beside that
  (`elements::adaptive::Fit` measures both forms; no character counts),
  else a dropdown. A text field narrows before the texts do.
- **A long list narrows.** A dropdown of more than
  `settings_view::FILTERED` choices (the fonts, the icon themes, bat's
  themes) opens with a field that keeps the choices holding every word
  typed, its list drawn a row at a time as it scrolls
  (`settings::Narrowed`); Up, Down and the page keys move its highlight
  (`ui.list_move`, which the dialog lets through), the pointer too, Enter
  takes it, Escape closes the list alone. The arrows on its control open
  it rather than step: each step would apply its choice (a font loaded
  for each one passed). A font's list writes the choice highlighted in
  itself, beneath it, and only that one (ADR-073): a family drawn is
  loaded and kept by the text system. Choices listed on a worker while
  the list is open replace its own, what is typed kept.
- **The fold.** A page with essentials shows them, under their sections,
  and folds the rest under More settings (N) at its end; unfolded, it
  shows all, in order, and stays so (`ui.settings_unfolded`, the
  window's state) until folded again. A page without essentials folds
  nothing. What a page reached shows whether folded or not, until
  another page is reached (`SettingsView::revealed`): every setting that
  differs from its default, the one a link pointed at, a plugin's
  settings reached from its Settings button. The search finds every
  setting, folded or not. Never hide a setting a mode or a link would
  then not find.
- **Sections alike are a table** (`settings::blocks`): consecutive
  sections of a page made of the same settings (titles and kinds, in
  order) draw as one, a line a section (its title and its first
  setting's first sentence), a column a setting, its controls named
  "section: title" to a screen reader. A choice there is a dropdown. A
  section of records or a setting under a switch is never a column. Its
  lines are essential or folded together.
- **Changed by you** lists what differed from its default when it was
  reached, settings and plugins (`PluginInfo::enabled_by_default`), each
  with its way back, then Restore all; its count in the sidebar is live.
- A change moves nothing. The revert button has its place in every row,
  empty while the value is the default (`settings_view::revert_slot`, the
  Keyboard page's rows too), and the save's status follows the page's
  title on its line: changing or restoring a value never reflows a
  description or changes the form of a control.
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
  `{ "page": "plugins" }`; a plugin's id as the page goes where its
  settings are, as the Plugins page's Settings button does. Report a problem whose remedy is a setting with
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
