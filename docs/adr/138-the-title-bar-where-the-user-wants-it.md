# ADR-138: The title bar where the user wants it

Status: accepted, 2026-11-15, on the user's answer (issue #11: on Fedora
45's GNOME the window had no buttons, could not be moved or sized by its
edges; they asked for a setting choosing the title bar above, on its own
row, or in skwir's top row, working on every desktop, GNOME's Wayland
included, and chose the default recommended below, the desktop's way;
they declined dragging the window from the address field's empty end,
which edits the path). Built on Linux; compiled and linted for macOS
and Windows (decision 8), never run there. Amended once more the same
day: decision 9 (X11's choices unavailable, Windows maximized). Amended the same day, the user
finding no controls in any mode on Hyprland and the bar shown a moment
before going: decisions 2 and 7; and the values named for whose bar
it is and how it looks (1), the user finding Above no name for the
system's bar. Amended again the same day, the user asking for macOS and
Windows: decision 8.

## Context

- **GNOME's compositor draws no title bar.** Wayland's `xdg-decoration`
  protocol lets a compositor draw the window's frame; Mutter does not
  implement it, nor does Weston, by design: there, an application draws
  its own title bar, as every GNOME application does in its top row (a
  header bar). gpui asks for the system's frame and, refused, falls back
  to the application's (`WindowDecorations::Client`): skwir drew none, so
  on GNOME its window had no buttons, nothing to drag, no edges to size,
  no shadow.
- **Elsewhere the system draws it**: KDE's KWin, COSMIC, sway, Hyprland
  (borders only), an X11 window manager, macOS and Windows. Some users
  there want the window's buttons in the application's top row too, as
  browsers and editors offer (Firefox's Title bar, Chrome's Use system
  title bar and borders, VS Code's `window.titleBarStyle`).
- **Skwir's top has no window row** (`docs/invariants/shell.md`): each
  area owns its top, the window's tools at the ends of whatever is
  there (`Shell::corner_at`, `Shell::toggles_at`), so the window's
  buttons have a natural place: after Split, Properties and Preview, and
  before the sidebar's toggle and More.

## Decision

1. **`ui.title_bar`**, on the Appearance page, the free desktops' only:
   Automatic, System, Compact, In the top row (named for whose bar it is
   and how it looks, amended: first Above and On a row of its own, both
   rows above the window). What the
   system is asked for follows it: its frame for Automatic and System,
   none for the other two (`TitleBar::asks`), asked again as the
   setting moves, on the open window (`Window::request_decorations`).
2. **What skwir draws follows what the system answered**
   (`title_bar::drawn`): where it frames the window, Automatic and System
   draw nothing of skwir's. Asked for no frame, a system may keep its own
   (amended): an X11 window manager does where no compositor runs (gpui
   falls back to it), with its title bar, and skwir draws none; a Wayland
   compositor does whatever is asked (Hyprland: borders, no title bar),
   and Wayland cannot say whether its frame has a title bar, so the two
   values that ask for skwir's (Compact, In the top row) draw
   it inside that frame, without skwir's margin, shadow, corners or
   edges; should that frame have a title bar too (Hyprland's hyprbars),
   Automatic gives the one. Where it draws none,
   the setting says which of skwir's: System, a bar like a desktop's above
   the window (its own ground, a line under it, the title in its middle);
   Compact, a slim band on the frame above the areas; In the
   top row, the window's buttons at the ends of the row that holds the
   window's tools there. Fullscreen, nothing.
3. **Automatic is the desktop's way**: the system's title bar where it
   draws one, the top row where it does not, as GNOME's applications have
   it. Beside the setting, what it comes to once a window knows
   (`automatic_now`).
4. **The buttons are the desktop's**: its layout (`App::button_layout`,
   GNOME's `button-layout` through the portal: Close alone on Fedora;
   gpui's minimize, maximize, close where none is read), only those the
   compositor offers (`Window::window_controls`: sway offers neither
   minimize nor maximize). Round, on a ground of their own
   (`Theme::control`, `control_hover`), as GNOME's; Maximize says Restore
   while the window is maximized. Each runs a command
   (`ui.close_window`, `ui.maximize_window`, `ui.minimize_window`), which
   the palette and agents have too; not a stop of the keyboard's.
5. **The empty parts move the window**: of the bar, of the rows that hold
   the window's buttons, and the frame's gutter above the cards. One
   element under what a row holds (`elements::window_drag`) acts only
   where nothing else is under the pointer (`Hitbox::is_frontmost`, gpui
   patch 0063), so a button, the address field and a tab keep their
   press. Pressed and moved, the window moves (from the first motion, so
   a click moves nothing); pressed twice, it is maximized or restored,
   except beside the tabs, where a double click opens a tab as before;
   the right button opens the system's menu for the window. The address
   field's empty end keeps editing the path.
6. **The frame**: a margin of 12 px (`title_bar::MARGIN`) around the
   window on the edges that meet no other window or the screen's
   (`Window::tiling`), where it casts its shadow (`Theme::window_shadow`,
   none while what is behind shows through) and where its edges and
   corners are pressed to size it; its corners rounded as a card's,
   what it holds kept within them, a card at a window's corner rounded as
   the window is (gpui patch 0064). What is sized or placed by the
   window's size reads its body within the margin (`title_bar::body`),
   never the viewport: the arrangement, the panels' limits and edges.

7. **Shown as answered** (amended): the window keeps what it shows until
   the compositor answers what was asked (gpui patch 0065,
   `Window::decorations_pending`), so a refusal shows nothing for a
   frame. What a refusal means here is said beside the setting once seen,
   for the run (`title_bar::refusal`): the bar drawn inside the desktop's
   frame, or none where a window manager keeps its title bar. No choice is
   disabled: the settings have no unavailable choice, and none can be
   known before it is asked.

8. **macOS and Windows** (amended): the setting on every system. Asked
   for none, macOS's title bar is made transparent, the content under it,
   its traffic lights the system's, put in the middle of skwir's first row
   (`title_bar::lights`) with room left for them at its start; Windows'
   is hidden, its frame, shadow and edges kept, and skwir draws Windows'
   caption buttons (minimize, maximize, close, square, Close red under the
   pointer: `Theme::caption_close`), told to Windows as such
   (`WindowControlArea`), which presses them itself and shows Snap Layouts
   over Maximize; the empty parts are told to it as caption, so it moves,
   snaps and maximizes the window and opens its menu; beside the tabs,
   where a double click opens a tab, they are not. Skwir draws its own
   frame on the free desktops alone (`DRAWS_FRAME`). The window is made
   as the setting asks and changed on an open window (gpui patch 0066).
   macOS's double click does what its settings say
   (`titlebar_double_click`), and a press there starts the move at once.

9. **Said before asked where it can be** (amended): where X11 runs no
   compositor, gpui knows before anything is asked that the application's
   decorations cannot be had (`Window::client_decorations_supported`,
   gpui patch 0067): Compact and In the top row are offered as choices
   that do nothing here, with why (`settings.md`, "A choice that does
   nothing here"), the refusal note kept for Wayland. And on Windows, a
   maximized window's decorations meet the screen's edges
   (`Decorations::Client`'s tiling, gpui patch 0066 amended): no gutter
   above the cards, the caption buttons flush with the window's top and
   right edges, the corner Close's.

## Consequences

- On GNOME, skwir's window has its buttons, moves and sizes as GNOME's
  own applications do, in Automatic. On KDE, sway, Hyprland, X11, macOS
  and Windows, Automatic changes nothing.
- The top of the window has a row of its own when the user asks for one
  (System where skwir draws it, Compact): `shell.md`'s "no window row" holds for
  the rest.
- **Checked** on headless Weston (no `xdg-decoration`, as Mutter): each
  value drawn, as cards and with lines, the corners rounded, the
  shadow; on headless sway: Automatic leaves the bar to sway, the top
  row gets the application's frame (`csd`), switched both ways on the
  open window; a drag on an empty part moved the window, a press on
  Split split it without moving it, Close closed it. **Not checked**:
  GNOME's Mutter itself and its portal's button layout (the reporter's
  Fedora 45 confirms), KWin, X11 with a compositor, the right button's
  menu, double-click.
- **macOS and Windows** (decision 8) are compiled and linted only: a gpui
  window cannot be drawn under Wine, and no Mac is at hand. Not seen: the
  lights' place, the caption buttons, Snap Layouts, a switch on an open
  window (macOS's style mask changed after it opened).
- Popups keep 8 px from the viewport's edges, not the body's: one near
  an edge may lie up to 4 px over the margin.
