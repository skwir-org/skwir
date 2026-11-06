# ADR-119: The window's grounds let the desktop through

Status: accepted and built, 2026-11-06, on Linux (checked on X11 and on
a Wayland compositor without blur); macOS and Windows compiled only.
Amends ADR-071 (the contrast it guards holds on the theme's own grounds).
Amended the same day: a preview's own opacity, one ground under every
kind of preview, bands as tints (Amendments).

## Context

- **Asked for:** a setting for how opaque the window's background is,
  the file list and the interface apart, behind More settings, as a
  slider (2026-11-06).
- **The compositor can already fade a window** (Hyprland's window rules,
  KWin's, picom's): the whole of it, its text, icons and pictures with
  it. Terminals that let the desktop through (kitty's
  `background_opacity`, foot's alpha) fade their background alone, and
  their text stays whole. Only the program knows its background from
  what is drawn on it.
- **gpui draws a window that lets what is behind it through**
  (`WindowBackgroundAppearance::Transparent`, `Blurred`): on Wayland,
  blurred through KDE's protocol where the compositor offers it; on X11
  with a compositor; on macOS and Windows with the system's blur. While
  the window is not opaque it draws text in greyscale, never subpixel.
- **The window painted its grounds in layers:** `bg` under everything,
  the bars `surface` on top, the preview panel `surface` under its
  paper. Opaque, it shows nothing; let through, layers add up, a bar
  more opaque than its neighbour.
- **The theme guards contrast on its own grounds** (ADR-071): text at
  4.5:1 (7:1 when the desktop asks for more) on `bg` and `surface`.
  What shows behind the window is not known, so no ground that lets it
  through can promise a ratio.

## Decision

1. **Two settings, the theme's two grounds** (three since the
   amendment below). `ui.background_opacity`
   is the chrome's (`surface`: the bars, the tabs, the sidebar, the
   panel around a preview, the dock), `ui.files_background_opacity` what
   is read's (`bg`: the files, the terminal, a preview's paper), each a
   percentage from 50 to 100 in fives, 100 by default, on the Appearance
   page, folded under More settings (ADR-072): a detail of the look.
   Each is a slider (`SettingSpec::slider`), the knob's value worn as it
   moves and saved once let go.
2. **Only the grounds.** Each of the window's areas paints its ground
   once, at its opacity (`Theme::window_bg`, `window_surface`), over
   nothing else of the window (`docs/invariants/shell.md`). Text, icons,
   pictures, rows, fields, cards and selections are drawn on it as they
   are, opaque; so is everything drawn over the window: menus,
   popovers, tooltips, toasts, dialogs, Peek and the whole screen.
3. **Down to half.** Below 50 %, text over a light background behind a
   dark look (or the reverse) no longer reads at any ratio worth the
   name; the setting stops there.
4. **Blurred where the system blurs.** While either ground lets the
   desktop through, the window asks for a blurred background
   (`WindowBackgroundAppearance::Blurred`, `shell::show_behind`); a
   compositor that blurs by its own rules (Hyprland) does as it is set
   to, one that cannot draws it unblurred. Opaque, the window says so,
   and the compositor draws nothing behind it. No switch for the blur.
5. **Whole for more contrast.** While the desktop asks for more contrast
   (`Contrast::More`), both grounds stay whole, whatever is chosen
   (`theme::Opacity::chosen`): no ratio can be kept over what is behind.

## Consequences

- At 100 %, the window is drawn as before, pixel for pixel in eleven
  states compared with the build before the grounds were painted once
  (2026-11-06); a drag over the sidebar tints its own ground rather than
  the files'.
- Below 100 %, text is drawn in greyscale (gpui), and the compositor
  draws what is behind the window at each frame Skwir draws.
- A strip drawn on a preview's content (its notice, an archive's or a
  table's column header, a byte's meanings) stays opaque: it is drawn on
  the paper, not an area of its own. (Since the amendment below, it
  tints the paper.)
- Contrast on these grounds is guarded as before, on the theme's own
  colours; what the desktop puts behind them is the user's to judge.

## Not done

- A picture of one's own behind the files (a folder's background): an
  image to decode and keep in memory, and its contrast unguarded too.
- Mica and Mica Alt, Windows 11's own materials (gpui offers them): a
  choice of material would be a setting of its own, for one system.
- Not checked: KWin's blur, Hyprland's, macOS, Windows.

## Amendments

### 2026-11-06: a preview's own opacity, one ground under every kind

- **Seen:** with only the files' ground lowered, previews were not on one
  surface. A picture, a page or a film sat on the letterbox, the
  chrome's (opaque), music and what is read on the files' paper
  (see-through); on that paper, alternate rows, column headers, notices,
  code blocks, a code theme's background and the pinned contents were
  opaque tokens, so a table let the desktop through every other row.
- **Asked for:** previews harmonized, then a third slider for them (the
  user, 2026-11-06: "Yes, good, go ahead" to both).
- **Decided:**
  1. **Three settings.** `ui.preview_background_opacity`, "Preview
     background opacity", beside the other two, 50 to 100 % in fives,
     100 by default: what a preview shows, whatever it shows (text, a
     table, music, the letterbox around a picture, a page or a film),
     and Properties beside it. The files' slider is the files' and the
     terminal's; the chrome's covers the preview panel's bars and
     header card. Reading is where a busy background behind the window
     costs most: the files can let the desktop through while the
     preview stays whole, or the other way round.
  2. **One ground under what a preview shows.** Its area paints its
     paper or its letterbox once, at the preview's opacity
     (`Theme::preview_grounds`, `theme::Grounds`' `read` and
     `letterbox`); a code view's paper is its theme's own background
     (`inspector::paper`), painted there too. Peek and the whole screen
     stay opaque, the whole screen's letterbox the stage.
  3. **Bands tint the ground.** What is laid on the paper to set a part
     apart (alternate rows, a table's, a byte view's and an archive's
     headers, a notice, a byte's meanings, code blocks and inline code,
     pinned contents) is a tint of the paper it lies on
     (`Theme::on_bg`, `color::overlay`, as `hover` already was): on the
     paper whole it shows the token itself, and on paper that lets the
     desktop through it lets through about as much. What is a thing of
     its own (a picture, a page, a selection, a chip, a field, the
     floating contents, menus) stays opaque.
  4. **Cut, not covered.** A text read a screen at a time no longer
     covers what is past the page's end with a strip of paper: the list
     is cut there (`inspector_pages::cut`), so nothing is painted over
     the ground.
- **Consequences:**
  - At 100 % nothing changes: a tint laid on its paper whole is the
    token (the stripes, headers and letterbox read `F4F5F6`, `F0F1F3`,
    `F0F0F0` on `FAFAFA` in the light look, as before).
  - A band of another look (a dark code block on a light page, code
    kept in the other look) can only be as dark as it is by hiding
    more: no tint over a see-through light ground is both dark and as
    see-through as it. The code view itself, on its own paper, is as
    see-through as any.
  - Reading the preview on a busy background is one slider away,
    without making the files opaque.
- **Checked** on a headless sway (no blur) with the preview at 60 %
  and the files and the chrome whole: a table, Markdown (a code block,
  a table, inline code), code in the dark look on a light window, a
  picture, music, bytes, a film, a long document a page at a time
  (the paper past the page's end the same as above it), its contents
  pinned and Properties above it. Not checked: X11 with a compositor,
  KWin's or Hyprland's blur, macOS and Windows.
