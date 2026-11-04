# ADR-117: Text sizes by part, from the desktop's

Status: accepted, 2026-11-04 (GitHub issue #5). Amends ADR-073 (its "Not
done: a size per role") and ADR-071 (the desktop's font is read with its
size). Built in four steps: the files' size (1), reading and code (2),
the interface's named sizes and heights (3), the desktop's sizes and
Qt's settings (4).

## Context

- **Issue #5:** "In the grid view for the file list, the font size is
  small; I would like to be able to change it." The reporter runs labwc
  at a scale of 1.25, applied to the whole window. The grid drew names
  at 12 px (`text_xs`), the list's rows, the sidebar and the board at
  14 px: the grid's names were a size smaller than any other view's.
- **Skwir's 14 px was gpui's, not a desktop's.** The window's root set
  `text_sm` (0.875 rem) and gpui's rem is 16 px: Tailwind CSS's scale,
  where 16 px is a browser's default. No decision compared it with a
  desktop's size. Skwir read the desktop's font family and dropped its
  size; GNOME's text scaling factor was not read either.
- **Nothing could be enlarged.** Zoom sizes pictures and icons, never
  names. The preview's text size went back to 100 % for every file, and
  was greyed for tables and the byte view: a CSV could not be made
  larger at all. Heights were fixed pixels (rows 26, controls 28, bars
  42), so larger text in them would have been clipped.
- **Measured beside Dolphin** on the reference machine, 2026-11-04, on
  Xvfb at 96 DPI: Dolphin draws in qt6ct's "Sans Serif, 10 pt" (DejaVu
  Sans, 13.3 px), "Documents" 72 px wide; Skwir's list in Noto Sans at
  14 px, 73 px; its grid at 12 px, 63 px. The two lists matched by
  coincidence: DejaVu is about 6 % wider than Noto at a size.
- **What others use when nothing is set:** GTK "Sans 10" (13.3 px), Plasma
  and Xfce 10 pt, GNOME 11 pt (14.7 px), Qt without a desktop's settings
  9 pt (12 px), Chromium's interface 12 px (13 on macOS), macOS 13 px,
  Windows 11's WinUI body 14 px (Explorer's file list, a legacy view,
  Segoe UI 9 pt), web pages 16 px. A browser's tabs and menus are in the
  toolkit's font: Firefox's GTK's, Chrome's Qt's on Plasma and LXQt and
  GTK's elsewhere, by its own choice.
- **GTK and Qt can disagree.** Outside GNOME, Plasma and Xfce (Hyprland,
  Sway, labwc, niri) they are set up apart (nwg-look, qt5ct, qt6ct): on
  the reference machine GTK names Noto Sans 10 and qt5ct and qt6ct "Sans
  Serif, 10". Skwir read Qt's only on Plasma.
- The user asked, 2026-11-03, for sizes per part of the window beside
  the font settings, and took the recommendations below on 2026-11-04.

## Decision

1. **Four parts, a size each.** Interface (`ui.text_size`, "Text size":
   bars, menus, the sidebar, dialogs, tooltips, footers), Files
   (`ui.files_text_size`: names and details in the list, the grid, the
   columns and the board; issue #5), Reading (`reading.text_size`: the
   running text of previews) and Code (`ui.code_text_size`: code
   previews, the byte view, the terminal). Each is a percentage in
   steps, 80, 90, 100, 110, 125, 150, 175 and 200 (`sizes::STEPS`),
   chosen like the preview's own sizes rather than in points, which
   mean different things on different desktops and displays. The files'
   100 % is the interface's size.
2. **100 % is the system's body text.** The interface's: the desktop's
   font size (points × 96 ⁄ 72) times its text scaling factor; where
   nothing names a size, the system's own: 10 pt (13.3 px) on Linux,
   GTK's own fallback; 13 px on macOS; WinUI's 14 px on Windows, times
   its Text size once read (Windows has no font size to read, and
   Explorer's 12 px is a legacy view's, under which Skwir's small text
   would fall below Microsoft's 12 px minimum). Reading stays at 16 px,
   as browsers keep a page's text whatever the desktop says; code
   follows the desktop's monospace size, as terminals and editors do,
   else 14 px. The text scaling factor applies to every part, and is
   followed with Desktop colors off, as more contrast is.
3. **A part is drawn in a rem of its own** (`elements::rem`, gpui's
   `with_rem_size` in each phase): the rem is the part's body text over
   0.875, so `text_sm` is its body text, `text_xs` its small text, and
   gpui's spacing scale follows. The window's rem is the interface's.
4. **What holds text grows with it.** A length that holds text is
   written at 100 % and drawn at its part's scale, in whole pixels
   (`sizes::Scale`): from the theme in `render`, from the rem in an
   element's layout. Pictures and icons keep the zoom's size: a list row
   is its text's height or its icon's, whichever is taller; a grid label
   is the picture's width or the least a name needs. A width dragged
   (the list's columns) is kept at 100 %.
5. **Sizes are tokens, as colours are.** The theme names a few sizes,
   Body, Small and Tiny (step 3), and every place names one of them
   rather than `text_xs` or a literal: a name or value you read or
   choose is Body, what labels or describes it is Small, badges Tiny.
   Grid names are Body (step 1), as are tab titles and Properties'
   values, their labels muted (step 3).
6. **The preview's picker changes the file shown;** Reading's and Code's
   settings are where each file starts, and the picker shows the same
   percentages. Tables and the byte view follow it (step 2).
7. **Where the fonts are read: "Fonts from"** (`ui.fonts_from`, Linux
   only): Automatic, GTK or Qt (step 4). Automatic reads the running
   desktop's own settings (GSettings, `kdeglobals`, xfconf, LXQt's
   `lxqt.conf`), else GTK's (`settings.ini`, then the settings portal),
   else Qt's (qt6ct or qt5ct, as `QT_QPA_PLATFORMTHEME` names), else the
   system's default size. GTK or Qt reads that toolkit's alone. A
   family and its size are read together, as every source writes them.
   Colours stay as ADR-071 reads them.
8. **Commands and keys.** `ui.text_size` (`part`, `dir`: in, out, reset)
   steps a part's size as its setting does, the palette offering the
   files'; no default keys: Ctrl+= and Ctrl+wheel stay zoom.
9. **Diagnostics.** About › Copy details says the display's scale and the
   fonts' source and sizes (step 4).

Rejected: a whole-window zoom made by patching gpui to multiply the
display's scale. One setting, sharp, but it enlarges everything as a
denser screen would, icons and spacing included, cannot size the parts
apart, and changes how gpui maps the pointer.

## Consequences

- On a desktop at 10 pt the window's body text goes from 14 to 13.3 px
  (step 4): the list, the sidebar and menus about 5 % smaller, the
  grid's names larger (12 to 13.3 px). Dolphin's size, and GTK's.
- `Theme::sizes` holds the parts' sizes, resolved with the look
  (`Theme::resolve`) whenever the settings or the desktop's style move;
  what was measured at a size is measured again (the grid's lines, keyed
  by size; the preview's rows).
- Step 1 (2026-11-04): `ui.files_text_size` on Appearance after Font; a
  pane's view drawn in the files' rem; the list's rows, header, gaps,
  name and columns, the columns view's columns, the grid's labels (now
  Body, their line 18 px at 100 %), captions and least width, the
  board's columns and cards at the files' scale; `ui.text_size`.
- Step 2 (2026-11-04): `reading.text_size` (Reading, Text, after the
  reading font) and `ui.code_text_size` (Reading, Code, after the code
  font). A preview is drawn at its part's setting until a size is
  chosen for it (`inspector::State::percent`), the picker showing the
  setting's percentages, stepping in the preview's own (60 to 250), As
  in Settings going back; tables (their cells, header and columns) and
  the byte view (its rows, header and columns) follow it, no longer
  greyed; a book laid out by an engine starts at Reading's size; the
  terminal at the code's scale.
