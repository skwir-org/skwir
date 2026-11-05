# ADR-117: Text sizes by part, from the desktop's

Status: accepted and built, 2026-11-04 (GitHub issue #5), on Linux;
macOS's and Windows' interface fonts and Windows' Text size read since
2026-11-05, compiled, Windows' tried under Wine (`docs/PORTING.md`). Amends ADR-073 (its "Not
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
   columns and the board, and an archive's members in its preview, a
   list of files too; issue #5), Reading (`reading.text_size`: the
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
   its Text size (Windows has no font size to read, and
   Explorer's 12 px is a legacy view's, under which Skwir's small text
   would fall below Microsoft's 12 px minimum). Reading stays at 16 px,
   as browsers keep a page's text whatever the desktop says; code
   follows the desktop's monospace size, as terminals and editors do,
   else 14 px. The text scaling factor applies to every part, and is
   followed whatever the fonts are read from, as more contrast is.
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
   choose, and anything written as a sentence (a tooltip, a setting's
   description, a message, a hint), is Body; a short label in a tight
   place (a button's, a column's header, a count, a caption, a
   shortcut) is Small, nine tenths of the body (amended 2026-11-05: it
   was six sevenths, 11.4 px at a desktop's 10 pt); badges Tiny. Grid
   names are Body (step 1), their lines 1.2 times the text, as are tab
   titles and Properties' values, their labels muted (step 3).
   Amended again on 2026-11-05 (the user: Small and Tiny sparingly):
   Body is the default, for all text read or acted on, every control
   included (buttons, dropdowns, segmented choices, fields, the
   toolbar's labels, the footer, dialogs' forms and messages); Small
   only for a note read after the Body text beside it (a shortcut or a
   key cap, a count or a unit, what is said under a name, a column's
   header, a group's heading, a short hint beside a control); Tiny,
   four fifths of the body (it was 0.71: 9.5 px at 10 pt, below any
   desktop's smallest text), for badges. Later that day (the user: tag
   chips too small) chips moved up a size and the last badge to Small,
   and Tiny was removed: the sizes are Body, Small and Title, and
   nothing is written below Small.
6. **The preview's picker changes the file shown;** Reading's and Code's
   settings are where each file starts, and the picker shows the same
   percentages. Tables and the byte view follow it (step 2).
7. **Where the fonts are read: "Fonts from"** (`ui.fonts_from`, Linux
   only): Automatic, Skwir's own, GTK or Qt (step 4; Skwir's own and
   its standing apart from Desktop colors, which is then the colors
   alone, 2026-11-05). Automatic reads the running
   desktop's own settings (GSettings, `kdeglobals`, xfconf, LXQt's
   `lxqt.conf`), else GTK's (as GTK's applications there take them: on
   Wayland the settings portal, else `settings.ini`; on X11
   `settings.ini`, else the portal), else Qt's (qt6ct or qt5ct, as `QT_QPA_PLATFORMTHEME` names), else the
   system's default size. GTK or Qt reads that toolkit's alone. A
   family and its size are read together, as every source writes them.
   Skwir's own is its fonts at the sizes Automatic reads (2026-11-05,
   the user's choice: it had a size of its own, 14 px, so choosing
   Skwir's look overrode the size someone set their desktop to; how
   large text is written is a need, as the text scaling and more
   contrast are, and the Text size is where Skwir's is chosen).
   Colours stay as ADR-071 reads them. On macOS and Windows (2026-11-05)
   Automatic is the system's interface font (a Mac's San Francisco at
   13 px, Windows' message font, Segoe UI, at 14 px times its Text size)
   and Fonts from offers Automatic and Skwir's own; code stays Skwir's
   own there, neither naming a monospace font.
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
- Step 3 (2026-11-04): `ui.text_size` (Appearance, after Font) sets the
  window's rem; every place names a size (`sizes::TextSizes`; about 190
  places, `text_xs` Small, `text_sm` Body, gpui's larger sizes Title,
  the badges' literals Tiny), checked by a test; tab titles and
  Properties' values are Body. About 270 lengths that hold text (bars,
  rows, controls, chips, menus' rows, paddings) are in rems
  (`sizes::sized`); the chip strip, the adaptive rows, the map's,
  Usage's and the charts' labels follow the rem in force; what is
  counted (the menu of places, the address card's rows and padding,
  the preview panel's limit, a popup's scroll) is scaled; the
  sidebar's width is kept at 100 %. A choice's picture in a form
  (`form::drawn`) keeps its size.
- Step 4 (2026-11-04): `skwir_platform::desktop_style` reads each font's
  size with its family (Pango's points or pixels, Qt's points or pixels:
  `pango_size`, `qt_size`) and the portal's `text-scaling-factor`; LXQt's
  `lxqt.conf` is read as its own desktop's (its icon theme too), Qt's
  fonts from qt6ct's or qt5ct's file as `QT_QPA_PLATFORMTHEME` names it,
  after GTK's in Automatic; GTK's and Qt's fonts are said apart
  (`DesktopStyle::gtk`, `qt`). `ui.fonts_from` (Linux, under Desktop
  colors) chooses them. 100 % is the desktop's size times its scaling,
  else `sizes::SYSTEM_BODY`; code the monospace size; reading 16 px
  times the scaling. About's Copy details says the display's scale (and
  `GPUI_X11_SCALE_FACTOR`) and the text's font, size, source and steps;
  the guide says what is followed and the X11 workarounds. A change of
  qt5ct's or qt6ct's file is taken in at the next change the desktop
  says, or the next start: it is not watched (until 2026-11-05, below).
- Followed, 2026-11-05: GTK's fonts are read in the order GTK reads
  them, read in GTK 3.24's and 4.22's sources (on Wayland the portal's,
  else GSettings', outrank `settings.ini`; on X11 without an XSETTINGS
  daemon `settings.ini` is all); qt5ct's, qt6ct's, LXQt's and GTK's
  files are followed as they are written or replaced, beside
  `kdeglobals` (one watcher for their folders, a folder not there not
  followed). Checked by the platform's tests: the order on Wayland and
  X11 against a fake portal, qt6ct's file written in a folder of the
  test's own. A font an old qt5ct wrote in Qt's binary form
  (`@Variant(…)`) names nothing, so Automatic goes on past it: qt5ct
  1.9 writes fonts as text (`QFont::toString`, read in its source) and
  writes an old one again so when its fonts are next saved; decoding
  the binary form was judged not worth it.
- 2026-11-05 too: a book its engine laid out (MuPDF's) at Reading's size
  is laid out again, where the reader is, when that setting changes
  while it is shown; one sized in the preview keeps its size, and the
  preview's reset follows the setting again. An archive's members (rows,
  header, indents, size column) are at the files' size, drawn in their
  rem, the size column giving way as the list's does. Checked on Xvfb:
  the synthetic EPUB through MuPDF, `reading.text_size` set to 200 in
  `settings.toml` while shown, the book laid out again at twice the
  size on its first page; a zip of nested folders at Files 100 % and
  150 %.
- Body by default, 2026-11-05 (point 5 amended): of 126 places at
  Small, 48 moved to Body (the shared button, dropdown and segment, the
  toolbar's buttons, the footer and the player's line, dialogs' forms,
  summaries and messages, the address card's controls, Usage's summary,
  the document outline); the places menu is measured at Body as its
  rows are drawn. Tiny is 0.8 of the body; the grid's tag chips, which
  wrote a length (10 px at 100 %, 9.5 at 10 pt) in a chip of a fixed
  16 px, take Tiny (Small on the largest pictures) in a chip that grows,
  and `every_place_names_a_text_size` now refuses a length as a text
  size. Then (the user, the same day: tag chips read too small) a tag's
  chip is Body on rows, Properties and the preview's header, as the
  sidebar's places are (it is clicked: read and acted on), 20 px tall at
  100 %, and Small over a picture, Body over the largest; Tiny is left
  to History's row label. The review, clash and delete dialogs' rows (`modal::Rows`) grow
  with the text, the review's at a Body line (24 px at 100 %). An empty
  place's title is Body, semibold (in a variable font too since gpui
  patch 0045, ADR-073), and its message is as wide as the
  pane up to its measure: it ran past a narrow pane's edges. Checked on
  Xvfb at the reference machine's 10 pt: the list, the grid with tagged
  pictures, Settings at 100 % and 150 %, the delete dialog, F1, the
  palette, Activity, Nearby in a 560 px window.
- macOS and Windows, 2026-11-05: `desktop_style` reads a Mac's interface
  font (`.AppleSystemUIFont`, the family `NSFont.systemFont` names, at
  `NSFont.systemFontSize`'s 13 px) and Windows' (the message font of
  `SystemParametersInfoW`'s non-client metrics) with its Text size
  (`TextScaleFactor` under `HKCU\Software\Microsoft\Accessibility`),
  followed through `RegNotifyChangeKeyValue` on that key and the window
  metrics', waiting on their events and the stop's, no timer.
  `desktop_style::COLORS` (Linux), `FONTS` (all three) and `TOOLKITS`
  (Linux: GTK and Qt read apart) say what each system reads; a font
  setting's label calls a family the system keeps apart (a leading `.`)
  "the system font". Checked: clippy for Linux, Windows and macOS; under
  Wine, the message font read, the Text size's range, and a Text size
  written in the test prefix's registry followed. A Mac's not run.
- Reviewed beside 0.3.0, 2026-11-05 (the release's AppImage and the
  build of step 4 on one display, one folder, the reference machine's
  settings): body text 14 to 13.3 px at the desktop's 10 pt, as GTK's
  applications there draw it (Dolphin looked as large as 0.3.0 because
  qt5ct names DejaVu Sans, about 6 % wider than Noto Sans); Small text
  12 to 11.4 px, below anything a native application there draws, in
  about 170 places; grid names in the same label, about 10 % fewer
  characters a line, their lines looser (17 px for 13.3 px). A trial
  with every Small as Body read better but overflowed the settings'
  choices. The user chose: Automatic keeps following the desktop's
  size exactly (a desktop at 10.5 pt gives 0.3.0's 14 px); sentences
  become Body (40 places: tooltips, settings' descriptions and
  choices, toasts' reasons, messages, notes and hints, the preview's
  line of facts, History's and Activity's actions); Small becomes nine
  tenths of the body (12 px at 10 pt, as in 0.3.0); grid lines 17 px at
  100 %. Fonts from gains "Skwir's own" and leaves Desktop colors (the
  next commit).
- Settings, 2026-11-05 (the user's choices): Fonts from (Automatic,
  Skwir's own, GTK, Qt) stands on its own after Desktop colors, which
  is the colors alone again, so its name fits without a new key; someone
  who had turned Desktop colors off keeps Skwir's own fonts
  (`desktop_style::keep_own_fonts`, as `Settings::rename` moves a
  value). Each font setting's Automatic says the family it comes to
  ("Automatic (Noto Sans)"); each size step the size it comes to, in
  points as the desktops' font settings say them ("110% (11 pt)"),
  written in its list at that size in its part's font (2026-11-05, the
  user's choice over a slider with a sample);
  Fonts from says what it reads now ("Now Noto Sans at 10 pt, from
  GTK."), all declared again when what they come to changes
  (`settings::show_resolved`). Percentages stay over points: three of
  the four sizes are relative by nature, and a percentage keeps
  following the desktop's size where a size in points would stop.

