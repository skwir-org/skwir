//! The Map view (a lens, ADR-007 and ADR-020): the files of the location a
//! pane shows, placed where they say they are. The `geo` plugin finds the
//! places on workers (`geo.scan`) and hands them over a batch at a time
//! (`geo.points`, each point once); this view draws them with
//! `elements::map`, over the same backgrounds as the map preview.
//!
//! It is a view like the others: a click selects (Ctrl adds or takes away,
//! a double click opens), Shift and a drag draws a rectangle that selects
//! what it holds, the preview follows the selection, Enter opens it. Close
//! markers merge into a bubble with their count, which a click opens: it
//! zooms in, or, for files at one place, spreads them around it in the
//! order they were taken until a click beside them. Spread
//! (`ui.map_view_spread`) does that for every place at once, instead of
//! bubbles. A marker standing alone for a picture shows the picture. The
//! arrows move the map and the zoom keys zoom it; Home fits it to its
//! files again. The last button of its row chooses the background, as the
//! map preview's does (`map_preview::background_rows`).
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use bibli_core::EntryId;
use bibli_core::geo::LatLon;
use bibli_core::thumbs::ThumbSize;
use gpui::{AnyElement, Context, SharedString, Window, div, prelude::*};

use crate::elements::map::{self, Handles, Pick, Picking, Place, Shapes, View};
use crate::shell::Shell;
use crate::theme::Theme;
use crate::thumbs::ThumbView;
use crate::toolbar::{self, Button};
use crate::view::{Nav, PaneView};

pub const ID: &str = "map";
/// The popup that chooses the map's background (`ui.toolbar_menu`'s
/// `kind`), from the button in the view's row.
pub const BACKGROUNDS: &str = "map_backgrounds";
/// A key moves the map by this many pixels.
const PAN_STEP: f64 = 120.;
/// The map opened on a file shows the streets around it.
const FOCUS_ZOOM: f64 = 15.;
/// The placeholder of a location with nothing to place stays this long at
/// most while the next location's scan has placed nothing yet: a folder
/// of notes after another does not blink.
const HOLD: std::time::Duration = std::time::Duration::from_millis(250);

#[derive(Default)]
pub struct MapView {
    handles: Handles,
    /// Where the background button is, for its popup to hang from.
    background: crate::elements::popover::PopoverAnchor,
    /// What the scan was asked for: the location, its listing, its names
    /// and its modification times (a picture changed is looked at again;
    /// the plugin keeps what it showed until something moved), and whether
    /// the listing was complete (only then is the scan).
    requested: Option<(String, u64, u64, u64, bool)>,
    /// The plugin's descriptor revision, and the one read last.
    source: Option<Arc<AtomicU64>>,
    seen: Option<u64>,
    generation: u64,
    places: Vec<Place>,
    /// What is drawn: the places of the files the pane shows, its order
    /// (`Pane::order`: filtered by name, narrowed, companions hidden)
    /// when drawn last.
    shapes: Arc<Shapes>,
    shown: Arc<[EntryId]>,
    /// `shown` as a set, and what holds the places drawn: the places a
    /// batch adds are added to the shapes, not all of them drawn again.
    shown_set: HashSet<EntryId>,
    /// Which files `shown` holds: the listing, its names, and what the
    /// pane shows of them (hidden files, the filter and the narrowing, the
    /// folders opened). Another order of the same key holds the same
    /// files, but where a narrowing shows files as their metadata tells.
    shown_of: Option<(u64, u64, bool, u64, u64)>,
    bounds: bibli_core::geo::BoundsBuilder,
    counts: (usize, usize, usize, bool),
    selection: (u64, Arc<HashSet<EntryId>>),
    /// The file the map opens on, once it is placed: the one selected
    /// when the view was shown ("Show on map" in Details).
    focus: Option<EntryId>,
    /// The placeholder was drawn last, and since when it is held for the
    /// location shown now (`HOLD`).
    placeholder_shown: bool,
    hold: Option<std::time::Instant>,
}

impl MapView {
    /// Take what the scan found since the last batch.
    fn receive(&mut self, shell: &mut Shell) {
        let pane = shell.pane;
        // The pane shows other files (a filter, a narrowing): the places
        // drawn follow, with nothing asked again. The same files sorted
        // again (a sort by date, as dates come in) draw nothing again.
        if !Arc::ptr_eq(&shell.ui().list.sorted, &self.shown) {
            let sorted = Arc::clone(&shell.ui().list.sorted);
            let (of, narrowed) = {
                let ws = shell.app.workspace.read();
                ws.pane(pane).map_or((None, false), |p| {
                    (
                        Some((
                            p.listing.id(),
                            p.listing.names_version(),
                            p.sort.show_hidden,
                            p.filter_version(),
                            p.tree_version(),
                        )),
                        p.narrowed().is_some(),
                    )
                })
            };
            let same = of.is_some() && of == self.shown_of && !narrowed
                || sorted.len() == self.shown_set.len()
                    && sorted.iter().all(|id| self.shown_set.contains(id));
            self.shown = sorted;
            self.shown_of = of;
            if !same {
                self.shown_set = self.shown.iter().copied().collect();
                self.redraw();
            }
        }
        let source = self.source.get_or_insert_with(|| {
            shell
                .app
                .registry
                .views
                .find(|s| &*s.id == ID)
                .map_or_else(Arc::default, |s| Arc::clone(&s.revision))
        });
        let revision = source.load(Ordering::Acquire);
        if self.seen == Some(revision) {
            return;
        }
        self.seen = Some(revision);
        // Read where the plugin keeps them, as they are.
        let ask = |shell: &Shell, since: usize| shell.app.maps.points(pane, since);
        let Some(mut points) = ask(shell, self.places.len()) else {
            return;
        };
        let mut again = false;
        if points.generation != self.generation {
            // Another scan: everything again.
            self.generation = points.generation;
            self.places.clear();
            again = true;
            if let Some(all) = ask(shell, 0) {
                points = all;
            }
        }
        let added = self.places.len();
        let names: Vec<SharedString> = {
            let ws = shell.app.workspace.read();
            let pane = ws.pane(pane);
            points
                .points
                .iter()
                .map(|p| {
                    pane.and_then(|pane| pane.entry(p.id))
                        .map(|e| SharedString::from(e.name_str().into_owned()))
                        .unwrap_or_default()
                })
                .collect()
        };
        for (placed, name) in points.points.into_iter().zip(names) {
            let Some(at) = LatLon::new(placed.lat, placed.lon) else {
                continue;
            };
            let line = placed
                .line
                .into_iter()
                .map(|segment| {
                    segment
                        .into_iter()
                        .filter_map(|[lat, lon]| LatLon::new(lat, lon))
                        .collect()
                })
                .collect();
            self.places.push(Place {
                entry: placed.id,
                at,
                name,
                line,
                clock: placed.clock,
            });
        }
        self.counts = (
            points.placed,
            points.looked,
            points.candidates,
            points.complete,
        );
        if again {
            self.redraw();
        } else {
            self.add(added);
        }
    }

    /// The shapes of the places of the files the pane shows.
    fn redraw(&mut self) {
        let places: Vec<&Place> = self
            .places
            .iter()
            .filter(|place| self.shown_set.contains(&place.entry))
            .collect();
        self.bounds = bibli_core::geo::BoundsBuilder::default();
        for place in &places {
            self.bounds.add(place.at);
        }
        self.shapes = Arc::new(Shapes::places(&places));
    }

    /// The places from `from` on, received just now, added to the shapes
    /// drawn: in place, unless a marker drawn would move with them.
    fn add(&mut self, from: usize) {
        let new: Vec<&Place> = self.places[from..]
            .iter()
            .filter(|place| self.shown_set.contains(&place.entry))
            .collect();
        if new.is_empty() {
            return;
        }
        let mut bounds = self.bounds.clone();
        for place in &new {
            bounds.add(place.at);
        }
        if Arc::make_mut(&mut self.shapes).add_places(&bounds, &new) {
            self.bounds = bounds;
        } else {
            self.redraw();
        }
    }

    /// What the header says about the scan.
    fn status(&self) -> String {
        let (placed, looked, candidates, complete) = self.counts;
        let files = |n: usize| if n == 1 { "file" } else { "files" };
        if !complete {
            format!(
                "Placing… {placed} of {candidates} pictures and tracks looked at so far: {looked}"
            )
        } else if looked > placed {
            format!(
                "{placed} {} placed · {} without a position",
                files(placed),
                looked - placed
            )
        } else {
            format!("{placed} {} placed", files(placed))
        }
    }
}

impl PaneView for MapView {
    fn id(&self) -> &'static str {
        ID
    }

    fn scroll_to(&mut self, _: usize) {}

    fn nav_delta(&self, _: Nav, _: &Shell) -> Option<isize> {
        None
    }

    /// The arrows move the map; Home fits it to its files again.
    fn nav(&mut self, nav: Nav, _: &mut Shell, _: &mut Window, cx: &mut Context<Shell>) -> bool {
        let (dx, dy) = match nav {
            Nav::Left => (-1., 0.),
            Nav::Right => (1., 0.),
            Nav::Up => (0., -1.),
            Nav::Down => (0., 1.),
            Nav::Home => {
                self.handles.view.set(None);
                cx.notify();
                return true;
            }
            _ => return false,
        };
        if let Some(view) = self.handles.view.get().or_else(|| self.handles.shown.get()) {
            self.handles
                .view
                .set(Some(view.pan(-dx * PAN_STEP, -dy * PAN_STEP)));
            cx.notify();
        }
        true
    }

    fn zooms(&self) -> crate::view::Zooms {
        crate::view::Zooms::Own
    }

    fn zoom(&mut self, dir: &str) {
        let Some(view) = self.handles.view.get().or_else(|| self.handles.shown.get()) else {
            return;
        };
        let levels = match dir {
            "in" => 1.,
            "out" => -1.,
            _ => {
                self.handles.view.set(None);
                return;
            }
        };
        self.handles.view.set(Some(View {
            zoom: (view.zoom + levels).clamp(0., map::MAX_ZOOM),
            ..view
        }));
    }

    fn reset(&mut self) {
        // Another location: fitted to its own files, nothing opened.
        self.handles.view.set(None);
        self.handles.fold();
    }

    fn render(&mut self, shell: &mut Shell, _: &mut Window, cx: &mut Context<Shell>) -> AnyElement {
        let pane = shell.pane;
        let (source, selection, folder) = {
            let ws = shell.app.workspace.read();
            let Some(p) = ws.pane(pane) else {
                return div().into_any_element();
            };
            let source = (
                p.location().map(ToString::to_string).unwrap_or_default(),
                p.listing.id(),
                p.listing.names_version(),
                p.listing.rewritten_version(),
                p.listing.is_complete(),
            );
            let selection = (self.selection.0 != p.selection.revision())
                .then(|| (p.selection.revision(), p.selection.iter().collect()));
            let folder = p.location().and_then(bibli_core::Location::path).is_some();
            (source, selection, folder)
        };
        if let Some((revision, ids)) = selection {
            self.selection = (revision, Arc::new(ids));
        }
        // Shown just now: its scan was let go when it was hidden, so it
        // asks again (what the plugin remembers places it at once), and
        // with one file selected the map opens on it.
        if shell.map_views.insert(pane) {
            self.requested = None;
            self.counts.3 = false;
            self.focus = match self.selection.1.iter().collect::<Vec<_>>().as_slice() {
                [only] => Some(**only),
                _ => None,
            };
        }
        // Another location, after one with nothing to place: its
        // placeholder is held while this one's scan has nothing yet.
        if self.placeholder_shown
            && self
                .requested
                .as_ref()
                .is_some_and(|requested| requested.0 != source.0)
        {
            self.hold = Some(std::time::Instant::now());
            // A redraw when the hold ends, whatever else wakes the window.
            cx.spawn(async move |shell, cx| {
                cx.background_executor().timer(HOLD).await;
                let _ = shell.update(cx, |_, cx| cx.notify());
            })
            .detach();
        }
        if self.requested.as_ref() != Some(&source) {
            self.requested = Some(source);
            if let Err(error) = shell
                .app
                .dispatch(&bibli_core::command::Invocation::with_args(
                    "geo.scan",
                    &serde_json::json!({ "pane": pane }),
                ))
            {
                shell.set_status(format!("{error:#}"), true, cx);
            }
        }
        self.receive(shell);
        if let Some(entry) = self.focus {
            if let Some(place) = self.places.iter().find(|p| p.entry == entry) {
                let (x, y) = map::math::project(place.at);
                self.handles.view.set(Some(View {
                    x,
                    y,
                    zoom: FOCUS_ZOOM,
                }));
                self.focus = None;
            } else if self.counts.3 {
                // Looked at everything: it has no place.
                self.focus = None;
            }
        }

        let theme = cx.global::<Theme>().clone();
        let (_, _, _, complete) = self.counts;
        // With nothing placed there is no map to draw: a map of nothing
        // fits the whole world, which would flash before the placeholder.
        // Nothing here says where it is: say so, and offer the subfolders;
        // while the scan looks, the header says so over the empty paper.
        let nothing = self.places.is_empty();
        let held = self.hold.is_some_and(|since| since.elapsed() < HOLD);
        if !nothing || complete || !held {
            self.hold = None;
        }
        self.placeholder_shown = nothing && (complete || held);
        if self.placeholder_shown {
            let placeholder = crate::placeholder::Placeholder {
                icon: "map",
                title: "Nothing here says where it is".into(),
                detail: "No picture or track of this location has a position. Pictures keep one when the camera knew where it was.".into(),
                action: folder.then_some(("Search the subfolders", "geo.search_subfolders")),
                problem: false,
            };
            let focus = shell.ui().placeholder_focus.clone();
            return crate::placeholder::render(placeholder, &focus, cx);
        }

        let mut header = toolbar::row(&theme)
            .id(("map-view-tools", pane.0 as usize))
            .child(crate::elements::clip::clipped(
                div()
                    .id("map-status")
                    .flex_1()
                    .min_w_0()
                    .text_xs()
                    .text_color(theme.text_muted)
                    .truncate(),
                self.status(),
                shell.font.clone(),
            ));
        if folder {
            header = header.child(
                Button::new(
                    format!("map-view-area-{}", pane.0),
                    "search",
                    "Search the pictures and videos of this folder and its subfolders made in the area shown",
                    "ui.map_view_search_area",
                )
                .label("This area")
                .render(shell, cx),
            );
            header = header.child(
                Button::new(
                    format!("map-view-subfolders-{}", pane.0),
                    "search",
                    "Show the pictures and tracks of the subfolders too",
                    "geo.search_subfolders",
                )
                .label("Subfolders")
                .render(shell, cx),
            );
        }
        shell
            .map_areas
            .insert(pane, Rc::clone(&self.handles.shown_area));
        let spread = shell.map_spread.contains(&pane);
        header = header.child(
            Button::new(
                format!("map-view-spread-{}", pane.0),
                "spread",
                if spread {
                    "Group the files at one place into a count again"
                } else {
                    "Spread the files at one place around it, in the order they were taken"
                },
                "ui.map_view_spread",
            )
            .active(spread)
            .render(shell, cx),
        );
        header = header.child(
            Button::new(
                format!("map-view-fit-{}", pane.0),
                "fit",
                "Fit the map to its files · Home",
                "ui.map_view_fit",
            )
            .active(self.handles.view.get().is_none())
            .render(shell, cx),
        );
        // The background, chosen here as in the map preview and Settings;
        // its popup hangs from the active pane's, the pane that opened it.
        let backgrounds = shell.map_tiles.borrow_mut().backgrounds();
        let open = shell.app.workspace.read().active_id() == pane
            && shell
                .toolbar_menu
                .as_ref()
                .is_some_and(|menu| menu.kind == BACKGROUNDS);
        if let Some(backgrounds) = &backgrounds {
            header = header.child(
                crate::map_preview::background_button(
                    format!("map-view-background-{}", pane.0),
                    backgrounds,
                    "ui.toolbar_menu",
                    BACKGROUNDS,
                )
                .anchor_if(&self.background, true)
                .active(open)
                .dropdown()
                .render(shell, cx),
            );
        }
        let popup = backgrounds.filter(|_| open).and_then(|backgrounds| {
            let popup = shell.toolbar_menu.as_ref()?;
            let panel = toolbar::popup_panel(popup, "ui.toolbar_menu", &theme, cx).child(
                crate::map_preview::background_rows(shell, &backgrounds, &theme, cx),
            );
            Some(self.background.popup(panel))
        });

        if nothing {
            return div()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .child(header)
                .child(div().flex_1().bg(theme.bg))
                .children(popup)
                .into_any_element();
        }
        let weak = cx.entity().downgrade();
        let picked = weak.clone();
        let picking = Picking {
            selected: Arc::clone(&self.selection.1),
            on_pick: Rc::new(move |pick, modifiers, clicks, window, cx| {
                let _ = picked.update(cx, |shell, cx| {
                    shell.with_pane(pane, |shell| {
                        let (ids, mode) = match pick {
                            Pick::Entry(id) => (
                                vec![id],
                                if modifiers.secondary() {
                                    "toggle"
                                } else {
                                    "replace"
                                },
                            ),
                            Pick::Cluster(ids, _) => (
                                ids.to_vec(),
                                if modifiers.secondary() {
                                    "add"
                                } else {
                                    "replace"
                                },
                            ),
                            Pick::Band(ids) => (
                                ids,
                                if modifiers.secondary() {
                                    "add"
                                } else {
                                    "replace"
                                },
                            ),
                            Pick::Nothing if modifiers.secondary() => return,
                            Pick::Nothing => (Vec::new(), "replace"),
                        };
                        shell.select_ids(ids, mode, window, cx);
                        if clicks == 2 {
                            shell.open_selection(window, cx);
                        }
                    });
                });
            }),
            thumb: Rc::new(move |id, _, cx| {
                weak.update(cx, |shell, _| {
                    shell.with_pane(pane, |shell| match shell.thumb(id, ThumbSize::Normal) {
                        ThumbView::Ready(image) => Some(image),
                        ThumbView::Pending => {
                            shell.request_hydrate([id]);
                            None
                        }
                        ThumbView::Unavailable => None,
                    })
                })
                .ok()
                .flatten()
            }),
        };
        let element = crate::map_preview::element(
            shell,
            ("map-view", pane.0 as usize),
            Arc::clone(&self.shapes),
            self.handles.clone(),
            &theme,
            None,
        )
        .picking(picking)
        .spread(spread);
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(header)
            .child(div().flex_1().min_h_0().child(element))
            .children(popup)
            .into_any_element()
    }
}

impl Shell {
    /// `ui.map_view_fit`: the active pane's map, fitted to its files again.
    pub(crate) fn map_view_fit(&mut self, cx: &mut Context<Self>) {
        if self.ui().view_id == ID {
            self.with_view(|view, _| view.zoom("reset"));
            cx.notify();
        }
    }

    /// `ui.map_view_spread`: the active pane's map spreads the files at
    /// one place around it, or groups them into a count again.
    pub(crate) fn map_view_spread(&mut self, cx: &mut Context<Self>) {
        if self.ui().view_id == ID {
            let pane = self.pane;
            if !self.map_spread.remove(&pane) {
                self.map_spread.insert(pane);
            }
            cx.notify();
        }
    }

    /// `ui.map_view_search_area`: the pictures and videos of the active
    /// pane's folder and subfolders made inside the area its map shows.
    pub(crate) fn map_view_search_area(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.ui().view_id != ID {
            return;
        }
        let pane = self.pane;
        let Some([south, west, north, east]) = self.map_areas.get(&pane).and_then(|a| a.get())
        else {
            return;
        };
        self.run(
            "geo.search_area",
            serde_json::json!({
                "pane": pane,
                "south": south,
                "west": west,
                "north": north,
                "east": east,
            }),
            window,
            cx,
        );
    }

    /// Panes that stopped showing their map let their scan go.
    pub(crate) fn sync_map_views(&mut self) {
        let shown: HashSet<_> = self
            .panes
            .iter()
            .filter(|p| p.view_id == ID)
            .map(|p| p.id)
            .collect();
        let gone: Vec<_> = self
            .map_views
            .iter()
            .copied()
            .filter(|pane| !shown.contains(pane))
            .collect();
        // A pane keeps its choice to spread while it lives.
        let ws = self.app.workspace.read();
        self.map_spread.retain(|&pane| ws.pane(pane).is_some());
        self.map_areas.retain(|pane, _| ws.pane(*pane).is_some());
        drop(ws);
        for pane in gone {
            self.map_views.remove(&pane);
            let _ = self
                .app
                .dispatch(&bibli_core::command::Invocation::with_args(
                    "geo.release",
                    &serde_json::json!({ "pane": pane }),
                ));
        }
    }
}
