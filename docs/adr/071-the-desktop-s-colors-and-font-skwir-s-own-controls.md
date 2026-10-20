# ADR-071: The desktop's colors and font, Skwir's own controls

Status: accepted, 2026-10-19. Steps 1 and 2 built 2026-10-19: the
derivation and the guard, Skwir's palettes through them; Linux's portal
read and followed (the accent, more contrast, the font). Steps 3 and 4
built 2026-10-20: Plasma's scheme, followed, and GTK3 themes' colors.
macOS and Windows are not read yet. Amended 2026-10-19 (two switches,
the font at the window's opening), again by ADR-073 (the font a choice
among the system's, three of them, followed live), and 2026-10-20 (steps
3 and 4).

## Context

- **Skwir draws every control itself**, from gpui's primitives: the form
  controls in `crates/ui/src/controls.rs`, the elements whose layout
  depends on their size in `crates/ui/src/elements/`. Colors are tokens
  of `theme.rs` (two literal colors outside it, in `elements/halo.rs`);
  radii are gpui's fixed scale (about 110 call sites); heights are
  constants (`controls::HEIGHT`, rows of 26 px, bars of 42 px).
- **What follows the desktop today:** the look, light or dark (`ui.theme`
  Automatic, through gpui: the portal's `color-scheme` on Linux); file
  and place icons (`ui.icon_theme` Automatic, the theme found where each
  desktop keeps it: GSettings, `kdeglobals`, xfconf, GTK's
  `settings.ini`, `skwir_platform::icons`); the title bar (the system's
  decorations). **What does not:** the accent, a preference
  for more contrast, a color scheme, the interface font (Linux tries
  Inter first, `crates/ui/src/lib.rs`).
- **What the desktops let a user choose:**
  - GNOME: the look, an accent and higher contrast, answered by the
    settings portal (`org.freedesktop.appearance`); libadwaita ignores
    GTK themes. Its interface font is `org.gnome.desktop.interface`'s
    `font-name`, also through the portal.
  - Plasma: a whole color scheme, an accent and a font, in `kdeglobals`.
  - Xfce, Cinnamon, MATE: a GTK3 theme, CSS whose named colors
    (`@define-color theme_bg_color`…) most themes define.
  - macOS: the look, an accent, "Increase contrast".
  - Windows: the look, an accent, a contrast theme (a palette of its
    own, which Windows asks applications to draw with).
- **The portal answers outside GNOME too.** On the reference session
  (Hyprland, 2026-10-19) it answers `color-scheme`, `accent-color`,
  `contrast`, and `org.gnome.desktop.interface`'s `font-name` and
  `gtk-theme`. gpui already listens to it through ashpd
  (`gpui_linux`'s `xdg_desktop_portal.rs`) but passes on only the look.
- **Skwir's own palettes fail three contrast pairs** (WCAG 2 ratios,
  computed 2026-10-19): in the light look, `text_muted` on `surface`
  4.35:1 (the sidebar) and `warning` on `bg` 4.06:1 (a warning in
  Settings); in the dark look, `on_fill` on `accent` 2.42:1 (the count
  on a drag, `dnd.rs`). Text asks for 4.5:1.
- The user asked, 2026-10-19, how feasible it would be "supporting GTK
  and Qt themes on Linux, and mimicking the respective native themes of
  macOS and Windows". The answer: colors, accent, contrast and font from
  the desktop are cheap and fit ADR-000's "harness the environment";
  controls shaped after each system are design work per system that
  cannot be looked at here (macOS is compiled only, Wine draws no
  Windows 11); rendering GTK or Qt themes needs the toolkits themselves.
  The user: "let's go with your recommendation".

## Decision

1. **Skwir's controls, on every system.** Shapes, sizes, spacing, radii
   and shadows stay Skwir's scales (`theme.rs`'s header) everywhere. No
   look of controls per system, no window material (Mica, vibrancy:
   gpui offers them, but they are a system's style). GTK and Qt are
   never loaded to draw: a GTK theme is CSS over GTK's widgets, a Qt
   style is code (Kvantum's SVG), and reproducing either means that
   toolkit in the process, on Linux alone, past the memory budget.
2. **From the desktop, what the user chose there:** the look (as now),
   the accent, the contrast preference, a color scheme where the desktop
   lets the user pick one, the families of its fonts (the interface's,
   documents', monospace; ADR-073). Never a
   platform's fixed palette (Adwaita's grays, macOS's window
   backgrounds, Fluent's): those are their designers' choices for their
   own controls, and copying them is the imitation point 1 declines.
3. **Where each comes from:**

   | | Accent | Contrast | Color scheme | Font |
   |---|---|---|---|---|
   | Linux | the portal's `accent-color` | the portal's `contrast` | Plasma (`XDG_CURRENT_DESKTOP` names KDE): `kdeglobals`. Elsewhere: the GTK3 theme the desktop names, where its files define its colors | on Plasma, `kdeglobals`' `[General] font` and `fixed`; elsewhere found as the GTK theme is |
   | macOS | `NSColor.controlAccentColor` | "Increase contrast" (`NSWorkspace`) | none | Skwir's |
   | Windows | `UISettings`' accent | a contrast theme on (`SPI_GETHIGHCONTRAST`) | the contrast theme's system colors (`GetSysColor`) | Skwir's |

   The GTK3 theme is `GTK_THEME`, else the one the desktop keeps, read
   where the icon theme is read (`chosen_in`, in the platform's
   `icons.rs`): GSettings' `gtk-theme`, Xfce's `/Net/ThemeName`,
   `settings.ini`'s `gtk-theme-name`; on a desktop none of those names,
   the portal's `gtk-theme`. The fonts are read beside it: the
   interface's (`font-name`, `/Gtk/FontName`, `gtk-font-name`, the
   portal's `font-name`), documents' (`document-font-name`, the
   portal's), monospace (`monospace-font-name`, `/Gtk/MonospaceFontName`,
   the portal's); a fontconfig alias among them (`Monospace`) is what
   `fc-match` makes of it (ADR-073). The
   theme is found under the XDG data folders (`skwir_core::xdg`) and
   `~/.themes`, with `gtk-dark.css` for the dark look where it has one.
   Its colors are its `@define-color`s, after its `@import`s within the
   theme, with GTK's `shade`, `mix`, `alpha`, `lighter` and `darker`
   evaluated. A theme whose CSS lives only in a compiled
   `gtk.gresource`, or GTK's built-in Adwaita, gives none.
   macOS and Windows name no font a user chose: Skwir's own there.
4. **A scheme fills the base tokens; the rest are derived.**

   | Token | `kdeglobals` | GTK3 theme | Windows contrast theme |
   |---|---|---|---|
   | `bg`, `text` | `[Colors:View]` `BackgroundNormal`, `ForegroundNormal` | `theme_base_color`, `theme_text_color` | `COLOR_WINDOW`, `COLOR_WINDOWTEXT` |
   | `surface` | `[Colors:Window]` `BackgroundNormal` | `theme_bg_color` | `COLOR_BTNFACE` |
   | `text_muted` | `[Colors:View]` `ForegroundInactive` | derived | `COLOR_GRAYTEXT` |
   | `zebra` | `[Colors:View]` `BackgroundAlternate` | derived | derived |
   | `border` | derived | `borders` | `COLOR_WINDOWTEXT` |
   | `accent` | `[General]` `AccentColor`, else the selection's background | `theme_selected_bg_color` | `COLOR_HIGHLIGHT` |
   | `selection_bg`, `selection_text` | `[Colors:Selection]` `BackgroundNormal`, `ForegroundNormal` | `theme_selected_bg_color`, `theme_selected_fg_color` | `COLOR_HIGHLIGHT`, `COLOR_HIGHLIGHTTEXT` |
   | `error`, `warning`, `success` | `[Colors:View]` `ForegroundNegative`, `ForegroundNeutral`, `ForegroundPositive` | `error_color`, `warning_color`, `success_color` | derived |

   An accent the system names (the first column of point 3) replaces the
   scheme's, and then the selection is derived from it; a Windows
   contrast theme keeps its own. Every other token is derived in
   `theme.rs` from these (`hover`, `code_bg`, `band`, `drop_target`,
   `chart`, a map's graticule and track…). Skwir's own palettes become
   the same derivation over Skwir's base colors, so the desktop's colors
   and Skwir's take one path; their derived tokens may move slightly,
   and are compared in screenshots before and after. Kept as Skwir's in
   every case (`Kept`): the tags' colors, the rating, a find's marks, a
   map's land, water, borders and route, `stage`, `picture_backdrop`,
   the Usage map's hues and what is drawn on them, the terminal's
   sixteen colors (a terminal's palette is its emulator's, not the
   desktop's) and the code themes (`reading.code_*`).
5. **Contrast is guarded.** Text on every surface it is drawn on (`bg`,
   `surface`, `code_bg`, `zebra`, `hover`), `selection_text` on
   `selection_bg`, `on_accent` on `accent` and `error`, `warning`,
   `success` on `bg` and `surface` reach 4.5:1; `accent` on `bg` and
   `surface` reaches 3:1 (a focus ring, a switch that is on). A
   foreground that falls short is moved in lightness, its hue kept, to
   the nearest lightness that passes (`color::readable`); a ground is
   never moved, but a fill under text (the accent, the selection) gives
   way when no lightness of its text reads on it (`color::readable_on`).
   Text on grounds of middle gray, where neither black nor white reaches
   the ratio, takes the better of the two, and a debug log line names
   the token. Skwir's own palettes pass the same check, asserted by a
   test, so building it corrects the three pairs above. `on_fill`,
   which was both the text on the accent and the text on the Usage
   map's blocks, is two tokens: `on_accent`, guarded, and `block_text`,
   kept.
6. **More contrast is followed whatever the setting says** (point 7):
   it is a need, not a style. Asked for (the portal's `contrast`,
   macOS's "Increase contrast"), the guard asks 7:1 of text and 4.5:1
   of the accent, and `border` on `bg` reaches 3:1. A Windows contrast
   theme is a palette the user chose: its colors fill the scheme of
   point 4, under the same stricter guard.
7. **A switch, on by default, and the fonts' choices** (amended twice
   on 2026-10-19, see below): `ui.desktop_colors`, "Desktop colors".
   Off, Skwir's own palette and accent. The fonts are three choices of
   their own (ADR-073): `ui.font`, "Font", for the interface, and the
   reading and code fonts, each Automatic or a family installed.
   Automatic is the desktop's font while Desktop colors is on, else
   Skwir's; a family chosen is kept either way. `ui.theme` (Automatic,
   Light, Dark) chooses the look either way. A scheme is one look's:
   when `ui.theme` forces the other look, that look is Skwir's palette
   with the desktop's accent.
8. **Read off the UI thread; the first frame draws what was read last.**
   The desktop's answers are read on a thread of `skwir-platform`'s own
   (`desktop_style::follow`: the portal through zbus's blocking API, the
   zbus the media controls and gpui's portal already link) and kept in
   the cache (`skwir_core::cache`), from which the next start draws its
   first frame. An answer that differs from what is drawn replaces it
   and wakes the window. With the cache deleted, the first frames are
   Skwir's own until the answer arrives. The fonts are tokens of the
   theme (ADR-073): what was read last, resolved against the text
   system with the rest of the look, Skwir's candidates taking over when
   it does not load (`AGENTS.md`: always an explicit family), and
   resolved again when the desktop names another. Only the family is
   the desktop's: sizes follow Skwir's scales and zoom.
9. **Followed while Skwir runs.** Linux: the portal's `SettingChanged`
   (the accent, the contrast, the fonts), and a watch
   on `kdeglobals`' folder for the file Plasma replaces
   (access events dropped, as `fs/watch.rs` does); a GTK theme is read
   again when another is named, not watched. macOS and Windows: read
   again on each appearance change gpui reports and when the window is
   activated, since gpui keeps the accent's and contrast's own notices
   to itself; a gpui patch may pass them on later.

## Consequences

- GNOME: Skwir's palette with the user's accent, more contrast when
  asked, GNOME's interface, document and monospace fonts. Plasma: the
  user's color scheme (Breeze's by default), its accent and fonts. Xfce, Cinnamon, MATE and
  compositors whose portal names a GTK3 theme (the reference session's
  does): that theme's colors. macOS: Skwir's palette and font, the
  accent, more contrast. Windows: Skwir's palette and font, the accent,
  a contrast theme's colors.
- Not covered: Qt palettes outside Plasma (qt5ct, qt6ct, Kvantum); a
  GTK4-only theme (libadwaita ignores them; GNOME's accent covers it);
  a session that names no GTK theme, whose portal has no settings and
  that is not Plasma, which keeps Skwir's colors. Xfce, Cinnamon and
  MATE are read as their icon themes are, untried on those desktops
  until built.
- Dependencies: `skwir-platform` takes zbus on Linux (already in
  `Cargo.lock`, through the media controls and gpui's ashpd; its tests
  take zbus's `p2p` for a portal of their own, whose `uuid` is in the
  lock too), objc2-app-kit's `NSColor` features on macOS, the `windows`
  crate's `UI_ViewManagement` on Windows; `just deny` and `just notices`
  are run again.
- Budgets: nothing is read on the UI thread; `just idle-pss` and the
  first frame are measured before and after.
- Tests: `kdeglobals` and GTK CSS read from synthetic fixtures (colors
  by reference, the five functions, `@import`s, a missing name); the
  guard over generated palettes; Skwir's palettes pass; the switch off
  restores Skwir's tokens; a change followed. Windows' readers tried
  under Wine for what it answers, the rest compiled only and said so;
  macOS compiled only.
- Once built, `theme.rs`'s header and `docs/invariants/shell.md` state
  the derivation and the guard, and the guide the switches.
- **Steps 1 to 4 are built.** Order (`docs/PLAN.md`, "Colors and font
  from the desktop"):
  1. the derivation and the guard in `theme.rs`, Skwir's palettes
     through them, the stricter contrast;
  2. the portal on Linux: accent, contrast, font, followed; the cache;
     the switch;
  3. Plasma's color scheme;
  4. GTK3 themes;
  5. macOS's and Windows' accent and contrast, Windows' contrast themes.

## Amendments

- 2026-10-19, step 2: the font is a choice of its own. The user, asked
  whether the desktop's font should come with its colors: "make the font
  a separate choice under the same switch". So two switches (point 7),
  the font's under the colors', and `SettingSpec::under` for a setting
  that applies only while a switch is on. The font is chosen when the
  window opens (point 8), the colors and contrast follow live. The
  portal is read through zbus, not ashpd: a blocking connection on a
  thread of its own, as the media controls do, with no async runtime to
  drive. The README's screenshots turn the desktop's colors off, so they
  never wear the accent of the machine that makes them.
- 2026-10-19, ADR-073: the font is chosen among the system's. The user,
  told that point 7 offered the desktop's font or Skwir's and nothing
  else: "don't leave it out, change the ADR relevant sections to add the
  font picker". So `ui.desktop_font` (a switch under Desktop colors,
  never released) became `ui.font`, a choice of Automatic or any family
  installed, no longer under the switch: a family chosen has nothing to
  do with the desktop's colors, and only Automatic asks whether they are
  worn. With it, the desktop's document and monospace fonts are read
  for the reading and code fonts (point 3), and the fonts are tokens of
  the theme, so a change, the desktop's or the user's, shows at once
  (points 8 and 9) rather than at the next start.
- 2026-10-20, steps 3 and 4: Plasma's scheme and GTK3 themes, read as
  points 3 and 4 say, with these decisions where they said less.
  - **Where Plasma's colors are.** `kdeglobals`, the user's before the
    system's (`XDG_CONFIG_DIRS`), key by key; what they leave out comes
    from the scheme they name (`[General] ColorScheme`, its
    `color-schemes/<name>.colors` in the data folders), `BreezeLight`
    then `Breeze` when they name none: "Breeze's by default" read from
    the installed file, never written in Skwir (`kde_scheme.rs`).
  - **A GTK3 theme** (`gtk_theme.rs`) is found as GTK 3 finds it: the
    user's data folder, `~/.themes`, the system's, in the newest
    `gtk-3.N` (even N, 24 down to 0) holding a `gtk.css`. Its imports
    are followed within the theme's folder by name (`..` resolved
    lexically, so a profile's links into other packages still read);
    `resource://` is not. A name defined by its own name takes its
    definition before (adw-gtk3 writes `@define-color warning_color
    @warning_color;` after defining it; GTK itself finds a loop there);
    other loops, `currentColor` (adw-gtk3's `borders`) and names never
    defined give none, and the token is derived. A color with
    transparency is laid over `bg`, as drawn; the grounds are taken
    opaque. `GTK_THEME=Name:dark` reads `gtk-dark.css` alone.
  - **Which look.** A scheme is put in the look its colors make (its
    text lighter than its ground: dark). Automatic takes the scheme's
    look when the desktop has one look's (Plasma's; a theme without
    `gtk-dark.css`), else the appearance (point 7 said only what a forced
    look does): Xfce's dark theme is dark though no portal says so.
  - **The accent.** The portal's replaces a scheme's, its selection then
    derived, only when it differs at eight bits a channel: Plasma's
    portal says the scheme's own, whose selection is kept. A look
    without a scheme wears the portal's accent, else the scheme's.
  - **Followed.** `kdeglobals` through `fs::Settled` on its folder (its
    name alone, access dropped, writes let settle 200 ms): a thread of
    its own on Plasma only. A GTK theme is read again when the portal
    says `gtk-theme` changed; GSettings', xfconf's and `settings.ini`'s
    own changes are not followed. A reading that says what was said
    wakes nothing, and what it freed (a theme's sheets) is given back
    before the thread waits again (`scheduler::release_now`).
  - **Checked:** synthetic fixtures (references, the five functions,
    `rgb()`, `hsl()`, imports within the theme and outside it, a cycle,
    a missing name, a compiled theme, a dark variant, `kdeglobals`
    replaced and rewritten the same); the guard on six real schemes as
    read (Breeze Light and Dark, adw-gtk3 and its dark, Materia and its
    dark), in both contrasts. Arc (nixpkgs' 20221218) names none: its
    sheet is compiled. In windows (debug build, Xvfb, no portal): a
    synthetic `kdeglobals` worn, replaced by a dark one while Skwir ran
    and followed at once (the look turned dark by itself), its
    `AccentColor` on a switch; `GTK_THEME=Materia` and `adw-gtk3-dark`
    worn. Release builds on headless sway (pixman, lavapipe, the
    session's portal naming adw-gtk3), HEAD and this alternated three
    times: own share 107.5 to 107.7 MiB against 107.3 to 107.6, first
    frame 77.8 to 89.0 ms against 76.9 to 80.3 (one run of this at load
    average 15 read 102 to 399 ms).
  - **Not checked:** a Plasma, Xfce, Cinnamon or MATE session; the
    session's own display. macOS and Windows compile the scheme's types
    and read none (step 5).
