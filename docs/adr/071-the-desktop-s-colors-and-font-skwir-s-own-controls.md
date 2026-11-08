# ADR-071: The desktop's colors and font, Skwir's own controls

Status: accepted, 2026-10-19. Steps 1 and 2 built 2026-10-19: the
derivation and the guard, Skwir's palettes through them; Linux's portal
read and followed (the accent, more contrast, the font). Steps 3 and 4
built 2026-10-20: Plasma's scheme, followed, and GTK3 themes' colors.
Step 5 built 2026-11-05: macOS's colors (point 2 amended for them) and
Windows' accent and contrast themes. Amended 2026-10-19 (two switches,
the font at the window's opening), again by ADR-073 (the font a choice
among the system's, three of them, followed live), 2026-10-20 (steps
3 and 4) and 2026-10-27 (the accent as text, the chrome apart, the hover
laid over), and 2026-11-05 (Colors from: Automatic, Skwir's own, GTK's
or Qt's, in place of the Desktop colors switch; Qt's palettes read;
then Skwir's palettes, twelve, among its values; then step 5, and
macOS's own colors read), and by ADR-119 (2026-11-06: the window's
grounds may let the desktop through; the contrast guarded is on the
theme's own grounds, and the window stays whole while more contrast is
asked); 2026-11-07 (the user's own GTK sheet over the theme, the Qt
platform themes after qt6ct, Omarchy's theme: the desktop shells of
Hyprland and its kind); 2026-11-08 (a GTK sheet's own sidebar color the
frame the cards lie on, ADR-123; the files followed before they are
read).

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
   `surface`, `code_bg`, `zebra`, `hover`; `accent_text` there and on
   `term`, amended 2026-10-27), `selection_text` on
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
   on 2026-10-19, see below; the switch a choice since 2026-11-05, at
   the end): `ui.desktop_colors`, "Desktop colors".
   Off, Skwir's own palette and accent. The fonts are three choices of
   their own (ADR-073): `ui.font`, "Font", for the interface, and the
   reading and code fonts, each Automatic or a family installed.
   Automatic is the desktop's font while Desktop colors is on, else
   Skwir's (since ADR-117, as `ui.fonts_from` says, Desktop colors being
   the colors alone); a family chosen is kept either way. `ui.theme` (Automatic,
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
- 2026-10-27: the accent as text, the chrome apart, the hover laid over.
  The user, of the address field: "the place pill at the beginning of
  the bar, and the folder / subfolder selected highlight have colors
  with not enough contrast which makes the text tiring to read", and of
  the window: "a bit more contrast between sidebar … and file view
  background … a similar contrast as Obsidian does".
  - **The accent written as text is a token of its own**, `accent_text`:
    the accent guarded as text is (4.5:1, 7:1 for more contrast) on every
    ground text is drawn on and on `term` laid on `bg` and `surface`.
    `accent` stays a mark and a fill, at 3:1. The pill and the chosen
    depth were the accent on `term`: 3.06:1 in adw-gtk3's light look,
    3.5:1 in its dark one and in Skwir's light one. Every text in the
    accent takes it (a term's key, a link, a toast's action, Properties'
    actions, the status bar's proposals, the preview's trail pointed at);
    a label on a link keeps the accent as its fill (`label_fill`).
  - **The chrome stands apart from what is read.** Whatever a scheme
    says, `surface` reads apart from `bg` at 1.08:1 at least (`CHROME`);
    a scheme closer than that has its `surface` moved in lightness, its
    hue kept, as a foreground is. Obsidian's page and sidebar stand at
    1.08 (its documented base colors `--color-base-00` and `-20`,
    #ffffff and #f6f6f6, #1e1e1e and #262626; that the sidebar takes
    `-20` is remembered, not read in its documentation), Skwir's own
    looks at 1.09, so they keep their colors. A GTK theme after
    libadwaita's (adw-gtk3) names its window's color as light as its
    view's (#fafafb on #ffffff, 1.02:1): it becomes #f6f6f7, and
    #242428 on #1d1d20 dark. Its sidebar's own color
    (`sidebar_bg_color`, #ebebed; #2e2e32 dark, 1.19:1 and 1.24:1) was
    worn first; "it's too strong", the user said, so the chrome stays
    the window's, set apart. As cards (ADR-123, amended 2026-11-08),
    the frame is the sidebar's own color where it lies under `bg`, else
    in a light look this chrome's gray, and a card's bars lie between
    it and `bg`, 1.07:1 under `bg` (amended 2026-11-08, the
    sidebar's color).
  - **`hover` is laid over what it is on**, translucent: the way from
    `bg` to its step carried to the edge of sRGB, at the opacity that
    shows the step on `bg` (`color::overlay`), so on the darker chrome
    it darkens by as much; an opaque one, a step from `bg`, showed less
    there, and nothing on a chrome as dark as it. Text is guarded on it laid on `bg` and on
    `surface`; Skwir's dark `text_muted` moved from #9a9a9a to #9e9e9e
    for it. Where it must hide what is under it (Properties' actions
    over a row's words) it is laid on its ground first.
- 2026-10-30, ADR-007's soft tiles: the Usage map's fills are pale or deep
  tints of eight hues of Skwir's own, still kept, and what is written on
  them is the look's own dark or light text (`block_text`), no longer
  white. The ring around a chosen block is drawn on the ground around
  it, so it is the accent, which is guarded at 3:1 there.
- 2026-11-01: where the desktop asks for more contrast, the map's fills
  move further from that text, their hues kept, so it reads at 7:1 as
  all text then does (`Theme::blocks` by look and contrast).
- 2026-11-05, **Colors from** (the user's choice, for consistency with
  ADR-117's Fonts from): `ui.colors_from`, Automatic, Skwir's own, GTK
  or Qt, replaces `ui.desktop_colors`. Automatic reads the desktop's own
  settings (Plasma's scheme, LXQt's palette, the GTK3 theme GNOME's,
  MATE's, Cinnamon's or Xfce's settings name), else GTK's theme (in
  GTK's order: `GTK_THEME`, then on Wayland the portal's `gtk-theme`
  before `settings.ini`, after it on X11), else Qt's (qt6ct's or
  qt5ct's scheme, as `QT_QPA_PLATFORMTHEME` names it, while its custom
  palette is on: its own `[ColorScheme]` or a KDE `.colors` file); GTK
  or Qt wears that toolkit's alone (`DesktopStyle::gtk_colors`,
  `qt_colors`, `colors_from`). Qt's are Plasma's on Plasma and LXQt's
  palette on LXQt, as their applications are given. LXQt's palette
  (`[Palette]` of `lxqt.conf`, read as `lxqt-qtplugin` does) counts once
  it names the base, the window and a text color; Qt derives the rest
  from the window's color alone, which names no scheme. A GTK3 theme is
  read once a reading however many sources name it. Someone who had
  turned Desktop colors off keeps Skwir's own colors (and fonts,
  ADR-117); the switch's key is forgotten and named Colors from's for
  `Settings::check`. More contrast is followed whatever is chosen. The
  accent rule of point 4 is unchanged for the scheme chosen. Not
  followed: a qt5ct or qt6ct scheme file edited in place (its
  configuration is followed), a Kvantum theme's colors; macOS and
  Windows still say no colors, so the setting is Linux's.
- 2026-11-05, **Skwir's palettes.** The user asked whether, when Skwir's
  own colors are chosen, a set of color themes could be offered, "like
  Brave". A proposal page drew candidates through a port of
  `Theme::derive` and its guard; the user took its recommendations and
  asked for all twelve at once, with the code themes they need. Colors
  from now offers, after Automatic, GTK and Qt, Skwir's palettes
  (`palettes.rs`): **Skwir** (what Skwir's own was; its value `skwir`
  is kept, so no settings file changes meaning), **Paper** (warm),
  **Slate** (cool), **Black** (white and true black), then the
  community's: **Solarized**, **Nord**, **Gruvbox**, **Catppuccin**
  (Latte, Mocha), **Dracula** (Alucard, the classic), **Rosé Pine**
  (Dawn, main), **Tokyo Night** (Day, Night) and **Everforest**
  (medium).
  - **A palette is a `Base` a look.** Every other token is derived and
    guarded as a desktop's scheme is; a palette names no token of its
    own, only its secondary text or selection where its makers design one
    for that use. The colors and roles are their makers' (checked
    2026-11-05 against each project's own published palette: Rosé Pine
    Dawn's text is the darker one it took in 2025, Nord's warning nord13
    as `nord.css` says, Tokyo Night's errors its `error` role).
  - **Point 2 holds**: never a platform's palette (Adwaita's, macOS's,
    Fluent's) nor a company's colors. A community palette is one its
    makers publish for every application, under a permissive licence;
    the packages' notices credit them.
  - **A palette keeps its own accent**: the desktop's is not worn, as
    Skwir's own colors wore none. More contrast is followed.
  - **Both looks.** Each has a light and a dark look (Dracula's spec now
    has Alucard). One with a single look would be a scheme of one look's
    (amendment of 2026-10-20): Automatic would take it, the other look
    forced being Skwir's own with its accent; a test holds that.
  - **Beyond the `Base`** (amending point 4's `Kept` for these): a
    community palette brings its terminal's sixteen colors, from its
    makers' own ports (Solarized's one set for both looks; Catppuccin's
    from `palette.json`; Nord's light look has none, so it keeps
    `Kept`'s), and its code theme, which code is drawn in while the
    Reading page says Default. bat ships Solarized's, Nord's (dark),
    Gruvbox's, Catppuccin's and Dracula's; Skwir carries Rosé Pine's
    (MIT), Tokyo Night's (Apache-2.0: the Day file's colors its
    generator left from Night, unreadable at 1.04 to 1.9:1, replaced by
    their Day counterparts, as the file says) and Everforest's (MIT, the
    only medium-contrast TextMate port, archived), in
    `crates/platform/assets/code-themes` (`highlight::CARRIED`), offered
    on the Reading page beside bat's. Nord's light look and Alucard have
    no code theme: Default is Skwir's there. Tags, ratings, find marks,
    maps and the Usage map stay `Kept`.
  - **The guard binds.** Solarized's light text (4.13:1 on its ground,
    3.46:1 on the chrome under a hover) is drawn a little darker; light
    palettes' yellows and greens darken the most. A test bounds how far
    the guard moves a palette's chrome, text and accent at normal
    contrast (0.07 of Oklab's lightness; Solarized's text moves 0.063),
    so a palette that would stop looking like itself fails.
  - **Every system.** Colors from is declared on macOS and Windows too,
    where Automatic is the Skwir palette until their colors are read
    (step 5).
  - **The dialog draws what each looks like.** A choice whose owner
    gives each value a swatch (`SettingConstraints::swatches`, data an
    agent sees as a plain choice) draws, before each value's name in its
    list and before the chosen one's on its control, a window in
    miniature in the colors it would give now, its look as the theme and
    the system make it. A gallery of cards under the row was built first
    and taken back the same day: the user found it took too much room.
    Then, asked whether to, the window wears the palette highlighted in
    the list while it is open, as VS Code's and Zed's theme pickers do:
    never saved, gone when the list closes without taking it, kept when
    taken until its save ends (`settings::previewed`).
  - **Checked:** the palettes through the guard in both contrasts and
    both looks (`every_palette_reads_in_either_contrast`), the bound
    above, each code theme offered for its look, the carried themes
    read and highlight, a palette worn whatever the desktop says (or
    with nothing said), the pictures drawn as worn. In a window: see
    `docs/STATUS-LOG.md`, 2026-11-05.
- 2026-11-05, **step 5, and macOS's own colors.** Asked whether Skwir
  could read the system's colors on macOS and Windows, and told that
  step 5 (the accent, more contrast, Windows' contrast themes) was
  accepted but not built, that macOS names far more (the colors AppKit
  draws its own windows in), which point 2 declined as "macOS's window
  backgrounds", and that Windows names no such colors, the user: "go
  ahead with step 5 and amending ADR-071".
  - **Point 2, amended for macOS.** Copying a platform's palette into
    Skwir stays declined. Reading what the system says at run time is
    harnessing the environment (ADR-000, 4), as Plasma's scheme is read
    though its user may never have chosen it: on macOS, Automatic reads
    AppKit's semantic colors, each look's as its appearance resolves
    them (`NSAppearance.performAsCurrentDrawingAppearance`): a list's
    ground (`controlBackgroundColor`), the window's as the chrome
    (`windowBackgroundColor`), labels and secondary labels, separators,
    the second alternating row, the accent (`controlAccentColor`), the
    selection (`selectedContentBackgroundColor`, which follows the
    accent) and its text, and the system's red, orange and green. Their
    translucent labels and lines are laid over the ground, as drawn.
    With Increase contrast on, the accessibility high-contrast
    appearances are read, and the stricter guard applies (point 6).
    Nothing is written in Skwir: a later macOS that changes them changes
    Skwir with it. Fluent's colors stay out on Windows, where no system
    call names them (they are WinUI's resources, not the system's).
  - **Windows** (point 3's table): the accent from `UISettings`
    (`UIColorType::Accent`, as gpui reads the look; WinRT on the reading
    thread's multithreaded apartment), worn on Skwir's colors; a contrast
    theme (`SPI_GETHIGHCONTRAST`) read whole from `GetSysColor` as point
    4's table says, keeping its own accent, under the stricter guard.
  - **Followed.** Windows: the registry keys where the accent, the
    contrast theme and its colors are kept, watched beside the text
    settings' (`windows::style::follow`, no timer). macOS: AppKit tells
    only its own windows of a change, so what it says is read again on a
    worker when the window's look changes or it is activated
    (`desktop_style::FOLLOWED`, `read_again`), as point 9 planned.
  - Colors from's Automatic says "macOS" or "Windows" while it wears a
    scheme read there; each system's description says what it reads.
  - **Checked:** the mapping of AppKit's colors (Apple's documented values
    for both appearances) and of a contrast theme's (Windows 11's Aquatic
    and Desert) into schemes, on Linux; under Wine, the reading itself
    (its blue accent, `#0078d7`, no contrast theme, its system colors);
    Windows and macOS compiled and linted. Not checked: any real Mac or
    Windows machine (`docs/HELP-WANTED.md`, Colors).
- 2026-11-05, **reviewed beside 0.3.1.** The portal is asked, at each
  reading, on a connection of the reading's own with a call timeout (5 s;
  the shared one has none), and a portal that does not answer is not
  asked again in that reading; it is read again when the portal starts
  (`NameOwnerChanged`), as at a login where it comes late. A source that
  names nothing (qt6ct with its custom palette off, LXQt's window color
  alone) is never said to be where the colors come from. Plasma and
  LXQt are read once a reading, their scheme being Qt's too. macOS's
  readings, which overlap as activations ask them, are worn in the order
  asked, and the cache is written only when what is said changed.
  Windows' keys are watched before its first reading. Colors from's
  palette taken from its list is worn while the save that carries it
  waits or runs, and no longer once it ends (failed, refused, or
  another surface's since), the dialog open or not; taking the palette
  held sends nothing and waits for nothing; a list's highlight going
  leaves a taken palette worn; no list is hidden while its preview stays
  (`docs/invariants/settings.md`). Swatches are made again only when the
  desktop's colors, the system's look or `ui.theme` change.
- 2026-11-07, **the desktop shells.** The user asked how Omarchy and the
  shells drawn over Hyprland and its kind (Noctalia, Caelestia, Dux
  Shell, which was DankMaterialShell) say their colors, whether Skwir
  took them as it takes GTK's and Qt's, and whether it could; then, told
  what each writes, "go ahead with 1-3". What each writes, read in their
  sources that day:

  | | GTK | Qt | Its own |
  |---|---|---|---|
  | Noctalia | `gtk-{3,4}.0/noctalia.css`, imported at the end of `gtk.css`; adw-gtk3 named | qt5ct's and qt6ct's `colors/noctalia.conf`; a KDE scheme applied to `kdeglobals` | its own |
  | Caelestia | `gtk-{3,4}.0/gtk.css` written whole; adw-gtk3-dark named, whatever the look | qtengine (`qtengine/config.json` naming its `.colors`); qt5ct and qt6ct before | `scheme.json` |
  | Dux Shell | `gtk-{3,4}.0/dank-colors.css`, linked or imported from `gtk.css`; adw-gtk3 named | qt5ct's and qt6ct's `colors/matugen.conf`; a KDE scheme where Plasma's tool is there | `dms-colors.json` |
  | Omarchy | none: GTK's built-in Adwaita named | none | `colors.toml` |

  The look reached Skwir already (the portal's `color-scheme`, which each
  sets), and on the reference session Noctalia's accent (its portal is
  Plasma's, which reads the `kdeglobals` Noctalia writes). Their colors
  did not: Skwir read the theme GTK's settings name (adw-gtk3's own
  grays), never the sheet laid over it, and Qt's platform themes but
  qt5ct and qt6ct. So, reading where the shells write for every
  application rather than their own palettes, which three formats each
  moving (Dux Shell renamed itself that month; Caelestia moved its Qt
  colors to qtengine) would tie Skwir to:
  1. **The user's own GTK sheet** (`gtk_theme::UserSheet`): GTK 3's
     `gtk.css`, else GTK 4's (libadwaita's) where GTK 3's defines no
     color, laid over the theme's sheets as GTK lays a sheet of higher
     priority, or read alone where no theme read is named (GTK's
     built-in Adwaita). A name's last definition is the one looked up,
     so a theme naming its colors by reference (adw-gtk3's
     `theme_base_color` is `@view_bg_color`) takes the user's. Its
     imports are read within the home folder. The user's sheet is one
     look's: laid over a theme's light and dark sheets, both come out
     the same look, and that look keeps the first. Point 4's GTK column
     takes libadwaita's names where GTK 3's are not named
     (`view_bg_color`, `window_bg_color`, `view_fg_color`), its accent
     libadwaita's `accent_bg_color` before `theme_selected_bg_color`
     (adw-gtk3's are one color), and a selection only as a ground and
     its text named together (Caelestia names a tint of its accent with
     a text it never defines: the selection is derived from the
     accent). On the reference session Automatic went from adw-gtk3's
     `#ffffff`, `#fafafb` and `#3584e4` to Noctalia's `#faf9f9` and
     `#416165`.
  2. **The Qt platform themes after qt6ct** (`desktop_settings::
     qt_settings`): qtengine (`QTENGINE_CONFIG`, then `qtengine/
     config.json` in the configuration folders: `theme.colorScheme`, `~`
     the home folder, and its fonts, `theme.font` and `theme.fontFixed`)
     and hyprqt6engine (`hypr/hyprqt6engine.conf` there and in
     `/etc/xdg`: `theme:color_scheme`, `theme:font`, `theme:font_size`,
     `theme:font_fixed`, `theme:font_fixed_size`, read as hyprlang
     writes them, its variables and `source`d files not followed), each
     where the program looks, as `QT_QPA_PLATFORMTHEME` names it. Both
     name a KDE scheme, read as qt6ct's `.colors` is. A font they leave
     out is unnamed: the program's own default is no choice of the
     user's.
  3. **Omarchy's theme** (`omarchy`): where Omarchy's session runs
     (`OMARCHY_PATH`, which it exports to every program; a state folder
     left by an Omarchy no longer run says nothing then), the desktop's
     own scheme, as Plasma's and LXQt's: `colors.toml` of
     `~/.local/state/omarchy/current/theme`, else of `~/.config/
     omarchy/current/theme` (where it was until 2026), read as Omarchy
     reads it (a line a key, its value quoted or not; `bg`, `fg` and the
     terminal's `color0`… where the newer names are missing).
     `background` is `bg`, `dark_background` `surface` (else the
     ground, which the theme sets apart), `foreground` `text`,
     `dark_foreground` `text_muted`, `muted` `border`, `accent`, `red`,
     `yellow`, `green` the outcomes. Its `selection` is not read: it is
     what a terminal or an editor marks text with (Omarchy's own VS Code
     theme marks a list's chosen rows with the accent), so the selection
     is derived from the accent. Colors from's Automatic says
     "Automatic (Omarchy)".
  - **Followed as written** (point 9): the files a reading names are
    asked again before each reading (`Own::followed`; after it until
    2026-11-08, when a write between the two was lost), so what a file
    names now is followed where it is: a shell rewrites the file it
    imports (`noctalia.css`, `dank-colors.css`) or the scheme its Qt
    settings name, not `gtk.css` or those settings. Followed: both
    `gtk.css` and what the one read imports, the platform theme's
    settings and the scheme they name, Omarchy's `current` folder (its
    `theme`, removed and the next renamed to it).
  - **Checked:** the readers on fixtures written as each shell writes
    (synthetic colors); the following on a scheme named anew and on
    Omarchy's swap; on Xvfb, with no portal, a test window wearing a
    sheet imported from `gtk.css` (`#f7f1e3`, its green accent) and
    another once only the imported file was written again (`#eef2fb`),
    then Omarchy's dark theme (`#1d1a17`) and its light one put in its
    place (`#eff1f5`); on the reference session, a reading through the
    platform (Noctalia's colors, above). Not checked: a real Caelestia,
    Dux Shell or Omarchy session, qtengine and hyprqt6engine installed.
- 2026-11-08, **a sheet's own sidebar color, the cards' frame.** With
  the cards made lighter (ADR-123, "Lighter"), the frame of a light look
  was the chrome's gray: a GTK sheet's window color, set apart from its
  view by `CHROME`. The user asked to close the gap the shells leave
  there and see the difference.
  - **The gap:** a sheet after libadwaita's names its sidebar apart from
    its window (`sidebar_bg_color`). Noctalia's names its window as
    light as its view (`#faf9f9`) and its sidebar `#eeeeed`, as Dolphin's
    window lies under its view; that color was never read, so the frame
    was the window's color set apart, `#f1f0f0`.
  - **Decided:** `sidebar_bg_color` is read from GTK sheets (the
    theme's, the user's laid over it, `gtk_theme::Sheet::scheme`) as a
    scheme's `sidebar`, carried into the look's `Base`. It is the frame
    the cards lie on where it lies under what is read, far enough
    under it for the bars between (ADR-123's `ROOM`, 1.097:1), giving
    way to the text drawn on it as the frame does; a card's bars lie
    between it and the files. The chrome stays
    the window's (2026-10-27): the bars of the look apart by lines, the
    sidebar there, menus and dialogs keep `surface`. "Too strong" was
    the sidebar's color worn as the whole chrome; here it is only the
    ground the cards lie on. A dark look's sidebar, lighter than its
    view (adw-gtk3-dark's `#2e2e32` on `#1d1d20`), is not the frame: a
    card stands above the ground it lies on, so the frame stays a step
    under the files.
  - **Consequences:** with Noctalia's sheet the frame is its sidebar's
    `#eeeeed` (1.105:1 under the files), Dolphin's window gray, the
    bars 1.07:1 under the files, about `#f2f1f1`, each card outlined in
    the border derived from it, `#d7d7d7` (ADR-123, "Lighter": a darker
    frame was tried for the header's edge and set aside for the
    outline). adw-gtk3's own sheet
    names `#ebebed` under `#ffffff`: its light frame is that, 1.19:1
    under the files, where it was `#f6f6f7`, as GNOME's own
    applications draw their sidebars; its bars about `#f7f7f7`. Plasma's
    scheme, Qt's palettes, Omarchy's theme, macOS and Windows name no
    sidebar: unchanged. Skwir's palettes name none either.
  - **Also:** the files a reading names are followed before it is
    read, no longer after. A test of a scheme named anew failed about
    one run in three under load: the next write to it came between the
    reading and the watch, and was lost. A shell naming a scheme and
    then writing it could lose the same write.
  - **Checked:** the sidebar read from a theme and taken from the user's
    sheet over it, a header bar's color not taken for it; the frame from
    a sidebar (Noctalia's, adw-gtk3's), moved to a step where nearer, a
    dark look's not taken, nothing else moved apart by lines; 400
    random schemes, a third with a sidebar, whose text and muted text
    read on every ground of the cards where any color could; the
    following test passed five combined runs of the UI's and the
    platform's tests (it had failed two of four). Not checked in a
    window: the user checks on their own desktop.
