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
| whether words can search in it, as the address field's scope | `LocationProvider::searchable`, `App::searchable` |
| how deep words look in it, a place that is no folder (ADR-108) | `LocationProvider::depths`, `App::depths` |
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
- It is its tag and how deep it holds files (`deep`, `tags.narrow`), never
  more: another tag, a kind or a year is the pane's lens (ADR-107), kept
  as its trail is walked and beyond, and its address reads `with`,
  `without`, `kind` and `year` as nothing. Its places count what the
  index knows, not what the lens keeps.
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
- **Scopes (ADR-098).** A place is a scope where its provider says
  words can search it (`LocationProvider::searchable`, from its text): a
  folder, a tag's place and every tag's, Recent; never the trash or the
  shelf, and a search is edited, not searched within. The shell keeps no
  list of schemes: Ctrl+F, the Search button, a row's scope and the
  scopes listed all ask `App::searchable`, and how deep words look there
  `App::depths` (ADR-108).
  Words search where the scope says: a folder is the search's root; a
  place with depths (a tag's: `deep` off and on, the top's too) is
  filtered at both and searched as it is shown (`in:` the place), or,
  chosen from elsewhere or shown at a depth that lists no file of its
  own (the top's tags, a tag of sub-tags only), with everything within
  it, the Run row saying so: what the pane lists decides, never the
  scheme. Any other place (Recent) is searched as what it lists
  (`in:recent:`, `within.rs`): opened through its provider, each entry
  tested as the walk tests a file, what it contains read in its lane,
  nothing below an entry reached; a search within a search is refused.
  An entry listed without its metadata is read by the search itself, as
  the walk reads a file (`fs::read_paths`, in the file's lane), never
  waited for from the place, whose hydration answers what it can: one
  gone is no match, one that cannot be read is tested with what the
  place listed. The search is complete once the place is and no check
  runs; one drain of the place's events at a time takes them in order.
  The whole disk is `/`, which plocate may answer (below).
- **Recent searches** are the query plugin's, kept in the cache (never in
  the settings or the logs): only what the address field ran
  (`remember`), never an agent's call; written on a worker, one change at
  a time, never on the window's thread. The rows of the same search
  elsewhere are of the words typed, never of the scope's own search.
- **The lens (ADR-107)** is what of a pane's filter or search the
  listing can decide of a file: each term of its AND with no word that
  finds names and nothing read from the file (`lens::is_lens`): a kind,
  an extension, a tag, a date, a size, a rating, a comment. It stays
  wherever the pane goes until taken off (a pill's ×, Quick filters,
  `query.narrow` without a facet, Escape over a filtered location); Back
  brings back nothing. The words that find names, and what only a search
  tells, go with the location. It is read again by its reader wherever a
  location is shown, so a date says the day it is read on
  (`modified:today`), and when its words read otherwise
  (`ReadFilter::next_change`, midnight for a folder kept open): one
  waiter in the core, for the earliest change any pane's filter says
  (`App::roll_filters_over`); a search lists itself again. A search text
  that does not read keeps the pane's.
  - `App::open_in`, where every location is shown, asks the location's
    provider how it takes the pane's lens (`LocationProvider::lensed`,
    with no lock held): a search holds it in its own terms (the query
    plugin adds it, `facets::with_lens`: a facet the search sets is its
    own, tags add up), and the history keeps the search as shown; any
    other location is filtered by it (`Pane::carry`, the lens alone as
    the filter, `ReadFilter::of_lens`). A navigation whose location says
    its lens whole is shown as is (`App::navigate_as_is`: the address
    field's words, a pill's choice): a search with its own terms,
    anything else filtered by nothing, so a search's last term taken off
    shows its folder whole, never an empty search (`show_refined`). The
    field's step taken back is shown as it was.
  - Whether a pane shows a location already is one check, as it would
    show it (`App::shows`, on `lensed_in`): a search asked again where it
    is shown is left as it is.
  - Back and Forward show a search's step with the pane's lens in place
    of the one its terms held then (`App::show_again`, `Bring::InPlace`);
    a search made of the lens alone keeps its terms, else it would walk
    the whole tree.
  - A change of a search's lens alone is made in place of it; a change
    of its words is a step Back undoes.
  - A tab or a split opened from a pane takes its lens (`new_tab`,
    `Workspace::add_pane_from`). The lens goes with its reader: the query
    plugin turned off leaves none (`App::read_filters_again`).
- **Quick filters** (`quick_filters.rs`, ADR-107): a bar of the tags
  within each tag the location has, the other tags its files have, their
  kinds once it is listed and their years once every file's metadata is
  in, each with how many files it keeps, what the lens sets already not
  offered; a click changes the lens (`query.narrow`), Alt+click leaves a
  tag out. Asked for (`ui.quick_filters`), over any location, it always
  shows and says why it offers nothing; over a search with a tag and a
  tag's place it shows by itself, with something to offer. Its count runs
  on a worker once per change of the listing, at most every 500 ms while
  it streams, a count under way answering before another is asked;
  another filter, the bar shown or hidden, or the listing complete counts
  at once. With the bar hidden it counts only the files without their
  metadata. It is the one asker of the metadata a filter needs of the
  files no view draws, handed over once per change of the listing's
  names. "Reading N files…" (the footer, the empty state) is said only
  for a filter that reads what files' metadata says, and ends for a
  file that cannot be read: its hydration answers it `Unreadable`
  (`keys::UNREAD`), never nothing.
- A filter types names once per suffix: a name's last extension, or the
  longest compound suffix the type database knows that it ends with
  (`tar.gz`, `mime::compound_suffixes`); never once per name.
- What filters a pane is pane state (`Pane::filter`, `read_filter`,
  `lens`, applied by `Pane::order`), tested on what the listing knows,
  never by reading files. Folders always show; a file the test cannot
  tell yet (its metadata still to come) stays hidden until it can. The
  core keeps its words, not what they read as: its pills are the query
  plugin's chips (`query.describe_filter`), as a search's are.
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
  parent without being published twice. A search checks a plan's
  changes again only when a change names a path at or below its folder,
  or a folder above it; it then follows only the results at or below
  the paths the change names (a path-ordered map), never every result.
- A search reads a folder's names for the stores that look for files
  beside a file (companions) only once a store asks, and keeps them
  while the walk decides that folder's groups (half a second unused,
  within a bound), one read shared by the walkers asking at once.
- **Hints (ADR-026):** a search asks indexes first (`skwir_core::hints`:
  Baloo, the tags index), checks what they name, and still walks: the
  index hints, the walk answers.
- **An answer that is an index's says so (ADR-050).** A search of `/`
  that plocate answers does not walk; each run says what its answer
  rests on (`ListingEvent::AsOf`: the index and when it was last brought
  up to date, or `None` for a walk), and the footer and `entries` say it.
  A provider that answers from an index without walking says it the same
  way; the shell shows it whatever the scheme.
