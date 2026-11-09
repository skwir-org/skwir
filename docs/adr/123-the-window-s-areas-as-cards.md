# ADR-123: The window's areas as cards

Status: accepted 2026-11-07 with the user, the default since that
evening (`ui.cards`, on; off, the window apart by lines as before);
built on Linux, macOS and Windows compiled only; amended the same
evening as tried, and on 2026-11-08 lighter, its cards outlined
(Amendments). Amends ADR-071
(tokens of a look: `frame`, `bar`, `sunken_bg`, `sunken_bar`) and
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

1. **One switch, on by default** (since the amendment "Kept, the
   default"). Settings › Appearance › Cards (`ui.cards`, under More
   settings since it is the default): the
   window as cards on its frame, or as it was, apart by lines. Off, the
   window is drawn as before.
2. **The frame is a token of the look** (as amended 2026-11-08,
   "Lighter"). `frame` lies a step under the files' ground and under
   the chrome's. Where the desktop names a sidebar color of its own
   under the files (a GTK sheet's `sidebar_bg_color`, ADR-071 amended
   2026-11-08), it is that. Else, in a light look, whose chrome lies
   under the files, it is the chrome's own gray (`surface`, the
   desktop's window color, which `CHROME` keeps 1.08:1 at least under
   `bg`). Either lies `ROOM` (1.097:1) under the files at least, room
   for the bars between. In a dark look, whose chrome is lighter than
   the files, it is `CHROME` under the files, its hue kept
   (`color::below`). It gives way to the text drawn on it (the
   sidebar's, the tabs'): never darker than `text` reads on at its
   ratio, a hover laid on it. Skwir's own: `#efefef`, `#161616`. As
   cards (`Theme::as_cards`), the bars (`bar`) of a light look lie
   `BAR` (1.07:1) under the files towards the frame (`color::along`,
   Skwir's own `#f2f2f2`) and `EDGE` (1.025:1) above it at least, the
   card's edge drawn by its outline (point 7); in a dark look they stay
   `surface`. The text and the muted text are guarded on
   the frame and the bars too, as the accent, what is written on it, the
   accent's text and the outcomes are (2026-11-09), and a split's other pane has grounds of
   its own, `sunken_bg` and `sunken_bar`: halfway towards the frame, as
   far as its text still reads. Apart by lines, no token changes.
3. **What is a card**: each pane (its row, its files, its timeline, its
   footer), each part of the preview panel (Properties with the header,
   the preview with its toolbar, a container's kept rows and the member
   it shows, the pinned player) and the terminal. A card keeps what it holds within its corners
   (`frame::card`, a rounded content mask: gpui patch 0053). Its row and
   its footer are bands of `bar` over it, no line between them and
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
7. **Gutters, not lines between areas**: 6 px (`frame::GUTTER`),
   between cards and at the top, an exception to the theme's multiples
   of 4 by the user's choice. Each card is outlined, in either look (as
   amended 2026-11-08, "Then, an outline"): a ring a device pixel wide
   on the frame just outside the card's shape, its notches followed, in
   `border`, the color the lines between areas were drawn in (a
   desktop's own where it names one), kept `CHROME` apart from the frame
   at least (`Theme::card_edge`, `frame::paint_outline`), drawn within
   the window, so not where a card meets its edge. It is the card's edge,
   not a line between two areas: two cards keep the gutter's frame
   between their outlines, and a gutter under the pointer shows the
   handle over both. The gutter between two cards is the edge dragged between them
   (`edge::gutter`): the split, the terminal's top, the preview's side,
   each with the handle's color under the pointer, a double click back
   to the size it ships with, measured from the gutter's middle. Between
   the sidebar and the cards there is none: the sidebar's own margins set
   it apart, and its edge stays its own.
8. **The pane commands act on** while the window is split: its row
   underlined in the accent (2 px along the band's foot), the other
   pane's grounds a step down (`sunken_bg`, `sunken_bar`: its files,
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

### 2026-11-07: kept, the default

- **Decided** (the user, having tried every step): Cards is kept and is
  the default (`settings::CARDS_DEFAULT`); this record is accepted as
  such.
- **The look apart by lines stays a choice**, the same switch off: some
  prefer a denser window or the one they knew, and a look taken away in
  the release that changes it is the hardest way to change it (Firefox's
  islands drew requests to turn them off, Mozilla Connect, 2026-03). It
  is kept working, not grown: an area added from now on is designed as a
  card first, and given the lines look only as far as it needs to work
  there. Whether to keep the choice is asked again after a release or
  two, from what is said of it.
- **The switch under More settings** (ADR-072): a detail of the look for
  the few who change it, as the opacities are; found by the settings'
  search, and shown unfolded once changed.


### 2026-11-08: lighter

- **Asked** (the user, beside Dolphin): the window's tint too dark
  since the cards; lighten it and the other grounds with it, keeping
  the look.
- **As it stands** (points 2 and 7; the bullets below are the way
  there, tried the same day): in a light look the frame is the
  desktop's sidebar color (a GTK sheet's, ADR-071) or its window gray,
  at least `ROOM` (1.097:1) under the files; the bars `BAR` (1.07:1)
  under the files and `EDGE` (1.025:1) above the frame at least; the
  other pane halfway towards the frame; each card outlined in `border`,
  in both looks. With Noctalia's colors: frame `#eeeeed`, as Dolphin's
  window, bars about `#f2f1f1`, files `#faf9f9`, outline `#d7d7d7`.
- **Measured** (Skwir's own light look, split): four grays where
  Dolphin has two. The frame (`#e7e7e7`) stood 1.19:1 under the files
  (`#fafafa`), the bars `#f0f0f0`, the other pane's files `#f2f2f2`;
  Dolphin's window `#eeeeed` stands 1.10:1 under its view `#faf9f9`.
- **Why**: the frame lay a step under the darker of a card's grounds. In
  a light look that is the bars, thin bands, so the frame lay two steps
  under the files, most of the card, and the sidebar darker than any
  chrome had been. In a dark look the darker is the files: one step,
  which is why only the light look was heavy. With a desktop's colors
  as Plasma's lay them (`#faf9f9`, `#eeeeed`) the rule gave `#e5e5e4`.
- **Decided**: the frame a step under the files (point 2): in a light
  look the chrome's own gray, so the sidebar and the tabs lie on the
  desktop's window color as Dolphin's do (`#eeeeed` there, `#f0f0f0`
  Skwir's own), and the bars give way to it, as far from it as from
  the files (`bar`, `#f4f3f3`, `#f5f5f5`). The other pane steps
  halfway towards the frame (two fifths before), which keeps it
  visible now the frame is nearer: its files about the bars' gray. The
  dark look keeps its frame, its bars and its files; only its other
  pane steps that much further (`#1a1a1a` files, `#1e1e1e` bars, for
  `#1b1b1b` and `#202020`).
- **Consequences**: within a card, the bars stand from the files at
  about 1.04:1, not `CHROME`: the card's edge on the frame sets the
  chrome apart, the bars only within it. In a light look the other
  pane's row stands from the frame by about 1.02:1, its field and its
  buttons kept on their own ground. The look apart by lines is drawn as
  before (`bar` is `surface` there). A theme's sidebar color
  (libadwaita's `sidebar_bg_color`) was not read yet: it is the frame
  since ADR-071's amendment of the same day.
- **Checked** on Xvfb (lavapipe), the window split with the preview, in
  Skwir's own light and dark looks and with two desktop sheets (Plasma's
  pair, and libadwaita names with the window as light as the view); the
  grounds sampled before and after: as above, and `#f1f0f0` frame,
  `#f5f4f4` bars where the window is as light as the view.
- **Then, the toolbar** (the user, the same day, on their desktop): the
  bars halfway stood only 1.045:1 to 1.05:1 from the files, too close.
  They are set by what they are told apart from instead: `BAR`
  (1.07:1) under the files, between the 1.09:1 they had and the
  halfway's, and `EDGE` (1.025:1) above the frame at least, which keeps
  the card's corners where its bars meet the frame. The frame keeps the
  desktop's color where that leaves the room (`ROOM`, 1.097:1: Noctalia's
  `#eeeeed`, adw-gtk3's `#ebebed`) and steps down where it does not
  (Skwir's own `#f0f0f0` to `#efefef`). Bars: Skwir's own `#f2f2f2`,
  Noctalia's about `#f2f1f1`; the card's edge on the frame 1.027:1 and
  1.032:1. A halfway step back (the frame darker again for every desktop)
  and the bars on the frame (Dolphin's, the islands direction, the card's
  top and corners gone) were set aside.
- **Then, the header** (the user, the same day): at 1.025:1 from the
  frame the card's header blended into the sidebar and the tabs. With
  grays alone the two edges share what lies between the frame and the
  files, so the frame steps down: `EDGE` 1.045:1, the header's edge as
  it was with the bars halfway, `ROOM` 1.118:1 (1.185:1 before the
  cards were made lighter). Skwir's own frame `#ededed`; Noctalia's
  sidebar (1.105:1) is nearer than the room and moves down too, about
  `#ececeb`; adw-gtk3's `#ebebed` (1.19:1) stays. The bars stay 1.07:1
  under the files. Tried first, as one constant: better, still blended,
  the user said, and Noctalia's own gray lost.
- **Then, an outline** (the user, the same day: keep Noctalia's gray,
  outline the cards): Dolphin outlines its views, a 1 px `#e5e5e4` on
  its `#eeeeed` (1.09:1, measured on the user's screenshot), and that
  is how it keeps two grays. Each card of a light look is outlined so
  (point 7), `CHROME` under the frame: with Noctalia's sheet
  `#e5e5e4`, Dolphin's own; Skwir's own `#e7e7e7` on `#efefef`. The
  card's edge no longer comes from the grays, so `EDGE` is 1.025:1
  again and `ROOM` 1.097:1: Noctalia's sidebar `#eeeeed` is the frame
  once more, the bars 1.07:1 under the files. A dark look's cards stand
  apart already (bars above the files, the frame under them) and have
  none. Checked on Xvfb: the outline a crisp device pixel on the
  frame, Dolphin's `#e5e5e4` with Noctalia's colors, `#e7e7e7` with
  Skwir's; round at the corners standing apart; along a notch's band,
  its concave fillet and under its buttons, at both ends (the sidebar
  put away, the preview off); none at the window's bottom; two between
  split panes with the frame between; none in the dark look. Not
  checked: while the window lets the desktop through (the ring is laid
  on the frame at its opacity, as what is written there).
- **Then, darker and in both looks** (the user, having tried it): a
  step darker, two of the chrome's steps under the frame (`OUTLINE`,
  1.166:1): Noctalia's about `#dddddc`, Skwir's own `#dedede`; and in
  the dark look too, by the same rule, as far under its frame as black
  lets it (Skwir's own: black under `#161616`, 1.16:1).
- **Then, the lines' color** (the user asked whether the outline stood
  as the lines before the cards did; it did not: 1.17:1 on the frame
  where the lines' `border` stood 1.25:1 on the chrome, and dark where
  they were light): the outline is `border`, in both looks. Skwir's own
  light `#d8d8d8` on `#efefef` (1.24:1), Noctalia's sheet `#d7d7d7`
  (derived: it names no `borders`) on `#eeeeed` (1.24:1), Skwir's own
  dark `#3a3a3a` on `#161616` (1.59:1), lighter than the frame and the
  cards, as the dark look's lines were. `OUTLINE` is gone.

### 2026-11-09: the outline from quads

- **Found** (a review of the code): the outline was an even-odd path,
  the only one a window drew by default, so every frame of the default
  look opened gpui's path pass, a multisampled target the size of the
  window, for a line a pixel wide.
- **Decided**: what a window draws every frame by default opens no path
  pass. The outline (`frame::paint_outline`) is made of quads, worked out
  in whole device pixels from the corners of the card's shape: a fill for
  each straight stretch, and a bordered disc kept to its quadrant for each
  arc, no two on the same pixel. The frame's ground stays a path while
  the window lets the desktop through, which is not the default.
- **Checked** on Xvfb (lavapipe), against the build before: a card with a
  notch at either end, both, split, the preview; at scale 1 the ring's
  pixels match to within the antialiasing of its arcs (a few levels);
  at 1.5 and 2, unbroken, at the corners and along each notch. Frame times
  not measured: the machine was too loaded for a difference to mean
  anything.
