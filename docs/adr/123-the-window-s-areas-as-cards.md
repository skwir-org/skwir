# ADR-123: The window's areas as cards

Status: accepted 2026-11-07 with the user as an experiment behind a
switch (`ui.cards`, off), built on Linux; macOS and Windows compiled
only. Amends ADR-071 (two tokens of a look: `frame`, `ring`) and
ADR-119 (the frame is one of the window's grounds).

## Context

- **Asked for** (the user, 2026-11-07): a window after today's browsers
  (Firefox's Nova, Zen, Arc), with fewer separating lines, a little room
  between the two sides of a split, rounded corners on the files, and
  the split told apart more clearly. Four directions were drawn on a
  canvas (today; the files alone as islands; each pane a card; wells,
  the files sunk into today's chrome) with four ways to mark the active
  pane; the user took the recommendation, pane cards with a ring, and
  asked for it built.
- **The window told its areas apart by lines**: the sidebar's right
  edge, each bar's lower edge, the footer's upper one, the terminal's
  and the preview's edges, and between split panes a line the accent
  when it was the active pane's, on the left of the right pane only.
- **Islands need a ground under the cards**: a card stands out when the
  window around it is darker, in either look. Skwir's chrome (`surface`)
  is darker than the files (`bg`) in the light look and lighter in the
  dark one: drawn on today's grounds, dark cards would look sunk (the
  wells direction).
- **Each ground is painted once** (ADR-119): a frame painted under the
  cards would add its opacity to theirs while the window lets the
  desktop through.
- **gpui clipped content to rectangles only**: a rounded area's children
  (a terminal's cell backgrounds, a page, a film, a row) would show past
  its corners. It said which edges of a window are tiled only with
  client-side decorations, and Skwir asks for the server's.

## Decision

1. **One switch, off by default.** Settings › Appearance › Cards
   (`ui.cards`, essential): the window as cards on its frame, or as it
   was, apart by lines. Off, the window is drawn as before.
2. **The frame is a token of the look.** `frame` lies under both
   grounds, darker than the darker by `CHROME` (1.08:1), its hue kept:
   under the bars' gray in a light look, under the files in a dark one
   (`color::below`; Skwir's own: `#e8e8e8`, `#161616`). It gives way to
   the text drawn on it (the sidebar's, the tabs'): never darker than
   `text` reads on at its ratio, a hover laid on it. Three palettes'
   light looks, whose text reads only just on their own chrome
   (Solarized, Everforest, Tokyo Night), keep their frame on their
   chrome. As cards, the muted text is guarded on the frame too
   (`Theme::as_cards`); apart by lines, no token changes. `ring` is the
   accent guarded as a mark on the frame.
3. **What is a card**: each pane (its bar, its files, its timeline, its
   footer), the preview panel (its bar, header, content and dock) and
   the terminal. A card is rounded (`rounded_xl`) and keeps what it holds
   within its corners (`overflow_hidden`, gpui patch 0053: a rounded
   element that hides its overflow clips its children to its arcs). Its
   bars are bands of `surface` over it, no line under them; a pane's
   footer lies on its files' ground, its text muted. The sidebar, its
   header and the tab strip lie on the frame and paint no ground (their
   drop tint lays on it); the tab shown is a pill of the files' ground.
4. **The frame is painted once** (`elements::frame`): each card says where
   it is as it is laid out, and the frame paints its ground before them,
   at the chrome's opacity: under everything while the window is whole,
   around rounded holes where the cards are while it lets the desktop
   through (an even-odd path), so each ground keeps its own opacity.
5. **Gutters, not lines.** 8 px between cards (`frame::GUTTER`), and
   around them where the window floats. The gutter between two cards is
   the edge dragged between them (`edge::gutter`): the split, the
   terminal's top, the preview's side, each with the handle's color
   under the pointer, a double click back to the size it ships with.
   Between the sidebar and the cards there is none: the sidebar's own
   margins set it apart, and its edge stays its own.
6. **Edges flush where the window meets another.** Each edge of the
   window that meets another window or the screen's edge
   (`Window::tiling`, gpui patch 0054: Wayland's tiled states, every edge
   while maximized or on the whole screen) loses its gutter, so a tiling
   compositor's own gaps are not doubled.
7. **The pane commands act on wears the ring** while the window is
   split: 2 px of `ring` outside its card, over the gutter. It changes no
   ground under text, so no contrast is checked again for it. Apart by
   lines, the accent line stays.
8. **No shadow.** Nothing new casts one: menus and popovers keep
   `shadow_md`, toasts and dialogs `shadow_lg`.

## Consequences

- As cards, the files have 16 to 24 px less across (a split and the
  preview: two or three gutters and the margin), and as much less tall
  with the terminal open. A window tiled loses only the gutters between
  cards.
- While the window lets the desktop through, the seam where the frame's
  hole meets a card's arc lets a little of what is behind through, a
  pixel wide along the four arcs of each card: two antialiased edges
  meet there, each painted once. Whole, the frame is painted under the
  cards and there is no seam.
- **With the switch off too**, a rounded element that hides its overflow
  now keeps its content within its corners (0053), as it always asked:
  the pictures of a folder's grid tile and the board's cards, the dock's
  cover, the swatches of the color settings, the Settings dialog's
  content inside its border.
- The bars' edges still meet: the sidebar's header, the tab strip, the
  panes' rows and the preview's bar start a gutter below the window's
  top (none tiled), at the same height.
- A pane's footer, on its files' ground, lets the desktop through as the
  files do, not as the chrome does.

## Not done

- **The ring on other cards.** The terminal and the preview take the
  keyboard but not the commands a pane takes; marking them would be a
  second meaning for the ring.
- **A card's size of radius or gutter as settings**: one look, tried
  first; the tweaks of the proposal page remain there.
- **Rounded clipping of meshes** (3D models, 0033): they keep their
  rectangle; a model drawn in a card's corner would show past its arc.
- **The other systems**: Metal's shaders are written and compiled
  nowhere; Direct3D's compile with Microsoft's DXC, not under Wine
  (whose compiler takes neither gpui's shaders nor these); tiling read
  on Wayland, maximized elsewhere.

## Checked

2026-11-07, the debug build, on Xvfb (lavapipe) in both looks: one pane,
two side by side and stacked, the preview at the right and at the
bottom, the terminal, two tabs, the gutter under the pointer; the
grounds sampled while the window let the desktop through (the chrome at
70 %, the files at 60 %: the sidebar read the frame at 70 %, the files
and their footer at 60 %, nothing painted twice). On a headless sway
1.12 tiled, the cards met the window's top, right and bottom edges.
Not checked: Hyprland, a compositor's blur, macOS, Windows.
