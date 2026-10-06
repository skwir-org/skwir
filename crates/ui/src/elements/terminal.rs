//! The terminal's grid (ADR-019): the plugin's `Screen` drawn cell by cell.
//!
//! Everything that depends on the element's size is decided in `prepaint`:
//! the cell is the mono font's advance and line height, the columns and rows
//! that fit are sent to the session when they change, and the pointer is
//! turned into cells with the bounds and cell of this frame. Runs are placed
//! at their own column and ASCII is shaped at exactly the cell's width, so a
//! bold face or a glyph from a fallback font cannot drift off the grid.
//!
//! A link (`Screen::links`) is drawn as one while Ctrl (Cmd) is held over
//! it, and followed by Ctrl+click, Shift added for a tab of its own
//! (`links.rs`); a plain press still selects. While the links are labelled
//! (Ctrl+Shift+O), each carries its label at its first cell.
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use bibli_core::terminal::{
    At, Attrs, Color, CursorShape, Input, Run, Screen, Size, Target, TerminalSession, Unit,
};
use gpui::{
    App, Bounds, CursorStyle, Element, ElementId, FocusHandle, Font, FontStyle, FontWeight,
    GlobalElementId, Hitbox, HitboxBehavior, Hsla, InspectorElementId, IntoElement, LayoutId,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, ScrollWheelEvent,
    ShapedLine, SharedString, StrikethroughStyle, Style, TextRun, UTF16Selection, UnderlineStyle,
    Window, point, px, relative, size,
};

/// Between the panel's edges and the grid.
const PAD: f32 = 6.;
/// Line height as a share of the font size.
const LEADING: f32 = 1.25;
/// Faint text (SGR 2), as programs mean it: the colour, less present.
const FAINT: f32 = 0.6;

/// The colours a screen is drawn with, from the theme.
#[derive(Clone)]
pub struct Colors {
    pub fg: Hsla,
    pub bg: Hsla,
    pub ansi: [Hsla; 16],
    pub selection: Hsla,
    /// A link's underline, and a label's fill.
    pub link: Hsla,
    /// A label's text, on `link`.
    pub on_link: Hsla,
}

impl Colors {
    fn of(&self, color: Color) -> Hsla {
        match color {
            Color::Foreground => self.fg,
            Color::Background => self.bg,
            Color::Ansi(index) => self.ansi[usize::from(index) % 16],
            Color::Rgb(r, g, b) => gpui::Rgba {
                r: f32::from(r) / 255.,
                g: f32::from(g) / 255.,
                b: f32::from(b) / 255.,
                a: 1.,
            }
            .into(),
        }
    }
}

/// What outlives a frame: the size last sent, a drag in progress, and the
/// part of a wheel turn too small to scroll a line yet.
#[derive(Default)]
pub struct GridState {
    sent: Cell<Option<Size>>,
    dragging: Cell<bool>,
    wheel: Cell<f32>,
    /// The pointer is on a link, whether drawn as one or not: Ctrl pressed
    /// or released there changes it (`Shell::on_modifiers`).
    on_link: Cell<bool>,
    /// The link drawn as one, by its place in `Screen::links`.
    armed: Cell<Option<usize>>,
    /// Where the link pressed with Ctrl leads: followed if released on it.
    pressed: RefCell<Option<Target>>,
    /// Where the armed link leads, when a program wrote it: said in a
    /// tooltip, since its text may say otherwise.
    tip: RefCell<Option<SharedString>>,
    /// The links are labelled: what is typed picks one, nothing reaches the
    /// shell.
    labelling: Cell<bool>,
}

impl GridState {
    pub fn pointer_on_link(&self) -> bool {
        self.on_link.get()
    }

    /// What the tooltip says: where the link under the pointer leads, when
    /// a program wrote it and Ctrl is held over it.
    pub fn tip(&self) -> Option<SharedString> {
        self.tip.borrow().clone()
    }

    pub fn set_labelling(&self, on: bool) {
        self.labelling.set(on);
    }
}

/// What a link says it leads to, for its tooltip.
fn describe(target: &Target) -> SharedString {
    match target {
        Target::Uri(uri) => uri.clone().into(),
        Target::Path {
            path, line: None, ..
        } => path.display().to_string().into(),
        Target::Path {
            path,
            line: Some(line),
            ..
        } => format!("{}:{line}", path.display()).into(),
    }
}

/// Follow a link: where it leads, and whether in a tab of its own.
pub type FollowLink = Rc<dyn Fn(&Target, bool, &mut Window, &mut App)>;

pub struct TerminalGrid {
    pub session: Arc<dyn TerminalSession>,
    pub screen: Arc<Screen>,
    pub focus: FocusHandle,
    pub font: Font,
    pub font_size: Pixels,
    pub colors: Colors,
    pub state: Rc<GridState>,
    pub follow: FollowLink,
    /// While the links are labelled: each link's label, in the order of
    /// `Screen::links`, and what was typed of one so far.
    pub labels: Option<(Vec<String>, String)>,
}

/// A frame's grid: where it starts and how big a cell is.
#[derive(Clone, Copy)]
struct Geometry {
    origin: Point<Pixels>,
    cell: gpui::Size<Pixels>,
    cols: u16,
    rows: u16,
}

impl Geometry {
    fn cell_bounds(&self, row: u16, col: u16, cells: u16) -> Bounds<Pixels> {
        Bounds::new(
            point(
                self.origin.x + self.cell.width * f32::from(col),
                self.origin.y + self.cell.height * f32::from(row),
            ),
            size(self.cell.width * f32::from(cells), self.cell.height),
        )
    }

    /// The cell under `position`, if it is on the grid.
    fn cell_under(&self, position: Point<Pixels>) -> Option<(u16, u16)> {
        let x = f32::from(position.x - self.origin.x) / f32::from(self.cell.width);
        let y = f32::from(position.y - self.origin.y) / f32::from(self.cell.height);
        (x >= 0. && y >= 0. && x < f32::from(self.cols) && y < f32::from(self.rows))
            .then_some((y as u16, x as u16))
    }

    /// The link of `screen` under `position`, by its place in the list.
    fn link_under(&self, screen: &Screen, position: Point<Pixels>) -> Option<usize> {
        let (row, col) = self.cell_under(position)?;
        screen.links.iter().position(|link| link.covers(row, col))
    }

    /// The cell under `position`, clamped to the grid, so a drag past an
    /// edge keeps selecting to it.
    fn at(&self, position: Point<Pixels>) -> At {
        let x = f32::from(position.x - self.origin.x) / f32::from(self.cell.width);
        let y = f32::from(position.y - self.origin.y) / f32::from(self.cell.height);
        let col = x.floor().clamp(0., f32::from(self.cols.saturating_sub(1)));
        let row = y.floor().clamp(0., f32::from(self.rows.saturating_sub(1)));
        At {
            row: row as u16,
            col: col as u16,
            right: x - col >= 0.5,
        }
    }
}

pub struct Prepainted {
    geometry: Geometry,
    hitbox: Hitbox,
    backgrounds: Vec<(Bounds<Pixels>, Hsla)>,
    text: Vec<(Point<Pixels>, ShapedLine)>,
    cursor: Option<(Bounds<Pixels>, Hsla, bool)>,
    /// The character under a block cursor, drawn again in the background.
    under_cursor: Option<(Point<Pixels>, ShapedLine)>,
    /// The armed link's underline, one bar per row it covers.
    underlines: Vec<Bounds<Pixels>>,
    /// Labels while the links are labelled: their fill and their text.
    labels: Vec<(Bounds<Pixels>, ShapedLine)>,
}

impl IntoElement for TerminalGrid {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

fn style_font(font: &Font, attrs: Attrs) -> Font {
    let mut font = font.clone();
    if attrs.has(Attrs::BOLD) {
        font.weight = FontWeight::BOLD;
    }
    if attrs.has(Attrs::ITALIC) {
        font.style = FontStyle::Italic;
    }
    font
}

impl TerminalGrid {
    fn text_run(&self, run: &Run, color: Hsla) -> TextRun {
        let attrs = run.style.attrs;
        TextRun {
            len: run.text.len(),
            font: style_font(&self.font, attrs),
            color,
            background_color: None,
            underline: attrs.has(Attrs::UNDERLINE).then_some(UnderlineStyle {
                thickness: px(1.),
                color: Some(color),
                wavy: false,
            }),
            strikethrough: attrs.has(Attrs::STRIKE).then_some(StrikethroughStyle {
                thickness: px(1.),
                color: Some(color),
            }),
            position: gpui::RunPosition::default(),
        }
    }

    fn foreground(&self, run: &Run) -> Hsla {
        let color = self.colors.of(run.style.fg);
        if run.style.attrs.has(Attrs::DIM) {
            color.opacity(FAINT)
        } else {
            color
        }
    }

    fn shape(&self, window: &Window, run: &Run, color: Hsla, cell: Pixels) -> ShapedLine {
        let runs = [self.text_run(run, color)];
        // Only ASCII is forced to the cell: a wide character spans two.
        let force = run.text.is_ascii().then_some(cell);
        window
            .text_system()
            .shape_line(run.text.clone().into(), self.font_size, &runs, force)
    }

    /// The text of the cell at `col` in `runs`, with its run, if any.
    fn cell_text(runs: &[Run], col: u16) -> Option<(Run, u16)> {
        let run = runs
            .iter()
            .find(|run| run.col <= col && col < run.col + run.cells)?;
        if run.text.is_ascii() {
            let index = usize::from(col - run.col);
            let text = run.text.get(index..=index)?;
            let one = Run {
                col,
                cells: 1,
                text: text.to_owned(),
                style: run.style,
            };
            Some((one, col))
        } else {
            Some((run.clone(), run.col))
        }
    }
}

impl Element for TerminalGrid {
    type RequestLayoutState = ();
    type PrepaintState = Prepainted;

    fn id(&self) -> Option<ElementId> {
        Some("terminal-grid".into())
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = relative(1.).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        (): &mut (),
        window: &mut Window,
        _: &mut App,
    ) -> Prepainted {
        let text_system = window.text_system();
        let font_id = text_system.resolve_font(&self.font);
        let advance = text_system
            .advance(font_id, self.font_size, 'm')
            .map_or(self.font_size * 0.6, |advance| advance.width);
        let cell = size(
            advance.max(px(1.)),
            (self.font_size * LEADING).round().max(px(1.)),
        );
        let inner = (
            f32::from(bounds.size.width) - 2. * PAD,
            f32::from(bounds.size.height) - 2. * PAD,
        );
        let cols = (inner.0 / f32::from(cell.width)).floor().clamp(2., 1000.) as u16;
        let rows = (inner.1 / f32::from(cell.height)).floor().clamp(1., 1000.) as u16;
        let size = Size {
            cols,
            rows,
            cell_width: f32::from(cell.width).round() as u16,
            cell_height: f32::from(cell.height).round() as u16,
        };
        if self.state.sent.get() != Some(size) {
            self.state.sent.set(Some(size));
            self.session.send(Input::Resize(size));
        }
        let geometry = Geometry {
            origin: point(bounds.origin.x + px(PAD), bounds.origin.y + px(PAD)),
            cell,
            cols,
            rows,
        };

        // The screen may still be the one from before a resize: draw what
        // fits, and the next screen fills the rest.
        let mut backgrounds = Vec::new();
        let mut text = Vec::new();
        for (row, runs) in self.screen.lines.iter().enumerate().take(usize::from(rows)) {
            let row = row as u16;
            for run in runs.iter().filter(|run| run.col < cols) {
                let cells = run.cells.min(cols - run.col);
                if run.style.bg != Color::Background {
                    backgrounds.push((
                        geometry.cell_bounds(row, run.col, cells),
                        self.colors.of(run.style.bg),
                    ));
                }
                if run.text.trim().is_empty() {
                    continue;
                }
                let origin = geometry.cell_bounds(row, run.col, cells).origin;
                text.push((
                    origin,
                    self.shape(window, run, self.foreground(run), cell.width),
                ));
            }
        }
        if let Some(selection) = self.screen.selection {
            let (start, end) = (selection.start, selection.end);
            for row in start.0..=end.0.min(rows.saturating_sub(1)) {
                let (from, to) = if selection.block {
                    (start.1.min(end.1), start.1.max(end.1))
                } else {
                    (
                        if row == start.0 { start.1 } else { 0 },
                        if row == end.0 { end.1 } else { cols - 1 },
                    )
                };
                if from <= to && from < cols {
                    let cells = to.min(cols - 1) - from + 1;
                    backgrounds.push((
                        geometry.cell_bounds(row, from, cells),
                        self.colors.selection,
                    ));
                }
            }
        }

        let focused = self.focus.is_focused(window);
        let mut under_cursor = None;
        let cursor = self
            .screen
            .cursor
            .filter(|cursor| cursor.row < rows && cursor.col < cols)
            .map(|cursor| {
                let cells = if cursor.wide { 2 } else { 1 };
                let whole = geometry.cell_bounds(cursor.row, cursor.col, cells);
                let color = self.colors.fg;
                match cursor.shape {
                    CursorShape::Block if focused => {
                        let runs = &self.screen.lines[usize::from(cursor.row)];
                        if let Some((run, col)) = Self::cell_text(runs, cursor.col) {
                            let shaped = self.shape(window, &run, self.colors.bg, cell.width);
                            let origin = geometry.cell_bounds(cursor.row, col, 1).origin;
                            under_cursor = Some((origin, shaped));
                        }
                        (whole, color, true)
                    }
                    // Without the keyboard, an outline: the place is shown,
                    // the text under it is not hidden.
                    CursorShape::Block => (whole, color, false),
                    CursorShape::Beam => (
                        Bounds::new(whole.origin, gpui::size(px(2.), whole.size.height)),
                        color,
                        true,
                    ),
                    CursorShape::Underline => (
                        Bounds::new(
                            point(whole.origin.x, whole.bottom() - px(2.)),
                            gpui::size(whole.size.width, px(2.)),
                        ),
                        color,
                        true,
                    ),
                }
            });
        // The link under the pointer, drawn as one while Ctrl is held.
        let under = bounds
            .contains(&window.mouse_position())
            .then(|| geometry.link_under(&self.screen, window.mouse_position()))
            .flatten();
        let armed = under.filter(|_| window.modifiers().secondary());
        self.state.on_link.set(under.is_some());
        self.state.armed.set(armed);
        let link = armed.and_then(|index| self.screen.links.get(index));
        *self.state.tip.borrow_mut() = link
            .filter(|link| link.written)
            .map(|link| describe(&link.target));
        let underlines = link.map_or_else(Vec::new, |link| {
            link.spans
                .iter()
                .filter(|&&(row, first, _)| row < rows && first < cols)
                .map(|&(row, first, last)| {
                    let cells = last.min(cols - 1) - first + 1;
                    let whole = geometry.cell_bounds(row, first, cells);
                    Bounds::new(
                        point(whole.origin.x, whole.bottom() - px(1.)),
                        gpui::size(whole.size.width, px(1.)),
                    )
                })
                .collect()
        });
        let labels = self
            .labels
            .as_ref()
            .map_or_else(Vec::new, |(labels, typed)| {
                self.screen
                    .links
                    .iter()
                    .zip(labels)
                    .filter(|(_, label)| label.starts_with(typed.as_str()))
                    .filter_map(|(link, label)| {
                        let &(row, first, _) = link.spans.first()?;
                        let cells = u16::try_from(label.len()).ok()?;
                        (row < rows && first + cells <= cols).then(|| {
                            let run = Run {
                                col: first,
                                cells,
                                text: label.clone(),
                                style: bibli_core::terminal::Style {
                                    attrs: Attrs::BOLD,
                                    ..bibli_core::terminal::Style::default()
                                },
                            };
                            let shaped = self.shape(window, &run, self.colors.on_link, cell.width);
                            (geometry.cell_bounds(row, first, cells), shaped)
                        })
                    })
                    .collect()
            });
        let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
        Prepainted {
            geometry,
            hitbox,
            backgrounds,
            text,
            cursor,
            under_cursor,
            underlines,
            labels,
        }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        (): &mut (),
        frame: &mut Prepainted,
        window: &mut Window,
        cx: &mut App,
    ) {
        let geometry = frame.geometry;
        let caret = frame
            .cursor
            .map_or(Bounds::new(geometry.origin, geometry.cell), |c| c.0);
        window.handle_input(
            &self.focus,
            Typing {
                session: Arc::clone(&self.session),
                caret,
                state: Rc::clone(&self.state),
            },
            cx,
        );
        window.with_content_mask(Some(gpui::ContentMask { bounds }), |window| {
            for (bounds, color) in frame.backgrounds.drain(..) {
                window.paint_quad(gpui::fill(bounds, color));
            }
            let line_height = geometry.cell.height;
            for (origin, line) in &frame.text {
                let _ = line.paint(*origin, line_height, window, cx);
            }
            if let Some((bounds, color, filled)) = frame.cursor {
                if filled {
                    window.paint_quad(gpui::fill(bounds, color));
                } else {
                    window.paint_quad(gpui::outline(bounds, color, gpui::BorderStyle::Solid));
                }
            }
            if let Some((origin, line)) = &frame.under_cursor {
                let _ = line.paint(*origin, line_height, window, cx);
            }
            for bar in frame.underlines.drain(..) {
                window.paint_quad(gpui::fill(bar, self.colors.link));
            }
            for (bounds, label) in &frame.labels {
                window.paint_quad(gpui::fill(*bounds, self.colors.link).corner_radii(px(2.)));
                let _ = label.paint(bounds.origin, line_height, window, cx);
            }
        });
        if self.state.armed.get().is_some() {
            window.set_cursor_style(CursorStyle::PointingHand, &frame.hitbox);
        }
        self.listen(&frame.hitbox, geometry, window);
    }
}

impl TerminalGrid {
    /// The pointer: a press selects (a double press words, a triple whole
    /// lines; with Shift it extends), a drag extends, the middle button
    /// pastes what is selected anywhere on the desktop, the wheel scrolls.
    /// Registered every frame, with this frame's geometry.
    fn listen(&self, hitbox: &Hitbox, geometry: Geometry, window: &mut Window) {
        let (session, state, focus) = (
            Arc::clone(&self.session),
            Rc::clone(&self.state),
            self.focus.clone(),
        );
        let hit = hitbox.clone();
        let screen = Arc::clone(&self.screen);
        window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
            if !phase.bubble() || !hit.is_hovered(window) {
                return;
            }
            match event.button {
                // A link pressed with Ctrl is followed when released on it,
                // and selects nothing.
                MouseButton::Left
                    if event.modifiers.secondary()
                        && let Some(link) = geometry
                            .link_under(&screen, event.position)
                            .and_then(|index| screen.links.get(index)) =>
                {
                    window.focus(&focus);
                    *state.pressed.borrow_mut() = Some(link.target.clone());
                    cx.stop_propagation();
                }
                MouseButton::Left => {
                    window.focus(&focus);
                    let at = geometry.at(event.position);
                    session.send(if event.modifiers.shift {
                        Input::Extend(at)
                    } else {
                        Input::Select {
                            at,
                            unit: match event.click_count {
                                0 | 1 => Unit::Char,
                                2 => Unit::Word,
                                _ => Unit::Line,
                            },
                        }
                    });
                    state.dragging.set(true);
                    cx.stop_propagation();
                }
                // The desktop's primary selection: Linux's alone.
                #[cfg(any(target_os = "linux", target_os = "freebsd"))]
                MouseButton::Middle => {
                    window.focus(&focus);
                    let text = cx.read_from_primary().and_then(|item| item.text());
                    if let Some(text) = text.filter(|text| !text.is_empty()) {
                        session.send(Input::Paste(text));
                    }
                    cx.stop_propagation();
                }
                _ => {}
            }
        });
        let (session, state) = (Arc::clone(&self.session), Rc::clone(&self.state));
        let (hit, screen) = (hitbox.clone(), Arc::clone(&self.screen));
        window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, _| {
            if !phase.bubble() {
                return;
            }
            if state.dragging.get() && event.pressed_button == Some(MouseButton::Left) {
                session.send(Input::Extend(geometry.at(event.position)));
            }
            // Onto a link or off it: drawn again, as one while Ctrl is held.
            let under = hit
                .is_hovered(window)
                .then(|| geometry.link_under(&screen, event.position))
                .flatten();
            let armed = under.filter(|_| event.modifiers.secondary());
            if under.is_some() != state.on_link.get() || armed != state.armed.get() {
                state.on_link.set(under.is_some());
                state.armed.set(armed);
                window.refresh();
            }
        });
        let (state, screen, follow) = (
            Rc::clone(&self.state),
            Arc::clone(&self.screen),
            Rc::clone(&self.follow),
        );
        window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
            if !phase.bubble() || event.button != MouseButton::Left {
                return;
            }
            state.dragging.set(false);
            let Some(pressed) = state.pressed.borrow_mut().take() else {
                return;
            };
            let released = geometry
                .link_under(&screen, event.position)
                .and_then(|index| screen.links.get(index))
                .is_some_and(|link| link.target == pressed);
            if released && event.modifiers.secondary() {
                follow(&pressed, event.modifiers.shift, window, cx);
                cx.stop_propagation();
            }
        });
        let (session, state, hit) = (
            Arc::clone(&self.session),
            Rc::clone(&self.state),
            hitbox.clone(),
        );
        window.on_mouse_event(move |event: &ScrollWheelEvent, phase, window, cx| {
            if !phase.bubble() || !hit.should_handle_scroll(window) {
                return;
            }
            let height = f32::from(geometry.cell.height);
            let turned =
                state.wheel.get() + f32::from(event.delta.pixel_delta(px(height)).y) / height;
            let lines = turned.trunc();
            state.wheel.set(turned - lines);
            if lines != 0. {
                session.send(Input::Wheel(lines as i32));
            }
            cx.stop_propagation();
        });
    }
}

/// Text from the keyboard, as the platform composes it: dead keys, the
/// compose key and input methods all end here. The keys that have no text
/// (and Ctrl or Alt chords) are the panel's key listener's.
struct Typing {
    session: Arc<dyn TerminalSession>,
    caret: Bounds<Pixels>,
    /// Nothing typed reaches the shell while the links are labelled.
    state: Rc<GridState>,
}

impl gpui::InputHandler for Typing {
    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut App,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: 0..0,
            reversed: false,
        })
    }

    fn marked_text_range(&mut self, _: &mut Window, _: &mut App) -> Option<std::ops::Range<usize>> {
        None
    }

    fn text_for_range(
        &mut self,
        _: std::ops::Range<usize>,
        _: &mut Option<std::ops::Range<usize>>,
        _: &mut Window,
        _: &mut App,
    ) -> Option<String> {
        None
    }

    fn replace_text_in_range(
        &mut self,
        _: Option<std::ops::Range<usize>>,
        text: &str,
        _: &mut Window,
        _: &mut App,
    ) {
        if !text.is_empty() && !self.state.labelling.get() {
            self.session.send(Input::Text(text.to_owned()));
        }
    }

    // A composition in progress is the input method's to show; the
    // terminal receives it once committed.
    fn replace_and_mark_text_in_range(
        &mut self,
        _: Option<std::ops::Range<usize>>,
        _: &str,
        _: Option<std::ops::Range<usize>>,
        _: &mut Window,
        _: &mut App,
    ) {
    }

    fn unmark_text(&mut self, _: &mut Window, _: &mut App) {}

    fn bounds_for_range(
        &mut self,
        _: std::ops::Range<usize>,
        _: &mut Window,
        _: &mut App,
    ) -> Option<Bounds<Pixels>> {
        Some(self.caret)
    }

    fn character_index_for_point(
        &mut self,
        _: Point<Pixels>,
        _: &mut Window,
        _: &mut App,
    ) -> Option<usize> {
        None
    }
}
