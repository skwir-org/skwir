# Settings invariants

Read this before declaring a setting or changing the settings dialog
(`crates/ui/src/settings.rs`, `settings_view.rs`, `controls.rs`) or
`skwir_core::settings`. It is binding, like `AGENTS.md`. ADR-010 holds
the reasoning.

## Declaring a setting

- Use the `SettingSpec` constructors: `toggle`, `choice`, `integer`,
  `number`, `text`, `path`, `command`; `records` for a list the user
  builds in the dialog (tile servers: a record needs an id in the dialog,
  which edits it by it; a file's may leave it out, as what reads records
  does), or one its owner keeps elsewhere (`records_kept_by`, a
  `RecordSource`: its commands list, add, edit and remove; a record may
  say what else its line does, `actions`, each a `label`, a `command` and
  its `args`, that another program changes it, `fixed`, which takes
  Edit and Remove away, and a `choice` of its own, a dropdown on its line:
  its `value` and `label`, its `choices` (each a `value`, `label`,
  `detail`, and whether `disabled`, greyed with why) and the `command`
  choosing runs, with `args` and the value in its `field`: a language's
  grammar); `data` for what the user makes
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
- **What Automatic comes to.** A choice whose default is Automatic says
  what it gives here and now where its owner knows (`SettingSpec::now`:
  one of its choices, `dark`, or a name, a font's family), and declares
  it again when that changes (the theme's look, the fonts read: through
  `settings::show_resolved`; the icon themes once listed). Never bake it
  into the label: the label stays "Automatic", and `choice_shown` adds
  what it comes to wherever a label is shown. An Automatic that tries
  engines in an order says it with `SettingSpec::tries` (each choice and
  the name of its owner's requirement it needs, from the same list the
  runtime tries: `document::AUTO`): the dialog, which probes every
  requirement when it opens, outlines the first found
  (`SettingSpec::with_found`). Never probe to say it elsewhere: a probe
  starts a helper. A record's own choice,
  whose labels its owner writes, says it the same way: "Automatic
  (bat)" (a language's grammar, `language_records`).
- **A choice of looks** (Colors from's palettes) gives each value a
  swatch (`SettingSpec::swatches`, the colors of a miniature window, as
  data): the dialog draws it in miniature before each value's name, in
  the choice's list and on its control, in the colors the value would
  give now; nothing is added under the row. Declared again when what
  they show changes (`settings::show_resolved`), each made again only
  when the desktop's colors, the system's look or the theme moved. Such a
  choice always opens as the list that narrows, however few its values
  (`settings_view::narrows`). While its list is open, the row
  highlighted (by the keys, the pointer or what is typed) is previewed
  (`settings::previewed`): its owner wears it as if chosen, nothing is
  saved, and closing the list without taking it (Escape, a click
  outside, a page changed, the dialog closed) shows what holds again;
  no list is hidden while its preview stays (`close_lists`). One taken
  stays shown while the save that carries it waits or runs (asked of
  `Settings::save_status`, the dialog open or not), and no longer once it
  ends, whatever changes the setting after; a value taken that is the
  one held sends nothing, so nothing waits. An owner that reads such a
  setting asks `previewed` first (`ColorSource::chosen`).
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
- Renaming or moving a key: people use skwir (0.1.0 and on), so call
  `Settings::rename(old, new)` before declaring it, and the user's value
  follows; `Settings::check` tells a file still saying the old name the
  new one, or, where it says the new one too, that the old is not read
  (`ui.narrow_folders` is `ui.quick_filters`). A plugin given a new id
  names the ones it had
  (`Plugin::former_ids`): its toggle follows it from each, and it moves
  its own keys in `register` (the books plugin, once `epub`, ADR-106).

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
- A row's title and description keep a share of the row whatever the
  control (`TEXTS_SHARE`, a share rather than a width, so a large text
  size in a narrow window still fits: ADR-117), and the control gives way
  in what is left, never cut at the card's edge. A choice is segmented
  buttons only while they fit beside that (`elements::adaptive::Fit`
  measures both forms; no character counts), else a dropdown. A text
  field narrows before the texts do.
- **Automatic says what it gives**, as a popup's setting row does
  (`docs/invariants/shell.md`): while the default is chosen, the segment
  it comes to now is outlined in dashes (`controls::now`; every segment
  keeps room for the outline, so marking one moves none), and Automatic
  is named "Automatic, now Dark" to a screen reader; a dropdown, its
  list and the revert button's tooltip say "Automatic (Dark)"
  (`SettingSpec::choice_shown`). What it says is Automatic's own answer,
  whatever is chosen: the revert tooltip of a Theme set to Light names the
  look the desktop's preference gives, Fonts from set to GTK names where
  Automatic reads (`Shown::automatic_dark`, `fonts_automatic`), never
  what the choice gives. A name that is no choice (a desktop's own
  fonts, Plasma) outlines nothing and is said in words alone.
- **A long list narrows.** A dropdown of more than
  `settings_view::FILTERED` choices (the icon themes, bat's themes), and
  a font's however few (the code font's monospaced families may be),
  opens with a field that keeps the choices holding every word typed,
  its list drawn a row at a time as it scrolls (`settings::Narrowed`),
  twelve rows or fewer where the popover leaves less under the row: the
  list gives way, never the field nor a font's specimen; Up, Down and
  the page keys move its highlight (`ui.list_move`, which the dialog
  lets through), the pointer too, Enter takes it, Escape closes the list
  alone. The arrows on its control open it rather than step: each step
  would apply its choice (a font loaded for each one passed). A font's
  list writes the choice highlighted in itself, beneath it, and only
  that one (ADR-073): a family drawn is loaded and kept by the text
  system. Choices listed on a worker while the list is open replace its
  own, what is typed kept.
- **A text size's choices are written at the size each gives**, in its
  part's font (the interface's for Text size and Files text size, the
  reading font, the code font: `settings_view::written_at`), each label
  in a rem of its own on the list's one left edge, its row as tall as
  it needs. The control itself stays at the dialog's size.
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
- **A list is a list, not more settings.** A records setting's records
  are drawn inset in its card, under its own row, in smaller rows a line
  apart (`settings_records`), never as rows like settings; an empty one
  says so (None found for a list its owner fills, None yet for one the
  user does). One longer than `settings_records::FOLD` has a filter over
  its words, which says how many it found, and shows its first rows until
  unfolded. A record's own choice steps by Left and Right, as a setting's
  does, its popup under its right edge, inside the dialog. A list is
  parsed once for each text it has (`SettingsView::parsed_records`),
  never per frame or per record; a list its owner keeps elsewhere is
  answered from memory (`ui.languages` reads no grammar).
- **Data is written by its owning command, off the window's thread.**
  `settings.apply` takes no `data` setting; its command changes it
  through `Settings::set` and saves it on a worker when asked from the
  window (a save holds the settings' writer across the file's write),
  the changes queued in the order made (`ui.grammar_choose`, as the
  pinned tags do), and where it is asked otherwise.
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
  started elsewhere. The window's own changes do too, its toggles and a
  `settings.apply` run from a menu or the palette (`settings::remember`).
- Without a window, `settings.apply` completes persistence and settings
  callbacks before returning; a failure is a command error. A headless
  caller must also refuse a save when startup fell back to memory after a
  settings-file load failure, even after its warning was consumed. The
  window keeps its asynchronous SaveState protocol, including calls over
  its control connection: lifecycle callbacks can stop that connection.
- A failed save restores the live value and shows the reason on the row.
  Invalid text never leaves its field.
- A save syncs the file and its folder (`Settings::write_whole`), which a
  disk busy with other writers can hold for seconds. A test of the
  window's protocol waits for its save's `SaveState` to leave `Saving`,
  woken by the waker, under a deadline for a hang, not a measure
  (`core/tests/settings.rs`).
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
- **A helper says how far it went** as lines `progress <done> <total>`
  on its standard output (`process::PROGRESS`), at most every tenth of a
  second, its answer the last line; Skwir follows them as it runs
  (`process::run_helper`), with a time limit counted only while it runs.
  The print helper says its pages drawn, and its shrink mode the images
  it went through.
- **Helpers answer `--build`** with the build of Skwir they are
  (`SKWIR_BUILD`: the version, plus the commit in a release build not made
  from the version's tag, from `tools/build/identity.rs`, which the
  helper's build script includes; About and `skwir --version` say it too).
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
