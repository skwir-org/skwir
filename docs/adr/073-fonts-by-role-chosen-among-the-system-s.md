# ADR-073: Fonts by role, chosen among the system's

Status: accepted, 2026-10-19, and built the same day on Linux; macOS and
Windows compiled, their font registries not asked (`docs/PORTING.md`).
Amends ADR-071 (its font switch becomes a choice, and the font follows
while Skwir runs).

## Context

- **Two families, chosen once.** The window opened in an interface
  family (the desktop's, ADR-071, else Skwir's candidates, Inter first)
  and a coding one (JetBrains Mono first), resolved before the first
  frame (`pick_font`, `crates/ui/src/lib.rs`). The shell kept them as
  two strings, and some forty files took copies: the toolbar's tooltips,
  the dialogs, the terminal, the settings window. A change could show
  only at the next start.
- **Reading mode wrote in the interface font.** Markdown and HTML,
  documents, books and mail drew their running text in the font of the
  menus. A page's own fonts already stay the reader's
  (`docs/invariants/previews.md`): a book's stylesheet names no family
  Skwir draws.
- **The desktop names three fonts.** GNOME's `org.gnome.desktop.interface`
  has `font-name`, `document-font-name` and `monospace-font-name`; MATE
  has the three; Plasma's `kdeglobals` has `[General] font` and `fixed`;
  Xfce's xsettings `/Gtk/FontName` and `/Gtk/MonospaceFontName`. The
  settings portal answers the GNOME three outside GNOME too: on the
  reference session (2026-10-19) Noto Sans, Noto Sans and Hack. Skwir
  read the first and drew code in JetBrains Mono whenever it was
  installed. A desktop may name a fontconfig alias (`Sans 11`,
  `Monospace 11`, GNOME's defaults before 48), which no text system but
  fontconfig resolves.
- **The system has many families.** 497 on the reference machine
  (`fc-list`), 40 of them monospaced (`fc-list :spacing=mono`).
- The user asked, 2026-10-19, to let the preferred font be chosen
  "among those on the system when relevant (text previews, EPUB/HTML
  rendering, etc.)", then to make the interface font a choice too, and
  took the recommendations below.

## Decision

1. **Three roles.** What is drawn asks a role, never a family:
   - **Interface** (`ui.font`, "Font"): menus, lists, bars, dialogs,
     tooltips, everything not below.
   - **Reading** (`reading.font`, "Reading font"): the running text of
     what is read: Markdown and HTML, word processors' documents, books
     the built-in engine reflows, mail, a book's note opened under its
     reference.
   - **Code** (`ui.code_font`, "Code font"): what is read a character
     at a time: the code view (source and text files), code in
     documents, the byte view, the terminal, the names compared in bulk
     rename and in a proposal's review.
   - **None of them:** what is drawn in a document's own fonts (PDF,
     DjVu, fixed-layout pages, books MuPDF lays out) and a font file's
     specimen.
2. **Automatic is the desktop's while Desktop colors is on.** Each role
   is Automatic by default: the interface takes the desktop's interface
   font, reading its document font (else the interface's), code its
   monospace font. With Desktop colors off, or where the desktop names
   none (macOS, Windows: ADR-071 point 3), Automatic is Skwir's: its
   interface candidates, the interface font for reading, its coding
   candidates. A name that is a fontconfig alias (`sans`, `sans-serif`,
   `serif`, `monospace`, `mono`) is resolved by `fc-match` where the
   desktop is read, as the MuPDF worker resolves a book's generic
   families (`crates/mupdf-worker/src/fonts.rs`).
3. **A family chosen is kept whatever Desktop colors says.** One that
   does not load (uninstalled since) is drawn as Automatic, and the
   setting says it is not installed, as the icon theme's does.
4. **Fonts are tokens of the theme.** `Theme` holds `font`,
   `reading_font` and `code_font`, resolved with the rest of the look
   (`Theme::resolve`) against the text system whenever the settings or
   the desktop's style move: each family asked whether it loads
   (`font_id`), never resolved (`resolve_font` panics when neither it
   nor gpui's fallback is installed), the first that loads winning, and
   when none of Skwir's candidates does, any family the system has.
   Nothing keeps a copy: what draws reads the theme, tooltips and
   windows of their own included, so a change shows at once. What was
   measured in a font is measured again when it changes (the preview's
   rows, as a text size change does); what is measured each frame (the
   terminal's cells, the grid's wrapped names keyed by font) follows by
   itself.
5. **The choices are what gpui can draw.** The families are the text
   system's own (`TextSystem::all_font_names`), those it hides (a
   leading `.`) left out, and, where the system says which, those with
   no face that writes Latin: gpui writes in a face only when it has an
   `m`, the others (Balinese, Arabic, emoji) staying fallbacks
   (`skwir_platform::fonts::families`, fontconfig's `:charset=6d`).
   They are listed on a worker at start and when the
   settings open, as bat's themes are, the setting declared again with
   them: a script or an agent chooses among them before the dialog ever
   opens, and a family a file names outside them is noted, not refused
   (`more_choices`). Listing reads the text system's index and runs
   `fc-list` once, about 15 ms in a process of its own on the reference
   machine. The code font lists the monospaced families where the
   system says which (`skwir_platform::fonts::monospaced`: fontconfig's
   spacing on Linux); a proportional font shifts columns and the
   terminal's cells. macOS and Windows list every family for code until
   they say (`docs/PORTING.md`).
6. **A long list narrows as you type.** A choice of more than
   `settings_view::FILTERED` entries (fonts, icon themes, bat's themes)
   opens with a field above its list that keeps the entries holding what
   is typed, the list drawn a row at a time as it scrolls; the arrows on
   its control open it rather than step through it, which would apply,
   and load, each font passed. A font's
   entries are written in the interface font; a line beneath them shows
   the entry pointed at, or else the one chosen, in its own family: a
   family drawn is loaded and kept by the text system, so only the one
   looked at is. The closed control writes the family chosen in itself.
7. **Where they are shown (ADR-072).** The interface font on Appearance,
   after Desktop colors, folded; the reading font on Reading's Text,
   essential (people are divided on it: serif or sans, a face made for
   dyslexia); the code font on Reading's Code, folded. Only the family
   is chosen: sizes stay the text size and zoom.

Not done: a size per role (the text size and zoom are per view), the
weight or the features of a family (ligatures stay off where glyphs
must keep their cells), fonts a user adds to Skwir alone (a font is
installed on the system, where every application finds it).

## Consequences

- `ui.desktop_font` (a switch, unreleased) is gone; `ui.font` takes its
  place, not under Desktop colors. ADR-071's points 7 to 9 are amended
  (see there).
- The first frame resolves three families instead of two; the reading
  font costs a load only when it differs from the interface's. Measured
  2026-10-20, `tools/measure/idle-pss.py` on a headless sway (pixman,
  lavapipe), release builds of HEAD and of this change alternated, seven
  runs each, the session's portal answering: own share 109.9 MiB against
  109.1 (medians; the change is not larger), first frame 76.2 ms against
  78.0 (this change 71 to 91 ms, HEAD 74 to 80: within the spread).
  Absolute values there are lavapipe's, above the session's.
- `skwir_platform::desktop_style::DesktopStyle` gains `document_font`
  and `monospace_font`, read and followed as `font` is; the cache keeps
  them for the first frame.
- `Shell::font` and `Shell::mono_font` go; `toolbar::tooltip` and
  `clip::Clip::tooltip` take no font.
- Tests: the roles' fallbacks (a family that does not load, Desktop
  colors off, a desktop that names none, none of Skwir's installed); the
  desktop's three fonts read and followed from a portal of the tests'
  own; fontconfig's aliases and its listing parsed; the settings' choices
  (code offered the monospaced, a family no longer installed said so),
  the pages they stand on and the essentials' budget.
- Checked in a window (debug build, Xvfb, the session's portal, the
  reference machine's fonts), 2026-10-19: the window opened in the
  desktop's three fonts (Noto Sans, Noto Sans, Hack); `settings.apply`
  from a script changed the reading, interface and code fonts, each
  shown at once, a Markdown preview shown measured again (its list items
  re-wrapped, nothing overlapping); the interface font's list narrowed
  by typing, moved by the arrows and the pointer, its specimen following,
  Enter applying it to the whole window and the dialog, Escape closing
  the list alone; the way back gave Automatic again; Desktop colors off
  gave Skwir's own (DejaVu Sans, DejaVu Sans Mono, Inter and JetBrains
  Mono not being installed). Not checked: Plasma's and Xfce's settings
  read (their code is the GNOME path's twin, untried there), a screen
  reader on the long list, idle memory on the session's own display.
- Seen while checking, not changed: Noto Sans on the reference machine is
  one variable file whose Bold is a named instance, and gpui draws its
  bold runs regular; DejaVu Serif's bold draws.
