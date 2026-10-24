# ADR-072: Settings shown by subject, the few that matter first

Status: accepted, 2026-10-19, and built the same day. Amends ADR-010 (its
2026-09-25 amendment: a page per namespace).

## Context

The dialog held 71 settings on 17 pages (7 of its own, 10 of plugins),
with 39 plugin switches and the Keyboard page beside them. A user asked to group related settings (tags
and notes) and for a short view of the important ones by default, so a
new user is not met with all of them.

- **A page was a namespace.** A key's prefix chose its page (ADR-010,
  2026-09-25), so a setting went where its owner was, not where its
  subject was. Tags were on four pages: Tags (the sidebar's suggestions,
  the index's memory), Notes (each store's Read and Write, 14 rows),
  General (tags kept when copying, companion files shown). The terminal's
  shell and the program for commands on the selection were on two pages;
  the document, book, DjVu and spreadsheet engines on four plugin pages
  apart from Reading, where how those documents read is; Thumbnails apart
  from the grid's. Four pages held one setting each.
- **Every row is a paragraph.** Several descriptions ran 50 to 75 words.
- **What is already reached elsewhere:** the search finds every setting,
  plugin and key; about a dozen settings are changed where they are used
  (the reader's button, the player's menu, the list's Columns menu, the
  address menu, the 3D model's bar, the slideshow's bar).

## Decision

- **The key is the owner's; the page is the subject's.** A setting names
  the page it is shown on (`SettingSpec::page`, one of
  `skwir_core::settings::page`), whoever owns the key. The key keeps its
  owner's namespace: turning a plugin off still withdraws its settings,
  and no key is renamed. A setting that names no page is on its owner's
  page as before (a plugin from outside skwir has one of its own).
- **The pages:** General (startup, tabs, the split, the address, drag and
  drop), Appearance, Folders and views (the Narrow by bar, the grid, the
  list's columns, thumbnails), Tags and notes (tags, then where notes are
  kept), Reading (pages, documents and their engines, books, text, code,
  spreadsheets), Pictures and media (pictures, playback, audio, video, 3D
  models, maps), Terminal (with the program commands run in), Privacy, Plugins, Keyboard, then
  Changed by you: eleven entries in the sidebar instead of seventeen,
  none a page of a single setting. A page lists the order of its sections; a section it
  does not list follows, where its first setting was declared.
- **The few first, the rest folded.** A setting is essential only when its
  owner says so (`SettingSpec::essential`): people are divided on its
  value (the theme, dates, autoplay), it decides what other programs or
  machines see (where notes are kept, private folders), or it acts on
  files without asking (a drop). Not when Automatic suits nearly everyone
  (engines), it tunes resources (memory, history), it is a detail of the
  look (dragged files), or it is changed where it is used. A page shows
  its essentials and folds the rest under **More settings (N)** at its
  end; opened, it shows the whole page, and stays open (the window's
  state, ADR-056) until folded again. A page without essentials folds
  nothing. A budget keeps the short view short: at most four essential
  lines a page and twenty in all (`crates/skwir/tests/settings.rs`), a
  new setting being folded unless its owner says otherwise.
- **Nothing is out of reach:** the search finds every setting, folded or
  not; a setting that differs from its default is shown whether folded
  or not; a link to a setting (a toast, the palette, `ui.settings {key}`)
  shows it. Rows shown when a page is reached stay shown until it is left,
  so restoring a value moves nothing.
- **Changed by you** lists every setting that differs from its default,
  and every plugin switched from its own default, each with its way back,
  then Restore all.
- **Sections alike are a table.** Consecutive sections of a page made of
  the same settings (titles and kinds, in order) are drawn as one table:
  a line a section, a column a setting. The stores of notes are seven
  lines of Read and Write instead of fourteen rows.
- **A row says one sentence.** A description's first sentence is drawn;
  the whole description is its tooltip and what a screen reader hears.
  The first sentence stands alone, of at most 30 words (checked by the
  same test).

Not done: a global Simple/Advanced switch. Links, the guide and toasts
name settings that a mode would hide; a value changed in one mode keeps
acting, unseen, in the other; a page would look unlike what the guide
says of it. A fold says where more is, on the page it is on.

## Consequences

- `docs/invariants/settings.md`'s "the namespace is the page" becomes
  "the key is the owner's, the page the subject's"; the `reading` and
  `notes` namespaces stay (switching one plugin off must not take
  another's preferences), but no longer decide where anything is shown.
- The Plugins page's Settings button goes to where the plugin's settings
  are; `ui.settings {page: "<plugin id>"}` too.
- A setting a plugin adds is folded unless its owner argues it into the
  budget.
- A user who unfolds every page sees what the dialog showed before, by
  subject.

## Amendments

### 2026-10-24: Network, a twelfth page

Remote folders' settings stood on General, in a section of their own,
when they were one setting: the tools that open an address. Configuring
servers from Skwir (ADR-086) added rclone's remotes there, a list
managed in place (Add Server, Test, Edit, Remove), which is not what
General is about (startup, tabs, the split, the address, drag and drop).
The user asked whether they deserved a page of their own.

- **Network**, after Terminal: rclone's remotes (section Servers), then
  the tools that open an address, in the order tried (Opening
  addresses). Named as the sidebar's section where those servers stand.
  Shown only while a setting is on it, so it goes with the remote
  plugin.
- Thumbnails in remote folders stay with the other thumbnails, on
  Folders and views: a page is its subject's, and thumbnails were one
  of the subjects this decision gathered.
- Twelve entries in the sidebar; none is a page of a single setting.
