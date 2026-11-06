# ADR-119: The window's grounds let the desktop through

Status: accepted and built, 2026-11-06, on Linux (checked on X11 and on
a Wayland compositor without blur); macOS and Windows compiled only.
Amends ADR-071 (the contrast it guards holds on the theme's own grounds).

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

1. **Two settings, the theme's two grounds.** `ui.background_opacity`
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
  the paper, not an area of its own.
- Contrast on these grounds is guarded as before, on the theme's own
  colours; what the desktop puts behind them is the user's to judge.

## Not done

- A picture of one's own behind the files (a folder's background): an
  image to decode and keep in memory, and its contrast unguarded too.
- Mica and Mica Alt, Windows 11's own materials (gpui offers them): a
  choice of material would be a setting of its own, for one system.
- Not checked: KWin's blur, Hyprland's, macOS, Windows.
