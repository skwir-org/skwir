# ADR-123: The window's areas as cards

Status: accepted 2026-11-07 with the user as an experiment behind a
switch (`ui.cards`, off), built on Linux; macOS and Windows compiled
only; amended the same evening as tried (Amendments). Amends ADR-071
(tokens of a look: `frame`, `sunken_bg`, `sunken_surface`) and ADR-119
(the frame is one of the window's grounds).

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
   chrome. As cards (`Theme::as_cards`), the muted text is guarded on
   the frame too, and a split's other pane has grounds of its own,
   `sunken_bg` and `sunken_surface`: two fifths of the way towards the
   frame, as far as its text still reads. Apart by lines, no token
   changes.
3. **What is a card**: each pane (its row, its files, its timeline, its
   footer), each part of the preview panel (Properties with the header,
   the preview with its toolbar, a container's kept rows and the member
   it shows, the pinned player) and the terminal. A card keeps what it holds within its corners
   (`frame::card`, a rounded content mask: gpui patch 0053). Its row and
   its footer are bands of `surface` over it, no line between them and
   the files. The sidebar, its header and the tab strip lie on the frame
   and paint no ground (their drop tint lays on it); the tab shown is a
   pill of the files' ground.
4. **A card runs to the window's edge** wherever it meets it: the panes
   and the terminal at the bottom, the preview at the bottom and the
   right, the panes at the right without the preview and at the left
   without the sidebar. Only the top keeps a gutter, the frame's own row
   (the tabs, the sidebar's header), the cards hanging under it as a
   browser's page under its toolbar, unless the window meets another
   there (`Window::tiling`, gpui patch 0054: Wayland's tiled states,
   every edge while maximized or on the whole screen). A corner is round
   (`rounded_xl`) where the card stands apart, square where either of its
   edges is on the window's, which the window's own corners round; each
   card finds which from where it is laid out, in any arrangement.
5. **The window's buttons are never a card's.** Split, Properties and
   Preview, and the sidebar's toggle and More when the sidebar is put
   away, lie on the frame. At the end of the tab strip they are on it
   already; above the preview at the right, its bar is a row of the
   frame, its card starting under it, level with the files; otherwise
   they are a notch in the pane's row that holds them (`frame::notch`):
   the row's band ends before them with a rounded corner, and its edge
   comes down to the files with a concave one, whose fillet is painted in
   the band's ground.
6. **The frame is painted once** (`elements::frame`): each card says where
   it is as it is laid out, its notches as they are, and the frame paints
   its ground before them, at the chrome's opacity: under everything
   while the window is whole, around holes the cards' shapes while it
   lets the desktop through (an even-odd path), so each ground keeps its
   own opacity.
7. **Gutters, not lines**: 6 px (`frame::GUTTER`), between cards and at
   the top, an exception to the theme's multiples of 4 by the user's
   choice. The gutter between two cards is the edge dragged between them
   (`edge::gutter`): the split, the terminal's top, the preview's side,
   each with the handle's color under the pointer, a double click back
   to the size it ships with, measured from the gutter's middle. Between
   the sidebar and the cards there is none: the sidebar's own margins set
   it apart, and its edge stays its own.
8. **The pane commands act on** while the window is split: its row
   underlined in the accent (2 px along the band's foot), the other
   pane's grounds a step down (`sunken_bg`, `sunken_surface`: its files,
   row, Quick filters, banner, timeline and footer). Apart by lines, the
   accent line stays.
9. **No shadow.** Nothing new casts one: menus and popovers keep
   `shadow_md`, toasts and dialogs `shadow_lg`.

## Consequences

- As cards, the files lose only the gutters: 6 px between the panes, 6
  between them and the preview or the terminal, and 6 at the top.
- While the window lets the desktop through, the frame's curves beside a
  card's rounded corners and a notch's are the exact complement of the
  card's, told how opaque the card is there (gpui patch 0055,
  `Window::paint_around`): no seam where they meet. Its straight edges
  are a path on whole device pixels, as quads are drawn.
- **With the switch off too**, a rounded element that hides its overflow
  now keeps its content within its corners (0053), as it always asked:
  the pictures of a folder's grid tile and the board's cards, the dock's
  cover, the swatches of the color settings, the Settings dialog's
  content inside its border.
- The bars' edges still meet: the sidebar's header, the tab strip, the
  panes' rows and the preview's row of buttons start a gutter below the
  window's top (none tiled), at the same height.
- The active pane keeps its grounds: a window not split is drawn as it
  is; only the other pane of a split steps down.

## Not done

- **A mark on other cards.** The terminal and the preview take the
  keyboard but not the commands a pane takes; marking them would give
  the mark a second meaning.
- **A card's radius or gutter as settings**: one look, tried first; the
  tweaks of the proposal page remain there.
- **Rounded clipping of meshes** (3D models, 0033): they keep their
  rectangle; a model drawn in a card's corner would show past its arc.
- **The other systems**: Metal's shaders are written and compiled
  nowhere; Direct3D's compile with Microsoft's DXC, not under Wine
  (whose compiler takes neither gpui's shaders nor these); tiling read
  on Wayland, maximized elsewhere.

## Checked

2026-11-07, the debug build, on Xvfb (lavapipe) in both looks: one pane,
two side by side and stacked, the preview at the right and at the
bottom, the terminal, two tabs, the gutters dragged (the split's and the
terminal's followed the pointer to half a pixel), the notches with the
sidebar shown and put away, the active pane on either side; the grounds
sampled while the window let the desktop through (the chrome at 70 %,
the files at 60 %: the frame, the bars, the files, the footers and a
notch's fillet each read their ground once). On a headless sway 1.12
tiled, the first build's cards met the window's top, right and bottom
edges. The user tried each step on their own desktop. Not checked: a
compositor's blur, macOS, Windows.

## Amendments

### 2026-11-07: as tried

The first build (f0998011) kept a gutter of 8 px all round the cards,
dropped where the window was tiled, ringed the active pane in the accent
(2 px outside its card), laid the footers on the files' ground and put
Split, Properties and Preview in the rightmost pane's row and the
preview's bar, inside the cards. Tried by the user the same evening:

- the cards run to the window's edge but at the top (point 4), the
  user's proposal, as browsers draw their page under the toolbar;
- the gutter is 6 px;
- the window's buttons lie on the frame, in notches or above the
  preview (point 5), the user's sketch;
- the active pane is underlined, the other lowered (point 8), after
  trying raised and underlined with a shadow on the frame, whose shadow
  the user had removed;
- the footers are bands, as the rows are.

### 2026-11-07: the preview panel's parts as cards

- **Asked** (the user): Properties and the preview as two cards, the
  design's language; then the pinned player too.
- **Decided:** what the panel holds is cards of their own, each pair
  apart by a gutter that is the edge dragged between them, measured from
  its middle (`preview::cards_under`): Properties with the header heading
  it, over the preview (beside it with the panel at the bottom, the
  header then as narrow as its column); the preview with its toolbar; a
  container's kept rows and its member's preview (`members::split`), the
  header heading the rows while Properties is off; the pinned player
  (`dock`) under the gutter that is its edge. The header heads the first
  card, whichever it is. With nothing to preview, Properties as tall as
  it says and the reason under it, apart by a gutter alone. Each says how
  opaque it is at its corners: a header, a toolbar or a bar is the
  chrome's, what it shows the preview's.
- **Consequences:** a gutter more (6 px) between each two of them; no
  line inside the panel. Peek, drawn over the window, keeps its own.

