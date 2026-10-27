# ADR-099: Submenus beside their row, a setting on one row, one menu look

Status: accepted, 2026-10-27, the user's go on the proposal "Menus:
submenus beside, choices in a row" (a page shared in the session, mock-ups
from the debug build's menus). Built and checked on Linux the same day
(`docs/STATUS-LOG.md`, "Menus: submenus beside, choices in a row"); not
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

## Not decided here

The file menu's own order (which hundreds hold what, Paste on a file,
Tags as one submenu with checks, Compress and Open With as submenus):
proposed the same day as the page's version 2, waiting for the user's
choices (`docs/PLAN.md`, "What is left").
