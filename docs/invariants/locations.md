# Location invariants: places, tags, the trash, searches

Read this before adding or changing a location provider (folder, search,
tag, trash, shelf, Recent) or how the shell shows one. Binding, like
`AGENTS.md`. Reasoning: ADR-032 (a tag is a place), ADR-034 (the sidebar
and the trash).

## The shell names no plugin

The UI asks a location, provider or place what it needs; it never
branches on a scheme or plugin id. A new kind of location is shown like
the others with no UI branch.

| The UI needs | It asks |
| --- | --- |
| whether it is a search | `LocationProvider::searches` |
| its name, glyph, empty text | `title`, `icon`, `empty_state` |
| its trail | `LocationProvider::trail` |
| what gestures mean there | `LocationProvider::verbs` |
| the tags it says of every entry, which they leave out | `LocationProvider::tags_shown` |
| whether notes keep a rating | `NoteStores::held` |
| a modified click on a place | `Place::clicks` |
| what a command put on the clipboard | `Outcome::Clipboard`, `Outcome::Files` |
| what the clipboard holds, for a command | `WantsClipboard`, answered as `clipboard` |
| a footer count | `tally::Tally` |
| which preview shows bytes | `PreviewProvider::shows_bytes` |
| F5 in a view; its own empty state | `PaneView::refresh`, `draws_empty` |
| a place's kind | `keys::PLACE_KIND` |
| what an open location holds | `OpenLocation::holds` |
| what its text typed goes on with | `LocationProvider::complete` (ADR-070) |
| the folders opening it reads (an agent's call refused a private one) | `LocationProvider::folders_read`, `App::folders_read` |

A folder is a location like the others: its trail is a `Trail`, its
chevrons list `App::places_in`, and Shift+F5/F6 give the selection to the
other pane as a drop there would (`Shell::give_to_pane`). Rating, comment
and `notes.agree` are core `notes.*` commands.

## Places, trails and verbs (ADR-032)

- **Places** are entries that are locations, not files
  (`EntryId::place`). Every view shows one as a folder (a tag's with its
  mark); opening it goes where `OpenLocation::location_of` says. It has no
  path, so commands acting on files say `!place` in `when`. Its provider
  counts what it holds (`keys::FILES`: size column and sort).
- A folder that may not open at its path (a trashed folder) also leads
  where `location_of` says, in every way of opening or dropping, and keeps
  its path for what acts on files.
- **Trails.** A location gives its trail (`LocationProvider::trail`: the
  folder it is of, its places, what narrows it), or is one place, its
  name (`App::trail`). The address draws it like a folder's (`trail.rs`,
  read once per location), but nothing that is not a folder looks like
  one: the first own place carries the location's `icon`; a tag place
  (`Crumb::tag`) is the tag's chip; a narrowing by a tag (`Pill::tag`) is a
  pill in its color (a `TagRef`), hollow for "without" (`toolbar::Tint`).
- Chevrons list `LocationProvider::places` on a worker; after the last
  place only when it holds places (`Trail::holds_places`). Up walks back
  (`App::parent`; a search goes up to its folder).
- **Verbs** (`LocationProvider::verbs`):

  | Gesture | Verb |
  | --- | --- |
  | drop or paste | `drop` |
  | Delete | `remove` (run by the core `take_out`) |
  | New Folder key and item | `create` (`ui.create_here`) |
  | F2 on a place | `rename` |
  | its own pane-row buttons | `tools` |

  `tools` (the shelf's Move/Copy to the other pane, Empty Trash) run with
  the pane they name (`ToolPane`) and are enabled only while the location
  lists something the pane can take (never the shelf into the shelf).
- A gesture without a verb keeps a folder's meaning or does nothing. Keys
  follow the place; commands keep their meaning (on the shelf Delete
  unshelves, Move to Trash still trashes). The menu adds the location's
  own items (`menu::verb_items`); the key context gets `refs`, `drops`,
  `creates`, `restores`, `place`.

## Tag locations (ADR-032)

- `tags:/Places/France` is the tag `Places/France` of the home folder
  (`location.rs`); `in` names another folder. A folder's tags are one step
  away: its last chevron, Browse tags here, the Tags button and Alt+G
  (`toggle`, which also goes back); All Tags with Alt (`tags.browse`).
- It lists tags within as places, each counting its files, and its own
  files: those with the tag exactly, or everything within with `deep`
  (Include Sub-Tags, a `Trail::toggles`).
- Narrowing (`with`, `without`, `kind`, `year`) is kept while walking its
  trail, a pill each (`Verbs::narrow`, `tags.narrow`).
- Its listing is the tags index (`TagIndex::files_under`): at once where a
  count covered the folder, then checked; counted on a first visit. It
  lists again when the index changes; F5 checks. Counting: [notes.md](notes.md).
- Verbs (`browse.rs`): a drop or paste tags the files, removing the tag
  they were dragged from (`tags.drop`); tags dropped move inside; Delete
  untags (a selected tag is removed from every file, a plan to review:
  `tags.untag`); New Folder makes a tag, listed until a file has it
  (`tags.new`); F2 renames a tag everywhere; a selected tag has its own
  items (`tags.place_menu`).
- A tag dropped or pasted in a folder is exported as a folder of its
  files, subfolders for tags within (`Verbs::export`, `tags.export`, a
  plan to review).
- A tag tile in the grid shows a few of its pictures, most recent first,
  framed in its color (`keys::PLACE_PICTURES`, `Shell::thumb_of`).
- There is no Tags view: tags are browsed as folders in any view.

## The trash as a place (ADR-034)

- `trash:` lists every trash (`skwir_platform::trash`: on Linux, the
  home trash and this machine's own volumes', as a plan reads them,
  `skwir_core::op::trash_folders_here`: no network mount's, no
  automount's), each file under its original name, with
  `keys::TRASHED_FROM` (Folder column) and `keys::TRASHED` (Deleted
  column, `SortKey::Deleted`, newest first).
- A trashed folder opens inside the trash (`Trash › old project › sub`),
  listed by the core's folder listing (`Trail::one_folder`).
- The trash plugin reads; the core acts ([operations.md](operations.md)).
- A location of trashed files says `Verbs::restore` (`restores`):
  Restore instead of Move to Trash; a drop (Ctrl or not) or a paste of
  files cut trashes, and files copied are refused (Ctrl+V, Shift+F5): the
  trash keeps files, not copies; Cut takes files out where pasted; Delete
  deletes for good.

## Remote folders

- An address (`sftp:`, `smb:`, `dav:`… `rclone:`) is a location that
  connects, then redirects to the mounted folder
  (`ListingEvent::Redirect`): the folder is the location, the address its
  name. [remote.md](remote.md) has the rest.

## Searches

- Pills and the address field: [shell.md](shell.md). The query language
  belongs to the query plugin, and so does what the address field offers
  of it (ADR-098: `field.rs`, `assist.rs`), as a source like any other.
  The parser's tokenizer and the field's lexer split text alike: a change
  to what a word is (`#print` is `tag:print`) is made in both, and
  `quote` writes a name that would read otherwise (`"#3"`).
- **Scopes (ADR-098).** Words search where the address field's scope
  says: a folder is the search's root; a place that says the tags of all
  it lists (a tag's, `LocationProvider::tags_shown`) is searched as those
  tags every match has, from the folder it is of (`Trail::base`), else
  home, never by reading its text; another place (Recent) is searched as
  what it lists (`in:recent:`, `within.rs`): opened through its provider,
  each entry tested as the walk tests a file, what it contains read in
  its lane, nothing below an entry reached; a search within a search is
  refused. The whole disk is `/`, which plocate may answer (below).
- **Recent searches** are the query plugin's, kept in the cache (never in
  the settings or the logs): only what the address field ran
  (`remember`), never an agent's call; written on a worker, one change at
  a time, never on the window's thread. The rows of the same search
  elsewhere are of the words typed, never of the scope's own search.
- **Narrowing.** Over a search with a tag, a bar (`narrow.rs`) offers
  tags within, other tags its files have, kinds and years, with counts
  from the results. A click refines (`query.refine`); Alt+click excludes a
  tag. Over a folder the same bar appears with Narrow by
  (`ui.narrow_folders`) or while narrowed (`query.narrow`), kept in the
  folder and below it.
- Narrowing is pane state (`workspace::Narrowed`, applied by
  `Pane::order`), tested on what the listing knows, never by reading
  files. Folders always show; files wait for their metadata (the bar's
  count reads `Pane::unfiltered_order` on a worker) and stay hidden until
  the test can tell. The core keeps its words, not what they read as:
  its pills, and the words typed's, are the query plugin's chips
  (`query.describe_filter`), as a search's are.
- A provider names its location (`LocationProvider::title`: `report in
  Papers`) for tabs and menus. A search's trail is the folder it looks in
  (the place it lists, `in:recent:`), then itself: the address draws the
  first part, and its pills for the rest (ADR-098).
- A search watches nothing, but sees Skwir's changes: after each plan the
  query plugin (`provider::Searches`, an executor observer) re-checks the
  touched files under each open search's root and updates rows without
  listing again. An arriving directory discovers previously unknown
  descendants through the initial search's traversal rules and depth
  measured from the search root; discoveries stream without clearing or
  completing the listing again. Known descendants follow their renamed
  parent without being published twice.
- **Hints (ADR-026):** a search asks indexes first (`skwir_core::hints`:
  Baloo, the tags index), checks what they name, and still walks: the
  index hints, the walk answers.
- **An answer that is an index's says so (ADR-050).** A search of `/`
  that plocate answers does not walk; each run says what its answer
  rests on (`ListingEvent::AsOf`: the index and when it was last brought
  up to date, or `None` for a walk), and the footer and `entries` say it.
  A provider that answers from an index without walking says it the same
  way; the shell shows it whatever the scheme.
