# ADR-099: Submenus beside their row, a setting on one row, one menu look

Status: accepted, 2026-10-27, the user's go on the proposal "Menus:
submenus beside, choices in a row" (a page shared in the session,
mock-ups from the debug build's menus); amended the same day by the
user's go on its version 2 (the file menu's order, decisions 6 to 9).
Built and checked on Linux the same day (`docs/STATUS-LOG.md`, "Menus:
submenus beside, choices in a row" and "The file menu, regrouped"); not
compiled for Windows or macOS yet (`just check-cross` not run).

## Context

- A context menu's submenu (`MenuItem::submenu`) unfolded under its row,
  pushing the rest down and Properties out of view; each submenu's row
  had a separator of its own. The reason in `menu.rs` ("a deferred draw
  inside this one, which GPUI cannot nest") stopped holding with the gpui
  vendored on 2026-10-18, which prepaints deferred draws in rounds.
- A menu item could not say it was on, or one of several values:
  plugins wrote "✓" into titles (a server's Thumbnails, Connect at
  Login), a tag's colors were nine "Color: …" rows with a check icon on
  the one set, a rating six rows that never said which.
- Toolbar popups (View, More and a dozen others) drew rows of their own:
  another background, taller rows, titles for every group, no keys, the
  current value filled like the row under the pointer. View took 24
  lines and ran past an 860 px window; More took 21, three settings of
  three values each a title and three rows.

## Decision

1. **A submenu opens beside its row**, as on Windows, macOS, KDE and GTK
   3 (GNOME's popovers slide one in place of the menu): its first row
   level with its row, on the right or, without room, the left, moved
   up to stay in the window (`PopoverAnchor::popup_beside`, a deferred
   draw above the menu's). The pointer opens it after resting on its row
   (`menu::REST`, 250 ms) or with a click; Right, Enter or Space open it
   and move into it, Left or Escape go back. The submenus' rows stand
   together before the menu's end under one line. A submenu holds no
   submenu.
2. **State is said, never written into a title.** `MenuItem::checked`
   is an item's on or off; `MenuItem::choice` (`registry::Choice`) makes
   items the values of one setting, told apart by `ChoiceLook`: icons,
   colors (`metadata::TAG_COLORS`) or stars. Both are optional fields of
   the serialized item.
3. **A setting is one row where its values allow it:** its name, then
   buttons (an icon each, five at most), swatches or stars. Values that
   need their words are rows, each checked or not, and a submenu they
   fill names the one chosen on its row. Choosing keeps the menu open.
   From the keys, Left and Right move a ring along the values, Enter or
   Space chooses; to a screen reader, a radio group of radio items.
4. **One menu look**: a popup that is a menu (`toolbar::menu_panel`)
   draws its rows as a context menu does: the chrome's background, rows
   of `toolbar::MENU_ROW` in the window's text with their keys, the row
   under the pointer or the keyboard highlighted, a check for what is on.
   A setting there is one row of `Button::segment`s (or tiles,
   `Button::tile`), Automatic in words, the value Automatic gives now
   outlined; Left and Right walk them.
5. **View and More rebuilt** with it: View's views as tiles, the sort's
   direction on the Sort by row in the key's words, the size on one row
   (11 lines on a list); More's panels as check rows, Split, Preview and
   File in an archive one row each (10 lines).

## Consequences

- Plugins say state through the item; the shell decides how it is drawn
  and names no plugin. The palette still lists each value's command.
- `ui.menu_expand` and `ui.menu_collapse` repeat while held: opening or
  leaving a submenu holds, and on a setting they step.
- The keys reach everything the pointer does in a menu, submenus and
  settings included; `just a11y-check` walks into a submenu and back.

## The file menu's order (amended 2026-10-27)

The user's go on the page's version 2, its five picks taken:

6. **The hundreds, by what is done to what a menu is about**, in every
   menu: 0xx open it, 1xx copy and keep it, 2xx change it (the
   destructive last, 280 to 299: Move to Trash beside Rename), 3xx make
   or find from it, 4xx the views and tools it opens, 5xx describe it,
   9xx the end. A submenu's row stands at its first item's order, in that
   item's group, rather than at the menu's end.
7. **Related actions share a submenu named by what they do; More and
   Notes go.** Open With › (the other applications for the selection's
   types, `open_with.menu`; Other Application…, the chooser; Run
   Command…), Compress › (To ZIP, Choose Format…, With a Password…), Find
   Similar ›, Tags › (Edit Tags…, a check per tag, the name tools).
   Rating and Comment… stand in the menu itself.
8. **Tags › checks the selection's tags**, then the pinned ones, a dash
   for a tag some files have (`Check::Partly`); a check in a submenu
   keeps the menu open, its command toggling (`tags.toggle`). A tag's own
   actions (its files here, its place, its color) are its chip's menu and
   the sidebar's.
9. **Paste is the folder's own menu's** (its background), never a file's:
   it pasted into the folder shown anyway. **Rename Several…** takes
   Rename's place with several files selected.

A picture with two tags: 21 rows and 10 separators before, 17 and 5
after.
