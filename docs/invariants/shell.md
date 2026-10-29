# Shell invariants: the window

Read this before changing the window in `crates/ui`: toolbars, colors
and fonts, popups, the top rows, menus, drops, feedback, the sidebar,
keys, tabs, the terminal panel, the player. Binding, like `AGENTS.md`.
Views are in [views.md](views.md); what locations decide, in
[locations.md](locations.md).

## Toolbars and buttons

- Reuse the command-backed buttons and bundled symbolic SVGs of
  `toolbar.rs`. Tooltips, focus styles and keyboard activation belong to
  the shared button. Keep a focused button's handle when it is disabled.
- Responsive rows pick a variant by intrinsic measurement in prepaint
  (`elements/adaptive.rs`, `AdaptiveRow`); no pixel breakpoints.
- Text cut to fit (`truncate`, `text_ellipsis_middle`, a clamp) is drawn
  through `elements/clip.rs`: while cut, its tooltip shows it whole,
  before its container's tooltip (`clipped_with`). Whether it was cut is
  read from layout when the tooltip is asked for, never in `render`.
- Pane actions capture their target pane while rendering and use
  pane-specific element IDs. A button names its pane in its command's
  arguments only where they accept one (`CommandSpec::accepts`, as the
  registry checks them): a place's Eject takes none, and an argument a
  command does not take is refused.
- An element is known by what it shows, never by where it stands
  (`elements::known_by`): gpui keeps an element's tooltip as it was
  built, its press and its screen reader's node by its id. A row known by
  its index hands them to whatever comes to stand there, and an element
  whose id stays while what it shows changes keeps saying the old (a
  server's row under an ejected mount kept the mount's "1.0 PB free").
  A file's row is known by its entry, a crumb by its place, a fact by its
  text, a picker's option by its item, a preview's rows by the file shown
  and their place in it; what is inside a row is known within it. Where
  two items may show the same, their position joins what they are known
  by.
- A focused control is pressed by Enter and Space through
  `modal::pressed`: where it has the keyboard (`toolbar_button`) the keys
  run `ui.toolbar_activate`, which it answers with its press, so no other
  binding of theirs (a dialog's Enter, the player's Space) answers behind
  it. gpui's own click on a key's release comes only where the key's
  press reached the control, which a binding stops, and only if focus has
  not moved since (its focus generation): a control whose keys are bound
  is never left to it, and nothing presses twice.
- Previews share one frame (`preview.rs`) and one toolbar
  (`preview_bar.rs`): a kind lists tools in slots, never builds a row
  ([previews.md](previews.md), "The preview frame"). Preview menus close
  when the content is replaced by another file or its presentation closes.

## Colors and fonts (ADR-071, ADR-073)

- Every token of a look is derived from its `Base` (`Theme::derive`):
  the desktop's scheme for that look (Plasma's `kdeglobals`, a GTK3
  theme's colors: `Desktop::schemes`), else Skwir's own (`Base::own`)
  wearing the desktop's accent. Text, the accent and the outcomes are
  guarded to their contrast (7:1 when the desktop asks for more); a
  token no desktop changes is `Kept`. Never a color beside the tokens.
- A scheme is one look's. A forced other look is Skwir's palette with
  the desktop's accent; Automatic takes the scheme's look when the
  desktop has one look's (`Theme::look`). An accent the system names
  replaces a scheme's, and the selection is then derived, unless it is
  the scheme's own said again.
- What the desktop says is read on `desktop_style`'s thread, never while
  drawing; the first frame draws what was read last (the cache). A
  reading that differs from what was said wakes the window; one that
  does not, nothing. It follows the portal's signals and `kdeglobals`
  replaced (its folder watched, access dropped), never a timer. Toolkits
  are never loaded: a GTK theme is read as the colors its sheet names.
- The fonts are the theme's tokens (`font`, `reading_font`,
  `code_font`), resolved with it, read where they are drawn.

## Popups and focus

- Anchor popovers to their visible trigger through `elements/popover.rs`,
  measured in the current prepaint.
- A deferred draw inside another is drawn in a later round (gpui since
  2026-10-18, ten deep). Peek stays in the normal overlay tree
  and defers its popovers, below modal dialogs. The settings dialog is not
  deferred (its dropdowns are) and stays the shell root's last overlay
  child.
- **Popups own the keyboard:** a menu or popover takes focus on open,
  returns it on close, and has its own key context (`menu.rs`).
- **A press cancels a tooltip waiting to show** (gpui patch 0038): what
  the press opens is never covered by the tooltip of what was pressed.
  A tooltip already shown goes at the press, as upstream has it.
- **Focus never dangles:** when the focused element leaves the window,
  the shell's focus-lost listener gives the keyboard to the pane drawn as
  active (`Shell::drawn_focus`), or to the Settings dialog while it is
  open, never to the pane behind it. Never refocus by hand after closing
  something.
- **One popup at a time,** all through `Shell::toggle_popup`. A press
  outside closes it and says so (`pointer` in its close command,
  `Shell::note_dismissed`); if that press is on the popup's own button,
  it stays closed. Any button's click closes the open popup the same way,
  but a popup's row that switches one of several choices it shows (which
  views have a button) keeps it open, its row keeping the keyboard
  (`Button::keeps_open`), as does a setting's value.
- **A setting in a popup is one row** (`toolbar::MenuPart::Choice`): its
  name, then its values (`Button::segment`), an icon each, Automatic in
  words; or tiles, four to a row (`Button::tile`). The value chosen is
  filled; while Automatic is chosen, the value it gives now is outlined,
  and Automatic's tooltip says why. Left and Right walk the values as Up
  and Down do (their key context's `toolbar::CHOICE`), Enter or Space
  chooses, and the popup stays open.
- **One menu look.** A popup that is a menu (`toolbar::menu_panel`) is
  drawn as a context menu is: on the chrome, as wide as its widest row,
  rows of `toolbar::MENU_ROW` in the window's text with their keys, the
  row under the pointer or the keyboard highlighted, and what is on said
  by a check, never by the highlight. Its rows are `Button::menu`; a
  context menu's rows (`menu::render`) keep the same metrics.
- Toolbar popup rows are buttons: arrows walk them, Home/End reach the
  ends (`ui.toolbar_first`, `ui.toolbar_last`). The row reached scrolls
  into view in prepaint (`toolbar::RevealRow`, read from the focused
  button's key context, `row`).
- gpui puts a tab group's own focus inside the group, ordered among its
  stops by its own `tab_index`: a popup's focus keeps index 0, before its
  rows, or Down and Tab pass them and leave for the window's first
  button (`toolbar::Popup`).
- A menu that depends on a toolbar variant is built once per variant and
  chosen in the popover's deferred prepaint from `AdaptiveRow::chosen`
  (`PopoverAnchor::popup_for`).

## The top of the window

No window row: each area owns its top, so a one-tab window has one row
of bars, split or not.

- **Sidebar header** (`address::sidebar_header`): the sidebar toggle and
  More (everything without a button). Without a sidebar they start the
  row its header would share: the tab strip while it shows, else the
  first pane's row. One rule decides (`Shell::corner_at`). More hangs
  from `Shell::more_anchor`.
- **Split and Preview toggles** mirror them on the right: in the preview
  panel's bar when it is open at the right, else at the end of the tab
  strip, else at the end of the rightmost (or top, when stacked) pane's
  row. One rule decides (`Shell::toggles_at`). Their labels show where
  there is room (`address::panel_toggles`). A preview at the bottom has no
  bar.
- **Tab strip** (`tabs.rs`): shown while the tabs are listed (two or
  more, or one with `ui.single_tab`) and not by the sidebar drawn this
  frame (`Shell::strip_shown`). With `ui.tabs_at` = sidebar, the tabs
  are the sidebar's first group and the panes' rows are the window's
  top; a sidebar put away or a window too narrow for it gives the strip
  back, so the tabs are always in sight.
- **Pane row** (`render_address`): Back, Forward; the address (trail or
  edited text), ending with Search, the keep-in-sidebar button and then one
  button per other way to see the folder (`registry::Aspect`, e.g. Tags,
  Alt+G), last so it stays under the pointer; then the view buttons and
  the View menu.
  - Search is Ctrl+F (`ui.search`), the one sign in sight that the field
    searches: there while the trail is, where Ctrl+F has a folder, a tag
    or Recent to look in or a search to edit, and something searches
    (`address::search_title`). Typed in, the field's token wears its
    magnifier, and the card's switch says Ctrl+Shift+F. Never a second
    field: the address is where words look (ADR-098).
  - No Up button (the trail's parent segment, Alt+Up) and no New Folder
    button (the folder's menu, Ctrl+Shift+N): each has a way in sight.
  - View buttons are the `ViewRole::Listing` views (list, grid,
    columns). `Alternate` (tree, board) and `Lens` (Usage) views get a
    button only while shown. The view states its role, never the shell.
  - The user may give any view a button or take one away
    (`ui.view_button`, `address::ViewButtons`): from the buttons'
    right-click, taken in the capture phase so no button takes the
    keyboard first, the Menu key on one (`view_buttons`), or View ›
    Buttons…. Kept with their data (`ui.view_buttons`, ADR-057), only
    where it differs from the role, so a view added later comes with its
    role's. The view shown always keeps its button, so the group never
    goes and always says which view is shown.
  - The View menu hangs from the active pane's row: every view as a tile
    (Buttons… at the end of their title), the sort keys with the direction
    on the Sort by row in the key's words, what is shown (hidden files,
    Narrow by, Fill thumbnails…), and the size on one row. More holds the
    panels without a button (Timeline, Terminal) and the layout's
    settings, one row each (`address::layout_values`).
  - Two `AdaptiveRow` variants: all buttons while the address keeps
    `FIELD_MIN`, else one button opening the View menu. The address has
    no width of its own when variants are measured.

### The trail

- A folder's trail starts at the deepest sidebar folder holding it, under
  the sidebar's name (`sidebar::roots`, re-read only when places change;
  `address::crumbs`): `Papers › 2026`, `stick › Photos`, else Home. With
  `ui.full_path`, every trail starts at `/` under real names.
- Tabs and history use the sidebar's name (`App::location_title`);
  listings, toasts and Properties use the folder's own. Up goes to the real
  parent, the folder left selected there (`App::name_in_parent`; a
  place's label in the place before it); Ctrl+L edits the real path.
- Back and Forward list history on right-click or a long press, as in a
  browser (`ui.history_menu`; `back`/`forward` take `steps`). A long
  press is any button's `toolbar::Button::on_hold`: held `HOLD`, it runs
  its command, and the release clicks nothing.
- Each step of a pane's history keeps the entry the cursor was on when
  the pane left it (`Pane::go_on`, `go_to`); Back and Forward select it
  again once listed, so a link followed is gone back from to the document
  it was in. A link to a file in the folder shown adds a step without
  listing the folder again (`App::step_here`).
- Chevrons list on a worker (`nav_menu.rs`): each one the folders in the
  segment before it; one after the last the shown folder's places
  (`LocationProvider::places_of`), a separator, then its subfolders
  (`ui.subfolder_menu`); one before the first, when the trail starts below
  `/`, the folders above from the root down (`ui.parents_menu`).
- In a narrow pane, the first segment in sight lists everything above it,
  wrapped-out segments included. Which segment that is comes from layout
  (`PaneUi::trail_shown`, from `on_children_prepainted`), read on demand.
- The trail and the list's detail columns share one layout technique: a
  wrapping flex line one row tall, so the nearest segments stay. A click
  beside the trail edits the path.

### Layout

- More's Layout group (`ui.layout`) places the timeline, the terminal,
  the split, the preview, and an archive member's preview. Split and
  preview default to Automatic, decided in `Shell::render` from the
  window's size (`layout.rs`). Shift+F3 splits the other way, for that
  split only.
- Closing a split closes the pane `core.close_split` names
  (`layout::CloseSplit`); Split's tooltip says which.

## The address field (`omnibox.rs`, ADR-070, ADR-098)

- **A scope before the text.** The field may hold a token: where words
  look. Ctrl+Shift+F (`ui.filter`) and Ctrl+F (`ui.search`) start with
  the folder shown, Ctrl+L with none and the path selected, Ctrl+J
  (`ui.jump`) with none and nothing typed, places first; over a search,
  Ctrl+F and Ctrl+L edit it, its root the scope, and Ctrl+Shift+F shows
  the folder it looks in filtered by its words. Only the scope is a
  token; terms stay text.
- **How deep words look** (`omnibox::Depth`, ADR-098, amended): in the
  folder shown alone, or in it and its subfolders. Ctrl+Shift+F starts
  on the first (and so does Ctrl+L, words then filtering the folder
  shown), Ctrl+F on the second; typed in, each switches to its own,
  keeping the words, as the switch on the card's first line does, which
  says each side's keys (`keys_for`) and what the words keep there. The
  token wears a funnel for the folder alone, a magnifier below it (a
  place searched as it lists, its own glyph). Another scope chosen is
  searched; none, words filter the folder shown.
  - The folder alone is the folder's own listing, filtered at each key
    (below): still a folder to paste into, drop on or make things in.
  - Below it is the search Enter runs, shown as the words are typed
    (`Suggestion::live`), `LIVE_PAUSE` (400 ms) after the last key, the
    one waiting before dropped; a switch shows it at once. It goes in
    place of one step of the pane's history, the field's own
    (`App::step_here`, then `navigate_in_place`): Back from it goes to
    what was shown before. A search that reads the files (contents, a
    picture's date or place), looks over the whole disk, or below a
    folder on a server (each walk round trips) waits for Enter, and the
    first row says so (the query plugin leaves `live` empty).
  - Enter with no row chosen keeps what the words show and gives the
    files the keyboard: the folder filtered; the search shown, now
    remembered (`query.search` leaves a search shown as it is), or run
    when it waited. A row chosen goes from what the pane showed: the
    field's step is taken back first, and a source's row runs over the
    folder filtered as it was (as Escape puts it back), never by the
    words typed. Words no source answers (the query plugin off) offer no
    Go row: Enter keeps what they show.
  - Escape takes the field's step back (`App::take_step_back`, as if it
    had never been gone to), puts back the filter and the narrowing the
    folder had, and gives the files the keyboard. The pane's address is
    never written into the field while it is typed in.
  - The field lets go when it loses the keyboard in a window still
    active (a click on a place or the files, Tab, F6; `omnibox_left`):
    what its words show stays, as Enter keeps it, the search waiting for
    its pause is dropped, and the field shows where the pane is. No state
    of the field outlives its focus to act on what the pane shows later.
  - A search shown after the pause replaces only what the pane showed
    when it was asked (a place clicked meanwhile stays), and is known by
    its words and its scope (`Omni::shown`): another scope chosen with
    words typed shows its own search at once, as a switch does.
  - Backspace at the start of the text takes the scope away; Left there
    selects the token first (`input::AtStart`, sent only where the field
    asks, `send_start`).
  - A click on the token, Down on it selected, Alt+Down
    (`ui.omnibox_scopes`), or `@` typed first list scopes: the folder
    shown, the one above, home, the whole disk, the sidebar's places
    words can search, those whose names go with what follows `@`; under
    them the query plugin's "Search for “@…”", for a name that starts so.
  - Ctrl+F on a highlighted row that has a scope (a folder, a tag,
    Recent) makes it the scope and clears the text for words; with none,
    words look below the folder shown, its scope.
  - What is a scope is its provider's answer (`App::searchable`,
    `LocationProvider::searchable`): a folder, a tag, Recent; never every
    tag, the trash or the shelf. Ctrl+F, the rows, the scopes listed and
    the Search button ask it, never a scheme.
- **The Search button** (`address::search_title`) ends the field, as
  Ctrl+F (`ui.search`): nothing else in sight says the field searches.
  "Search this folder and its subfolders" on a folder, "Search" and the
  place's title where words can search it (a tag, Recent), "Edit this
  search" over one; none where words cannot look (every tag, the
  trash), while the field is typed in (its token wears the magnifier),
  or while no source searches words (`suggest::searched`, a source's
  `SuggestSource::searches`: the query plugin off). It names no
  command of a plugin.
- **Rows come from sources** (`skwir_core::suggest`): each asked again at
  every change of the text, in a job of its own in its lane, the jobs
  before cancelled; an answer is kept by source until the next. A key
  takes no row away: each source's rows stay until its answer to the new
  text comes, so the card never closes, shrinks or jumps for a frame
  between keys (it did whenever a frame was drawn before the answers).
  A row answering an earlier text (what Enter does, completions, the
  reading, other scopes) draws after the text only what its completion
  still adds to it (`rest_after`); Enter and Tab take rows only once
  they are the text's (`settle_rows`, below). Rows go by `Section`, then
  rank; a row the arrows chose stays chosen when answers arrive. The
  shell names no plugin: it runs the rows' invocations and the core's
  commands (`filter`, `navigate`, `open`).
  - The query plugin answers what Enter does with words (the search in
    the scope, or why it cannot run: Enter then waits), what the term at
    the caret goes on with, the words read as terms ("Did you mean",
    applied only when picked), the same search elsewhere, and before
    anything is typed with a scope the terms as buttons, recent and
    saved searches; and the spans the text is drawn with.
  - Jump, the tags plugin and the shell (`suggestions.rs`: other tabs,
    the pane's history, the sidebar's places, a few commands with their
    keys) answer the rest.
  - Words are mostly a search, so a source offers a row only for words it
    finds as the row reads: Jump each word unbroken in the path from `~`,
    in the name before its folders, never letters strewn over a path.
  - The field's own source stays the paths and places it completes
    (below), read in their lane.
- **The rows and the field are one card** (ADR-098, amended): typed in
  and with rows, the field's card stands around it (`popover::
  popup_around`, at the popups' priority), from just before its left
  edge, as wide as the field (at least 430 px, moved left to stay in the
  window, never short of the field's right edge); the rows go under a
  line where the eye already is. The field's text and scope draw above
  the card (deferred one priority higher, `omnibox::CARD_PRIORITY`), so
  their key context, focus and pointer stay theirs; the card's top leaves
  them the field's height (`address::FIELD_HEIGHT`), never a size read in
  `render`. While words are typed, or about to be, the card's first line
  is how deep they look (above) and what they keep there, the folder's
  count or the search's: the card hides the list's top.
- **`?` alone** lists what the field takes: the shell's grammar (a path,
  a place's address from the schemes providers open, `@`, words, `>`)
  and the query plugin's terms with examples; Enter or Tab writes how the
  row starts (`~/`, a scheme, `@`, `>`, a term's key such as `kind:`),
  never the example.
- **Before anything is typed**, the dropdown shows what the sources offer
  then, nothing highlighted: Enter does what the text says (the location
  shown again, a search's words again). A row the arrows reach runs as
  any row (a place, a tab, a recent search), and Tab takes one that
  writes text (`completes` holds for it).
- The card says the keys bound (`keys_for`: its footer, the highlighted
  row's, the depth switch), never keys written in its words: a scheme or
  the user may bind others, and one bound to none is not said.
- The keyboard never leaves the field (Up/Down are bound in its context).
  Rows run on press with `prevent_default`. The rows are a list box
  (`modal::options`); the row the arrows reach is the field's active
  descendant, so typing still reads back the field's text and caret.
  Shift+Delete (`ui.omnibox_forget`) forgets a recent search highlighted.
- **The text is drawn with the spans** the query plugin reads
  (`TextInput::set_styles`): keys in the accent, values on `term`,
  operators muted, what cannot be read with a wavy line in `error`. An
  edit keeps the spans wholly before it, which still cover the same
  characters, until a source has read the new text; replacing the text
  clears them. The UI knows no query syntax.
- **Completion (ADR-070, `complete.rs`).** A path lists the names of the
  folder typed that go on with the name typed: folders, then files, at
  most eight, matched as `complete.rs` says (case ignored unless a
  capital is typed; whole, start, a word's start, anywhere; hidden names
  only for a dot or where they are shown), ranked by `jump.scores`. Go is
  first only while no name is typed or none matches: otherwise Enter goes
  to the first name, whose rest is drawn after the text
  (`TextInput::set_hint`). A source's row draws its `rest` the same way.
  - The folder typed is read on a worker, once while the field is typed
    in, the one read before stopped; a folder a pane lists gives its
    listing's names. Never on the UI thread: matching is in memory.
    `./` and `../` are the folder shown's, never the process's.
  - A folder part that names no folder is taken a name's start at a
    time (`~/w/sk/`), each level ranked, at most four folders: the rows
    then say which folder, and nothing is drawn after the text.
  - A place's text completes through its provider
    (`LocationProvider::complete`), matched the same way.
  - Tab (`ui.omnibox_complete`, bound in `address completes`): a row
    chosen with the arrows; else words leaving one name shown become its
    path; else the part the names share; else the highlighted row; else
    the first is highlighted. A folder taken ends with its separator and is
    listed in turn. Taking is an edit (Ctrl+Z). Right or End at the end
    takes what is drawn. Where nothing completes, Tab walks on; while the
    names it would choose among are read, it waits for them (below).
  - A row keeps its exact path: a name that is not UTF-8 goes where it
    says, though the field can only write it approximately.
- Enter on a path stats it on a worker (`reveal_path`): a folder is gone
  to, a file shown selected in its folder (hidden files then shown, and
  said), one not there said. Shift+Enter opens a file (`open`). A
  `file://` URI typed or pasted is read as the path it names here
  (`uri::path_of`, `Shell::omni_text`), the field keeping what was typed. A relative path with no folder shown (a search, a
  tag, the trash) goes nowhere, and says why.
- Enter or Tab pressed before every source answered the text asks them
  all there and then (`settle_rows`, `suggest::each`; they answer from
  memory), so what the key takes is always the text's. A row clicked
  runs as it reads.
- Enter or Tab pressed while the names a path or a place goes on with are
  still read (the folder typed, a provider's `complete`: on workers, never
  here) waits for them (`Omni::held`, `completing`): the key is done once
  they come, as if pressed then, so Enter goes to the first name as it
  would have. Another text, the arrows, Escape or the field letting go
  forget it; the card says the folder read once it takes a moment. A
  Tab held that then finds nothing keeps the field rather than walking on
  later.
- **Words typed over a folder, looking in it alone, filter it at once**,
  as the core's `filter` reads them through the query plugin
  (`narrow::read_filter`): the longest start that parses; words, `name:`
  and `ext:` hold for folders too, other terms for files, every folder
  staying; contents and a picture's date or place need a search below
  it, which the footer says; a
  filter that reads metadata has every file's asked for. It runs at each
  key on the window's thread, so a type is asked once per suffix
  (`lang::Types`, as `mime::by_names` asks a selection): 6 to 13 ms a key
  over 100 000 names in release. Showing another
  location clears it; Escape (`filtered` key context) shows everything;
  once the field lets go, its facets stay as pills after the trail
  (below). Ctrl+Shift+F over a filtered folder edits its words, Ctrl+F
  takes them below it. Over the folder alone, the search's row is none,
  and a term completed or the words read as terms are written by Enter,
  as by Tab.
- A search keeps the trail of the folder it looks in (the place it
  lists, `in:recent:`: `LocationProvider::trail`), its own crumb drawn
  as its pills; Escape with nothing selected goes to that folder (`up`,
  `search` in the key context), as Escape in a folder filtered shows it
  whole. A tag location keeps its trail and is a place: Escape never
  leaves it, as it never leaves a folder (Backspace and Alt+G do).
- What narrows the folder shown (a tag location's `with`, Narrow by's
  choices, the words typed) is the search's own chips right after the
  trail, read once per change (`query.describe_filter`, `search_bar::
  Filters`): the same labels and menus, each choice changing it in place
  (`query.narrow`, `query.filter`, or going where the location leads
  without it), and "+ Filter", which adds to the words typed and offers
  no facet that reads the files. Each wears a funnel (`filter`), a
  search's pills their facet's icons: a funnel never stands for a
  search, nor a facet's icon for a folder narrowed. The first pill says
  how deep the words look, a dropdown: a folder's "This folder only"
  (`query.describe_filter`'s `depth`), its subfolders searched for its
  narrowing and words (`query.below`); a search's "And subfolders" (or
  "This folder only", searched so), its folder filtered by its words
  when the folder alone can tell them, else searched alone
  (`query.here`). Its tooltip says the key that does the same from the
  field.
- The trail and the pills after it give way alike, each by its natural
  width (`adaptive::Fit`, drawn from its left edge): the trail wraps its
  first places out, the pills draw the richest of their fits that holds
  (the depth pill its icon alone first), a folder's keeping their funnel
  at the last, their × left to their menus. The room after them edits
  the address.

### Search pills (`search_bar.rs`)

- A search shows as pills after its folder's trail: how deep it looks,
  each facet `query.describe` says is set, then "+ Filter". Each choice
  runs the invocation the plugin
  gave (`query.refine`, or `ui.ask` for a typed value), and navigates, so
  Back undoes it. A change that takes the last term away shows the place
  searched (the trail's: the folder, or what it lists), as Escape does
  and as a folder's last pill gone shows it whole: an empty search is
  asked for, never left over. One asked with nothing (`in:/x`) stays a
  search while its depth or hidden files change.
- The query plugin owns the meaning (`facets.rs`). Pills are read once
  per location, and again when the sidebar's names change; never per
  frame.
- The text is the one truth: a part no facet holds is a pill of its own;
  clicking beside the pills edits the text.
- The pills are one line, the richest that fits (`search_bar::Fit`):
  every pill as it reads; else how deep as its icon; else that and
  "+ Filter" as icons and each facet as its icon and value; else the
  facets' values alone, cut short with both ends kept, the later giving
  way first. Nothing is hidden by wrapping: a facet narrowed away is
  still a pill, said in full in its tooltip.
- Required tags are chips read as paths (`Places › France`) in the tag's
  color, "+" between them; an excluded tag (`not_tag`) is hollow.
- An affordance on what it acts on (a pill's ×) takes no focus; the
  pill's menu reaches it from the keyboard.

## Context menu

- `when` decides who sees an item: `empty` for items about the folder
  itself (the background menu, which clears the selection first); also
  `folder` for those that need a real folder, so searches and the shelf
  never offer them; `kind:image` (every selected entry) or `has:.gpx` (at
  least one) for items about some files (`registry::MenuItem`). Never
  offer an item to every file and let the command refuse.
- Words that depend on the environment ("Open With Image Viewer") come
  from a `MenuSource`, answered from memory as the menu opens; the plain
  items stay registered for the palette. Its command takes what its menu
  gives (`pane`, always; a place's `location`, a tag's `tags`): one that
  refuses an argument answers nothing, unsaid
  (`crates/skwir/tests/plugins_toggle.rs` gives every source its menu's).
- Rare items set `submenu`. Give items an `icon`.
- **State is said, never written into a title.** An item that is on or
  off sets `MenuItem::checked`; the values of one setting set `choice`
  (`registry::Choice`). A choice is one row where its values allow it
  (`ChoiceLook`): its name, then buttons (an icon each, five at most),
  swatches (colors) or stars. A press chooses a value and the menu stays
  open with its checks updated (`Shell::choose`). From the keys, Left and
  Right move a ring along the values, choosing nothing, and Enter or Space
  chooses the one ringed; Left from the first value of a submenu's
  choice goes back to its row, as from any other row. Values that need their words are rows, each checked
  or not, and a submenu they fill says the one chosen on its row (a
  server's Thumbnails). To a screen reader a choice's row is a radio
  group, its values radio items.
- **A submenu opens beside its row** (`PopoverAnchor::popup_beside`):
  its first row level with its row, on the right or, without room, on
  the left, moved up to stay in the window. A submenu's row stands where
  its first item's order puts it, in that item's group, with the first
  icon among its items. The pointer opens one
  after resting `menu::REST` on its row, or at once with a click; resting
  on another row closes it, reaching it in time keeps it. Right, Enter or
  Space open it and move into it; Left or Escape go back to its row. A
  press on it is no press outside the menu (`MenuState::in_sub`). A
  submenu holds no submenu.
- **A check in a submenu keeps the menu open** (the tags in Tags):
  pressed, it turns over and the menu stays for the next, so its command
  toggles (`tags.toggle`), a second press undoing the first. A check at
  the menu's own level closes it, as any item does. A check on for some of
  what it is about is `Check::Partly`, a dash.
- **The file menu** (ADR-099): Open, Open With › (the type's other
  applications, Other Application…, Run Command…), Open Terminal Here;
  Cut, Copy, Copy Path, Add to Shelf, Add to Bookmarks; Rename (Rename
  Several… with several), Move to Trash, Delete Permanently; Extract Here,
  Compress ›, Find Similar ›; Tags › (Edit Tags…, a check per tag, the
  name tools), Rating, Comment…, the GPS items; Properties. Paste is the
  folder's own menu's, on its background.
- **Never taller than the window.** The menu stands where it opened (at
  the pointer, or under the keyboard's row), moved to stay in the
  window, and takes at most the window's height less its margins, in
  prepaint (`PopoverAnchor::popup_at`, `popup_first`: `Room::Window`,
  where a toolbar's popup shrinks to the room on its button's side
  instead). Its rows scroll inside its padding; the row the keyboard
  reaches is brought into view in the frame that draws it
  (`MenuState::reveal`, the separators counted among the rows'
  children, `menu::child_of`).
- **A tag's menu, where a file shows it.** A right press on a tag's chip
  or dot (`Shell::tag_act`: an entry's in a view, the preview's header,
  Properties) selects the entry as its row's right press does
  (`Shell::open_chip_menu`) and opens the tag's own menu, asked with
  `files` (`tags.tag_menu`): its files in the folder shown, its place,
  the tag editor of the selected files (Edit Tags…: where a file shows
  its tags, its chips are the way to it), taking it from them, then
  what its pill's menu offers.
  The keys reach the same: the context menu of selected files gives each
  of their tags (at most four, pinned first) a submenu, `Tag “draft”`
  (`tags.files_menu`), with its color too (the editor is the menu's own
  Tags…). A chip in a side column or
  on another place's card is no entry of the pane: a press there opens
  nothing.

## Drag and drop

- **A drop asks.** Held keys decide (`dnd::held`: Shift moves, Ctrl
  copies, Ctrl+Shift links); with none, a menu at the pointer asks
  (`core.drop_asks`, `Shell::ask_drop`), the default highlighted for Enter
  (`dnd::drop_op`: ours move within a filesystem and copy across; other
  applications' copy).
- Each row runs the core command (`move`, `copy`, `link`) with `entries`,
  so it acts on what was dropped, not the selection by then.
- Plugins add rows through the registry's `drop` menu (order 100–899;
  their command gets `entries` and `dest`): Extract Here needs no branch
  in the shell.
- **Send to the other pane… (Shift+F7)** asks with the same menu, for the
  selection and the other pane's folder (`Shell::ask_other_pane`), hung
  from the cursor's row: extracting across a split is a drop's row.
- A drop on a place uses its location's verb (`Verbs::drop`). A drag
  carrying places is not asked about.
- **A drag of files shows what it took** as the views draw it
  (`dragicon.rs`): the row pressed, else the selection's first, and as
  many more as `ui.drag_image` lays out (`dragicon::Layout`: a fan, the
  default, a pile, a mosaic, or the one alone), one of each other type
  first. Each is its thumbnail as the window keeps it
  (`ThumbCache::kept`, whichever size a view drew), else its icon, drawn
  for the drag on GPUI's executor (`icons::pixels`) while a glyph stands
  in; a badge counts them, the mosaic's last quarter says how many more.
  One file is its picture alone in every layout. The same image goes to
  the compositor if the drag leaves the window (`dnd::FilesIcon`). It
  never asks the core's thumbnails: the window takes what it draws out
  of them (`Thumbnails::take`).
- A drop on the terminal types the files' paths at its prompt and asks
  nothing: no file is copied or moved (see the terminal panel).

## Feedback

- **An answer is said where it was asked**, where the user is looking.
  A field or popup that asks something answers in itself: a find's "No
  match", why it did not look, its failure, are its popup's status line
  (`preview_bar::Field::say`, gone when the text changes), announced as
  a toast is. A toast is for an answer with no place of its own in view,
  and for one whose place is gone (a script's find, no field open).
- **Every user action gets an answer**, with no place of its own in
  `toast.rs`: a sentence built from the applied `Change`s ("Moved
  “a.txt” to Trash"), with Undo when the journal can take it back (of
  the plan it reports, `undo {"plan"}`: refused, saying why, once a plan
  asked after it touched what it did), Redo after an undo, Retry first
  for a plan a step's error stopped (ADR-080; a toast holds its buttons
  in order, `Toast::actions`).
  Progress bars:
  [operations.md](operations.md); an answer said while plans run is drawn
  under their cards, never hidden by them. It names the file a step acts
  on as the user knows it: an item of a trash by the name it had, which
  the executor reads from its record before the step takes it away
  (`Progress::named`), never by the trash's (`a.txt.2`, `$R…`).
- `Shell::set_status` is a toast; `set_status_about` (core:
  `App::notify_about_setting`) links to the setting that fixes it.
  Toasts are drawn under the overlays, but over the preview on the whole
  screen, which covers the window (previews.md, "The whole screen").
- **Footers hold state, never messages** (`status.rs`). A pane's footer:
  counts, the selection's size, or with nothing selected what its files
  weigh (`weight.rs`: each file shown as the listing stat-ed it, else as
  a worker in the folder's lane stat-ed it once per listing, kept by its
  entry; a filter, hidden files shown or a file come adds up again and
  stats only what neither knows; a size learnt adds up again once the
  request running has answered; `PaneView::weighs` for a view that
  measures folders). The window's state (Undo, pending paste, plugin
  counts via `tally::Tally`, proposals, the player's strip) ends the last
  pane's footer. Side panels have no footer.
- A key whose command needs a selection has `selection` in its `when`,
  so it does nothing rather than report "nothing selected".
- **Animation:** only a bounded entrance under 200 ms, plus the player's
  play/pause flash (`media::FLASH_MS`).
- **Empty is a state.** `placeholder.rs` says why a pane is empty
  (`LocationProvider::empty_state` for non-folders) or failed (the
  system's message as detail) and offers a way out, inside the pane so
  drops and the menu still work. The banner above the view says an error
  beside existing entries, or what the pane still waits for
  (`loading::message`), with the ways out of a wait, as the toolbar's
  buttons (`loading::Way`): Cancel for a location still opening (an
  address connecting), Go Back for one on a server that does not answer
  (Go Home where the pane has no Back), and Disconnect where the place
  holding that server offers Eject (its invocation, asked of the place:
  the banner names no plugin). Leaving a location stops what opens or
  lists it; the server stays connected.

## Properties and links

- A provider's fact can carry a command (`Detail::with_action`), a link
  run through `Shell::run`. Links show over the end of the row's last
  line while the pointer is on it, taking no room otherwise; those that
  are all a row says or that stop or start what runs always show, after
  the value ([previews.md](previews.md), "Header and Properties").
  Clicking the row copies the value; only the link acts.
- **Links (ADR-029).** A link written in a document answers a click. One
  found in raw text (`skwir_platform::links`: addresses by scheme, never a
  bare domain; paths in the terminal and those a file's text writes)
  answers Ctrl+click, drawn as a link
  only while Ctrl is held over it (`Run::found`, `Shell::armed_link`);
  Shift adds a tab.
- Addresses open only through `ui.open_url`; files and folders only
  through `ui.reveal` (with `line`, the preview shows it), never opened.
- Ctrl+Shift+O labels the links in view (`links.rs`).
- A terminal path resolves against the folder its line was printed in,
  stamped into the grid by the plugin (`plugins/terminal/src/links.rs`).
  Stamp new kinds of found links there too, never resolve them at click.
- A path found in text is looked up only through `links::resolve`, which
  asks `fs::volume::Here` first: never a server named by the path
  (`\\host\share`) nor below an automount point not mounted yet
  (`/net/host`), since the lookup alone reaches that host (ADR-029).

## The sidebar

- **Icons.** A place has a theme icon name. Symbolic-only or
  scheme-colored icons (Breeze's small ones) are shapes drawn tinted
  (`icons::Pic::Shape`); full-color SVGs are rasterized for the pixels
  they cover (`icons::element`). The theme is `ui.icon_theme` while
  installed, else the desktop's, changed only through
  `IconCache::set_theme`. On macOS and Windows the name maps to a system
  icon (`system_icons::for_name`).
- **Place fields:** `space` (filled by a worker: asking a volume can
  block), `eject`, `unavailable` (dims the row; the reason is its
  tooltip), `busy` ("Unlocking…": a still hourglass, nothing to click
  until done), `menu` (its own items). A rescan publishes at once and
  keeps the last measurements (`carry_over`), so nothing blinks.
- **A tag looks the same wherever it is shown:** its chip, a dot and a
  tint of its color (`tags::chip`): on files' rows and tiles, in
  Properties and the preview's header, in the sidebar
  (`tags::place_chip`, a place with a `color`; chips wrap together,
  `sidebar::wrap_pills`), in the address's trail, and in menus (an item
  with a `MenuItem::color`: the Tags submenu, the Tags section's menu).
  The one shown is filled deeper (`Theme::tag_shown`). A tag the sidebar
  only suggests keeps its dot without the tint: an outline in a tag's
  color means the files without it (a narrowing's pill, `toolbar::Tint`),
  never a fainter tag.
- **Pointed at, a place rings its files where they are.** A place whose
  location says tags of what it lists (a tag's pill, a saved search for
  one: `LocationProvider::tags_shown`) rings the entries the pane shows
  that have them or a tag within them, in the tag's color around them
  and its tint under them (`Shell::ring_tags_of`, `Look::ring`, drawn by
  `entry::highlight` and the board's cards), until it is left. Asked of
  the location's text, never of a plugin: a new kind of place that says
  tags rings its files with no branch here.
- The sidebar repeats nothing its location says.
- **Keeping.** Ctrl+D and the pane's keep button run the core `keep`
  command, which hands the location to the `Keeper` registered for its
  scheme (bookmarks, saved searches). A kept place carries `forget` (the
  button runs it while shown) and `reorder`. A new kind of kept place
  registers a keeper and fills those two fields; no UI branch.
- **Bookmarks** are edited only through `BookmarkEdit`
  (`skwir_platform::places`); each edit returns its inverse, offered as
  Undo/Redo (`App::notify_undoable`, as for any edit outside a plan).
  Other applications' lines are kept; the file is watched. A folder
  dropped beside the rows is bookmarked (`places.bookmark`, GTK
  bookmarks).
- Files the desktop shares (bookmarks, `mimeapps.list`) are rewritten
  only through `skwir_platform::file::rewrite`, by an edit returning its
  inverse.
- **Tags in the sidebar:** pinned tags (`tags.pin`, ordered by dragging,
  unpinned by the keep button), else suggestions from the index
  (`Place::suggested`, their dots without the tint), then All Tags
  (`tags:/`). A home tag
  folder, not narrowed, is kept as its pin (`Keeper::keeps`).
  Shift+click narrows to files that also have the tag; Alt+click shows its
  files in the folder shown (`tags.pill`). The tag editor offers pinned
  tags first.
- **Drops:** a place takes what its location takes (`Verbs::drop`); one
  that takes nothing (Recent, a search) swallows the drop, never
  bookmarking it. A place dragged onto a file's row (`dnd::takes_gifts`)
  gives that file, or the selection containing it, what it takes
  (`takes_gift_for`).
- **Arrangement (ADR-034).** Groups are the user's (`sidebar::Sections`,
  part of the window's shape): click folds, drag moves, the sidebar's
  menu shows or hides each (`ui.sidebar_section`). A plugin names a
  place's group, never where it goes.
- Places are arranged by who keeps them (`sidebar::Role`): the user's
  kept places are their owner's to order, rename (`Place::rename`) and
  take out; suggested ones can be hidden; offered ones (Home, Recent, the
  Trash, volumes) are moved, renamed and hidden in the sidebar only
  (`ui.sidebar_place`, `Sections::places` by `Place::key`), never in the
  owner's files. A place moves only among those the same command moves
  (`sidebar::mover`); `arrange` is the one reading of all this. Within a
  group, what is no folder comes after the folders (a server the tools
  know after the mounts) unless it moves: what moves keeps its owner's
  order (a remote folder's bookmark among the folders', as the file has
  them).
- A group with every place hidden keeps its header, whose menu lists
  them; places offered hidden (`Place::hidden`) appear there unchecked.
- Rename… (`Place::rename`, F2) and Remove From Sidebar
  (`Place::forget`) are the sidebar's own items, never the owner's.
- F6 (`ui.focus_next_part`) goes round the parts of the window shown,
  as in other applications: the sidebar, the files, the preview (what it
  shows takes the keys, `Shell::shown_focus`), a part not shown passed.
  Forward only: Shift+F6 is the split's. The terminal is not one of them:
  its programs have F6, and F4 takes the keyboard in and out of it.
  `ui.focus_sidebar` (the palette's) brings a sidebar put away back.
- In the sidebar (`Sidebar` context, `ui.sidebar_key`): arrows walk,
  Enter opens and returns the keyboard, Menu, Alt+arrows move, F2,
  Delete, Escape goes back to the files. A click never takes the
  keyboard (`prevent_default`).
- **Tabs in the sidebar** (`ui.tabs_at`, `sidebar::TABS`): the first
  group, drawn while the tabs are listed: a row per tab
  (`tabs::sidebar_rows`), then New tab. It folds as any group but is not
  moved or put away; the setting takes it away. Its rows take the
  sidebar's keys (`Item::Tab`: Enter shows the tab, Delete closes it,
  Menu opens its menu, Alt+arrows move it; `Item::NewTab`), drops into
  the folder shown, a drag along them (a line above or below the row),
  the middle button and the right button, as in the strip.
- Volumes: [operations.md](operations.md).

## Asking the user

A plugin needing text dispatches `ui.ask` with the command to complete;
one needing a choice dispatches `ui.choose` with its choices or a command
listing them on a worker (`choose.rs`; `option` adds a flag such as
"Always use for…"). Never a prompt or picker of its own.

- **One text field** (`input.rs`), for every field: a rename, the
  address, a form's, a find. A line longer than the field scrolls to
  keep the caret in view (`scrolled`, in prepaint). A drag selects
  wherever the pointer goes in the window, not only over the field,
  along the line alone (straying above or below never jumps to an end);
  held past an end, it selects on and the line scrolls, faster the
  further out (`autoscroll_x`, a timer that lives only while it is held
  there), until the pointer comes back, the button is let go or the text
  ends.

- **A click answers what a key answers.** A dialog that waits for an
  answer ends with `modal::footer`: what its keys do, muted, then Cancel
  and the answer, named by its verb (Rename, Unlock, Delete; `ui.ask`
  takes it as `accept`, OK when absent). A picker's rows are its buttons.
  A press outside cancels too, but is never the only way: what is only
  read (About, F1's overview) closes from its ✕, as Settings does.
- **A dialog keeps the keyboard.** Tab walks its field, Cancel and its
  answer, round their ends (`Shell::walk_dialog`, `modal::tab_among`),
  never the window behind it; a list one picks from keeps it in its
  field. A focused footer button is pressed by Enter and Space, before
  the dialog's own Enter (a focused Cancel cancels): a footer holds only
  `modal::Pressed` controls; a press on one
  leaves the keyboard in the field; Escape that nothing in the dialog
  answered cancels it (`modal::modal`).
- **A question names what it is about when asked.** `ui.ask` fills the
  asked command's `pane`, `entries` (the selection) and `in` (the folder
  shown) that the asker left out, as they are when it opens: Rename
  renames the file it was opened for, whatever is selected by the
  answer.
- **A question before deleting lists what goes** (`deleting.rs`): each
  item as the user knows it (an item of a trash by its own name, from
  where it came, read from its record), what it is, what it weighs or a
  folder holds, and whether companion files go with it: a mark after its
  name, "+ metadata" while the whole name fits beside it, else "+"
  (`AdaptiveRow`), which says where they are when pointed at. The name
  has the room first, a companion's name never cuts it, and too long it
  is cut in its middle, its extension kept, as the views cut names. Its
  folder on each row only when they are not all in one, which the line
  under the list says once with what they hold in all. The rows come
  from the plan's paths at once and never wait for the disk: a worker
  reads the rest, then counts each item as deleting it takes it
  (`fs::count::count_each`: a link is itself, a file of several names
  counts once), handing over what it has as it goes, stopped when the
  question closes. The question's height is set when it opens.
- **What a dialog lists scrolls, all of it** (`modal::Rows`): a box of
  rows of one height, a set number in sight, only those on screen built,
  scrolled by the wheel and by Up, Down and the page keys
  (`ui.list_move`) while the dialog has the keyboard; never cut at a
  count, nor clipped unsaid. What a deletion takes, a proposal's steps
  (each line cut in its middle, so it ends with what it comes to) and
  what a question to allow says.
- **A proposal says once the folder its steps share** ("In ~/Projects"),
  and its lines say their paths from there (`Change::within`: `rename
  a.txt -> b.txt`, `move c.txt -> archive/c.txt`); a path outside it
  stays whole. The folder is the deepest holding each path's own folder
  (a step on a folder is said from the one above), none at a drive's
  root. A `Change` said alone (`Display`, what agents read) keeps every
  path whole.

## Keys

- **F1** (`shortcuts.rs`, ADR-100) lists the bindings of the focused
  element's key contexts; a new context needs only a name in
  `keymap::context_label`. One row per action, its keys side by side
  whichever place each works in; a family's actions (`Form::family`) on
  one row, the modifiers they all hold said once. Rows stand under their
  command's topic (`CommandSpec::topic`), the topics of the place nearest
  the focus first, and flow down as many columns as fit
  (`elements::columns`: whole topics, a row stacked only where it does
  not fit its column, all counted from one measure of each row, never
  from a size read in `render`); what does not fit scrolls under the
  overlay scrollbar. Its field has the keyboard when it opens:
  what is typed narrows the list and finds the bindings that do not run
  here too ("Elsewhere", each under its place and condition). Escape
  empties the field, then closes. **All shortcuts…** opens the Keyboard
  page with what was typed (`ui.settings { page, filter }`).
- **One way to find a shortcut** (`keymap::finds`), for F1, the Keyboard
  page's filter and the Settings search: every word in what the action
  does, its place (never its conditions: "not in the trash" is no answer
  to "trash") or its command; or, when the text names keys (a `+`, a
  modifier, a named key alone: `ctrl t`, `Shift+Delete`, `F2`), a
  binding holding them all. A letter alone is a word. A command run three times with the pointer gets
  one hint naming its shortcut, once ever: call `Shell::note_pointer_use`
  from new pointer-only entry points.
- **Layers (ADR-024),** lowest first: plugin defaults
  (`Registry::keymap`), the shell's (`keymap::BUILTIN`), key schemes
  (`Registry::schemes`, e.g. `vim`), the user's (`keyboard.bindings`).
  `keymap::install` installs them in that order. A binding hidden by a
  later layer is left out of hints and tooltips. A user's record without a
  command frees its keys (`gpui::NoAction`); one whose command nothing
  registers waits for its plugin, bound to nothing (`keymap::dormant`).
- **Text fields keep their characters.** A binding whose first stroke
  types a character (a letter, `.`, Space, with Shift at most), wherever
  it is bound, is bound to nothing in the text field's context
  (`keymap::typed_in_fields`, after the bindings it shadows), as the
  terminal keeps its keys: `.` bound anywhere still types in the address
  bar, a search, a rename.
- **A key held down runs again only a step (ADR-066).** The system's
  repeats of a key reach the window's root first
  (`Shell::capture_run_command`), which drops a command that does not
  repeat by its declaration (`CommandSpec::repeats`); a command that
  steps (the cursor, a page, the volume) declares it where it is
  declared (the shell's in `commands::repeats_of`). A switch never
  repeats: a held key would flip it over and over. Whether a key is held
  is gpui's `Window::key_down_event().is_held` (patch 0032), never a
  timer of ours. `keymap::tests::held_keys_repeat_only_steps` holds that
  every built-in key that moves repeats.
- **Peek's key, held, peeks until let go.** `ui.peek` run by a key's
  press keeps the key (`preview::PeekPress`): a tap leaves peek open; a
  press the system repeated closes it when let go (the root's key-up),
  unless peek was used meanwhile (a command other than `ui.cursor`, a
  click in it). Opening or closing peek any other way forgets the press.
- **Titles.** An action is called by `keymap::title`: its command's form
  for its arguments (`Declared::forms`), never by its command's title
  alone. A key bound with arguments gets a form where its command is
  declared (the shell's in `commands::KEY_FORMS`); `keymap_titles.rs`
  holds it.
- **The Keyboard page** reads the keymap's bindings, hidden ones too
  (`keymap::bindings`), as one row per action and place
  (`settings_keys.rs`), draws them in one list (`settings_keys_view.rs`)
  and asks for a new shortcut's command (`settings_keys_new.rs`). It
  writes `keyboard.bindings` only through `settings_keys::apply`, where a
  record written goes last and takes out the user's own it covers, so it
  is the one that runs and nothing it replaced comes back. It says what
  new keys would do through `settings_keys::verdict`, which reads
  precedence as GPUI does (the deepest place, then the latest), and what
  any change leaves of the other actions' keys, and whether it may be
  made, through `settings_keys::effects` (the keymap built before and
  after; worked out when the keys or the change change, never per
  frame). Keys are taken with `App::intercept_keystrokes`, so no binding
  runs while one is recorded; a bar with no keys to take gives its answer
  the keyboard. A place is grouped by its label in `keymap::PLACES`,
  which also says whether it is inside a dialog or a menu; a command that
  must keep a key says so (`Declared::keeps_a_key`), and no change leaves
  it without one in a place.
- Type-ahead is on only while no binding takes an unmodified character in
  the file list (`Keymap::typeahead`).
- A key that types a character hides the pointer until the mouse moves
  (`CursorHideMode::OnTyping`, set at startup in `lib.rs`). A key that
  only moves, such as an arrow, leaves it. The fullscreen preview hides
  it with its controls.
- Multi-stroke bindings wait without a timeout (vendored patch 0010);
  `sequence.rs` shows what can follow.
- A plugin that changes standard keys is off until asked
  (`Plugin::enabled_by_default`). A scheme key meaning a standard gesture
  says so (`keys.like`: vim's `d` is Delete) and takes that key's bindings
  and conditions, never a copy.
- Notation and `when` tokens: `AGENTS.md`, "Conventions".

## Tabs (ADR-018)

- Tabs are core data (`tab.*` commands); a tab holds one pane or two.
- **How a tab is drawn** (`tabs.rs`, one `Label` for the strip and the
  sidebar). In the strip, a pill floating in the row: the tab shown a
  card in the panes' ground (`bg`, a `border` line, no shadow), the
  others their names alone until pointed at (`hover`). In the sidebar, a
  row like a place's, the tab shown the same card, never the selection's
  color: that says which place the pane shows.
- A tab's × is on the tab shown and the one under the pointer
  (`Shell::tab_hovered`, `tabs::closes`), never on a lone tab.
- A tab is named like the place it shows (`App::location_title`). Split,
  it is named by both panes, in the order they are laid out, a line
  between them; the pane with the keys in the text's color, the other
  `text_muted`, so the names stay put when the keys move. Its icon is
  the split's, as the Split button draws it for that tab's arrangement
  (`Shell::tab_stacked`). Short of room, the name with more letters
  gives way and the other keeps up to half (`tabs::gives_way`). Said
  aloud and dragged, the names are joined ("Field Guide and Maps"); the
  tooltip has each pane's location.
- A hidden tab is dormant: its panes keep history, sort and selection,
  and release listing, watchers and kept columns (`Pane::suspend`).
  Showing it reopens them through `App::show`. `core.sleep_hidden_tabs`
  off keeps every tab listed (`App::apply_tabs_sleep`).
- While the tab shown gets ready (`Shell::place_pane`), the window keeps
  drawing the old one for at most `tabs::HOLD`; the core keeps it open
  until `App::settle_tabs`. A user command first draws the tab shown
  (`Shell::present`).
- The shell never creates or drops a `PaneUi`: it changes the workspace
  and `Shell::sync_tabs` follows its revision, so `Shell::panes` is always
  the panes on screen (`is_split` and `other_pane` rely on it). Hidden
  panes wait in `Shell::hidden`, views reset; a pane gone is released
  there whoever closed it.
- Per-pane plugin state ends with `location_lifetime`, which dormancy
  ends too.
- Tabs are remembered only with `core.startup = last`
  (`core.last_tabs`); only the shown tab opens at startup.

## What the window remembers

- The last window's shape (view, split and share, sidebar width and
  sections, preview place and size, archive-member placement, Properties'
  folds, the types shown as their source, timeline and what it measures
  and counts, zoom, Usage's levels and mode, column widths) is
  application data, not preference:
  `session.rs` keeps it under one key, written only when it changes. A
  new pane opens in the current pane's view.
- **Edges** dragged to size an area are one element (`elements/edge.rs`):
  double click restores the default; shared room via `edge::share`.
- The preview panel's width is bounded by what the panes and sidebar
  need (`preview::max_width`, from the window's width, no fixed cap); a
  small window draws it narrower without rewriting the remembered width.
  The same at the bottom (`preview::max_height`). The pinned player's dock
  is bounded by the panel (`Shell::dock_height_limit`); until the edge is
  dragged, the default height adds the dock's.
- The sidebar's width is bounded likewise (`sidebar::max_width`), and the
  sidebar hides below a window width computed in `Shell::render`.
- The split edge stores a share, not pixels (the panes' flex grow); the
  drag reads the container bounds from `DragMoveEvent`. Each pane keeps
  `PANE_KEEP` across, `PANE_KEEP_HEIGHT` stacked. The share belongs to its
  tab (`tab_splits`, swapped in `Shell::present`); a tab gaining or losing
  its split, by any route, starts even again (`sync_tabs`);
  only the shown tab's share is remembered.

## The timeline strip (`timeline.rs`)

- The width pressed is the width shown (`Histogram::bucket`). Unpicked,
  the bars are the finest that show every date (`fits`); a width picked
  holds while the pane shows what it showed then (a `Zoom` ends with
  `location_lifetime`), so each place opens on its whole span. A width
  finer than `fits` is a window of `MAX_BARS` bars, moved by the wheel
  over the bars and the arrows (`ui.timeline_pan`), anchored to a bar's
  instant (`from`, `until`) the plugin places; a width picked over a
  moved window ends where it did.
- Dates far from all the others (`outside`) never decide the width: the
  plugin leaves out strays (a hundredth of the files at most, further
  from the rest than it spans and by more than a year).
- The bars are one element (`elements/bars.rs`): labels, ticks and counts
  are chosen in prepaint from each bar's rank, the finest level whose
  labels all have room. Bars are dated on the plugin's calendar
  (`Calendar::system`), so a label is the bar's period.
- A bar's click, or a drag across bars, searches their period in one
  term of the time counted (`modified:`, `created:`, `accessed:`,
  `taken:`; `timeline.select`); in a search, it refines that term
  (`query.refine`), so a bar of a bar's files is a click away and Back
  returns. Only a folder's subtree is walked: a search lists what it
  lists.
- What the strip keeps of a visit (`timeline::Visit`, ending with
  `location_lifetime`): the width picked, its window, the keys' bar and
  where a range began, bars kept by their first instant so a count again
  or a moved window finds them. What it measures (files, size) and the
  time it counts are the window's, remembered (`session.rs`).
- The selection is the plugin's to count (`Bar::selected`): the pane's
  selection revision is a recount's, throttled with the dates
  (`RECOUNT`). A picture's clock (`keys::TAKEN`) is read in the plugin's
  background count, as a subtree is walked (`Counted::Listing`), through
  the core's fields; a count again of the same visit shows the last one
  meanwhile.
- The bars take the keyboard (`keymap::TIMELINE`, in F6's round between
  the files and the preview); its keys are steps that repeat when held.
  To a screen reader the chart is a list box whose bars are elements of
  no size built under `shell.a11y`, the keys' one claiming the active
  descendant: gpui moves the focus only to a real element's claim, never
  to a synthetic child's.

## The terminal panel (ADR-019)

- It draws core's `terminal::Screen` and sends `terminal::Input`; the VT
  engine (`alacritty_terminal`) stays inside the `terminal` plugin.
- The session's threads parse, build screens (at most one per 8 ms) and
  wake the app; the UI thread only compares a revision. The shell starts
  on a worker.
- With the keyboard, every root binding is shadowed (`NoAction` in the
  `Terminal` context, built by `keymap::install`) except
  `TERMINAL_PASSTHROUGH`; add a key there if it must work from the
  terminal.
- The terminal's own keys (copy, paste, paging, links) are bound in its
  place (`when: terminal`). The shadows are bound before every other
  binding, so a key of the terminal's wins over the same keys bound to
  the window in any layer. A key it takes never reaches the program: an
  unbound Ctrl+Shift+C was Ctrl+C to it. Test a new one as GPUI reads the
  keymap (`keymap::tests::the_terminals_keys_are_its_own_there`).
- Its menu (`Shell::open_terminal_menu`) opens with the right button,
  or the Menu key, standing above the cursor (`MenuHost::Terminal`, its
  row marked by the grid in prepaint). Shift+F10 stays the program's,
  as the other F-keys are. On a link, the menu says where a link a
  program wrote leads before it is followed, as its tooltip does.
- Paths reach the prompt quoted by the session for its shell
  (`TerminalSession::quote`, the family `cd` uses), as a paste, never
  with an Enter: files dropped on the panel, the selection with
  Ctrl+Shift+Enter (`ui.terminal_paths`, from the files or the
  terminal), or files on the clipboard pasted (ADR-062: their text, a
  path a line, would run line by line in a shell that takes no
  bracketed paste). A shell whose quoting is not known (nu, xonsh) is
  typed nothing, and says so; neither is a name a line cannot carry.
- The plugin types a `cd` only while the shell's process group has the
  terminal. Test with a real `/bin/sh`
  (`crates/plugins/terminal/tests/session.rs`).

## The player

- What plays is core data: the player publishes `playing::NowPlaying` in
  `App::player`; others (media keys via a plugin, scripts, agents) leave a
  `PlayerRequest`, taken at the next wake.
- A pinned player (`player.rs`) keeps playing while the selection moves,
  in the dock at the preview panel's foot (`dock.rs`); with the panel
  closed, a strip in the last pane's footer. Playback rules:
  [media.md](media.md).
