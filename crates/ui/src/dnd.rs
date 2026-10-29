//! Drag and drop: internal moves/copies between rows, cells, columns, and
//! sidebar places, and external paths dropped from other applications.
//!
//! What a drop in a folder does is what the keys held say (`held`): Shift
//! moves, Ctrl copies, Ctrl+Shift links. With none held it asks
//! (`core.drop_asks`, on by default: `Shell::ask_drop`), the row of what
//! it would do unasked ready for Enter, with what plugins offer for what
//! was dropped (the registry's `drop` menu: Extract Here); or, not
//! asking, does that (`drop_op`): our own files move within a filesystem
//! and are copied to another, so a drop onto a USB stick leaves the
//! folder it came from as it was, and paths from another application are
//! copied.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui::{
    Context, Div, ExternalPaths, Pixels, Point, Render, RenderImage, Stateful, Window, div, img,
    point, prelude::*, px,
};
use skwir_core::EntryId;
use skwir_core::command::Invocation;
use skwir_core::fs;
use skwir_core::op::EntryRef;
use skwir_core::os_path::PathArg;
use skwir_core::registry::MenuItem;
use skwir_core::workspace::PaneId;

use crate::shell::Shell;
use crate::theme::Theme;

/// What a drop does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DropOp {
    Copy,
    Move,
    /// A link to each, made in the folder (`op::Link`).
    Link,
}

impl DropOp {
    /// Every drop, in the order the drop menu lists them.
    const ALL: [DropOp; 3] = [DropOp::Move, DropOp::Copy, DropOp::Link];

    fn command(self) -> &'static str {
        match self {
            DropOp::Copy => "copy",
            DropOp::Move => "move",
            DropOp::Link => "link",
        }
    }

    /// The keys held that make a drop this one without asking (`held`),
    /// as a binding spells them.
    fn keys(self) -> &'static str {
        match self {
            DropOp::Move => "shift",
            DropOp::Copy => "ctrl",
            DropOp::Link => "ctrl-shift",
        }
    }

    fn title(self) -> &'static str {
        match self {
            DropOp::Move => "Move Here",
            DropOp::Copy => "Copy Here",
            DropOp::Link => "Link Here",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            DropOp::Move => "move",
            DropOp::Copy => "copy",
            DropOp::Link => "link",
        }
    }
}

/// What was dropped, as a drop's menu asks about it: the entries, and
/// what each is as its name says (the name that says its type, and the
/// type it has whatever its name: a folder), for the `drop` items' `when`.
#[derive(Clone, Debug, Default)]
pub struct Dropped {
    pub entries: Vec<EntryRef>,
    pub types: Vec<(std::ffi::OsString, Option<&'static str>)>,
}

impl Dropped {
    /// Paths from another application: who they are and whether each is a
    /// folder (a link, as what it leads to), read from the disk, on a
    /// worker. Those gone are left out.
    fn of_paths(paths: &[PathBuf]) -> Self {
        let mut dropped = Dropped::default();
        for path in paths {
            let Some(entry) = EntryRef::from_path(path) else {
                continue;
            };
            let folder = std::fs::metadata(path).is_ok_and(|m| m.is_dir());
            dropped.types.push((
                path.file_name().unwrap_or_default().to_owned(),
                folder.then_some("inode/directory"),
            ));
            dropped.entries.push(entry);
        }
        dropped
    }

    /// The arguments of the command a drop of it into `dest` runs
    /// (`move`, `copy`, `link`), the entries named: never the selection by
    /// the time it runs. `None` when nothing is named, since a command
    /// given no entries takes the selection.
    fn args(&self, dest: &Path, pane: Option<PaneId>) -> Option<serde_json::Value> {
        if self.entries.is_empty() {
            return None;
        }
        let mut args = serde_json::json!({ "entries": self.entries, "dest": PathArg::from(dest) });
        if let Some(pane) = pane {
            args["pane"] = serde_json::json!(pane);
        }
        Some(args)
    }

    /// What `when` reads of it: how many, whether all are folders or
    /// files, and their types. Nothing of where it is: a drop's menu is
    /// about what was dropped.
    fn when(&self) -> (crate::menu::SelectionCtx, crate::menu::SelectionTypes) {
        let folder =
            |(_, inode): &(std::ffi::OsString, Option<&str>)| *inode == Some("inode/directory");
        let ctx = crate::menu::SelectionCtx {
            count: self.entries.len(),
            all_dirs: self.types.iter().all(folder),
            all_files: !self.types.iter().any(folder),
            ..crate::menu::SelectionCtx::default()
        };
        let types = crate::menu::SelectionTypes::of(
            self.types
                .iter()
                .map(|(name, inode)| (name.as_os_str(), *inode)),
        );
        (ctx, types)
    }
}

/// What the keys held say a drop does, when they say: Shift moves, Ctrl
/// copies, both link.
pub fn held(modifiers: gpui::Modifiers) -> Option<DropOp> {
    match (modifiers.control, modifiers.shift) {
        (true, true) => Some(DropOp::Link),
        (true, false) => Some(DropOp::Copy),
        (false, true) => Some(DropOp::Move),
        (false, false) => None,
    }
}

/// The keys held decide; without them, `same_device` does (whether what is
/// dropped is on the destination's filesystem; `None` when not known,
/// which copies: nothing is taken from where it was by a guess).
pub fn drop_op(modifiers: gpui::Modifiers, same_device: Option<bool>) -> DropOp {
    held(modifiers).unwrap_or(if same_device == Some(true) {
        DropOp::Move
    } else {
        DropOp::Copy
    })
}

/// What a drag carries: only the pane it started in. Rows are made
/// draggable every frame, so the payload must cost nothing to build; the
/// selection it moves is read when the drag starts and when it drops.
#[derive(Clone, Copy, Debug)]
pub struct DragPayload {
    pub pane: PaneId,
}

/// The icon of the files a drag carries (`dragicon.rs`): drawn under the
/// pointer in the window, and handed to the compositor when the drag leaves
/// it, so crossing the window's edge changes nothing. Kept until the next
/// drag of files, or the icon that replaces it, which lets go of its
/// texture.
pub struct FilesIcon {
    icon: Arc<gpui::ExternalDragIcon>,
    image: Arc<RenderImage>,
}

/// The floating preview under the cursor: what is dragged, named, and for
/// files their icon above the name.
pub struct DragPreview {
    count: usize,
    label: String,
    position: Point<Pixels>,
    /// The files' icon and the point of it under the pointer.
    icon: Option<(Arc<RenderImage>, Point<Pixels>)>,
    /// What the files' icon draws, while an entry's icon is drawn for it:
    /// drawn again with each that arrives, in place of its glyph.
    look: Option<crate::dragicon::DragLook>,
    shell: Option<gpui::WeakEntity<Shell>>,
}

impl DragPreview {
    /// One thing dragged inside the window, under its name.
    pub fn named(label: String, position: Point<Pixels>) -> Self {
        DragPreview {
            count: 1,
            label,
            position,
            icon: None,
            look: None,
            shell: None,
        }
    }

    /// The entries' icons drawn since: the files' icon drawn again with
    /// them, in the window and for the compositor. Asked while drawing, so
    /// that the preview is drawn again when one arrives.
    fn take_icons(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(look), Some(shell)) = (self.look.as_mut(), self.shell.as_ref()) else {
            return;
        };
        if !look.take_icons(window, cx) {
            return;
        }
        let drawn = look.drawn();
        if let Ok(Some(icon)) =
            shell.update(cx, |shell, cx| shell.show_files_icon(drawn, window, cx))
        {
            self.icon = Some(icon);
        }
    }
}

impl Render for DragPreview {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.take_icons(window, cx);
        let theme = cx.global::<Theme>().clone();
        let text = if self.count == 1 {
            self.label.clone()
        } else {
            crate::format::count(self.count, "item")
        };
        // A key held says what the drop will do wherever it lands; without
        // one it depends on where, and the label does not guess.
        let text = match held(window.modifiers()) {
            Some(DropOp::Copy) => format!("Copy {text}"),
            Some(DropOp::Move) => format!("Move {text}"),
            Some(DropOp::Link) => format!("Link {text}"),
            None => text,
        };
        let label = div()
            .px_2()
            .py_1()
            .rounded_md()
            .bg(theme.accent)
            .text_color(theme.on_accent)
            .text_xs()
            .shadow_md()
            .child(text);
        let Some((image, hotspot)) = &self.icon else {
            return div()
                .pl(self.position.x + px(12.))
                .pt(self.position.y + px(12.))
                .child(label);
        };
        // Where the compositor puts the same icon once the drag leaves.
        let size = image.size(0);
        div().child(
            div()
                .absolute()
                .left(self.position.x - hotspot.x)
                .top(self.position.y - hotspot.y)
                .flex()
                .flex_col()
                .items_center()
                .gap_1()
                .child(
                    img(image.clone())
                        .w(px(size.width.0 as f32))
                        .h(px(size.height.0 as f32)),
                )
                .child(label),
        )
    }
}

impl Shell {
    /// The paths of `pane`'s selection: what a drag from it carries.
    pub fn drag_paths(&self, pane: PaneId) -> Vec<PathBuf> {
        let ws = self.app.workspace.read();
        ws.pane(pane)
            .map(|pane| {
                pane.selection
                    .iter()
                    .filter_map(|id| pane.path_of(id))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The selection's paths, each with whether it opens as a folder, for a
    /// drag handed to the compositor.
    fn drag_files(&self, pane: PaneId) -> Vec<(PathBuf, bool)> {
        let ws = self.app.workspace.read();
        ws.pane(pane)
            .map(|pane| {
                pane.selection
                    .iter()
                    .filter_map(|id| {
                        let dir = pane.entry(id).is_some_and(skwir_core::Entry::is_dir);
                        Some((pane.path_of(id)?, dir))
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Count and first name of `pane`'s selection, for the drag preview.
    fn drag_label(&self, pane: PaneId) -> (usize, String) {
        let ws = self.app.workspace.read();
        ws.pane(pane).map_or((0, String::new()), |pane| {
            let label = pane
                .selection
                .iter()
                .next()
                .and_then(|id| pane.entry(id))
                .map(|e| e.name_str().into_owned())
                .unwrap_or_default();
            (pane.selection.len(), label)
        })
    }

    /// `icon` as the files' icon: drawn under the pointer (the image and
    /// its point under it, returned) and handed to the compositor if the
    /// drag leaves the window. The icon before it is drawn no more.
    fn show_files_icon(
        &mut self,
        icon: gpui::ExternalDragIcon,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<(Arc<RenderImage>, Point<Pixels>)> {
        if let Some(last) = self.files_icon.take() {
            cx.drop_image(last.image, Some(window));
        }
        let hotspot = point(px(icon.hotspot_x as f32), px(icon.hotspot_y as f32));
        let image = crate::thumbs::owned_render_image(icon.width, icon.height, icon.rgba.clone())?;
        self.files_icon = Some(FilesIcon {
            icon: Arc::new(icon),
            image: image.clone(),
        });
        Some((image, hotspot))
    }

    /// Whether `paths` are exactly this pane's current selection: a drag that
    /// left through the compositor and came back to us.
    fn is_own_drag(&self, paths: &[PathBuf]) -> bool {
        same_paths(&self.drag_paths(self.pane), paths)
    }

    /// Paths dropped onto `dest`, from another application or from our own
    /// compositor-level drag: our own selection as `drop_into` says, foreign
    /// paths as the keys held say, else asked (`ask_drop`, Copy ready) or
    /// copied.
    ///
    /// Only the own-drag check (a path comparison) runs here. Identifying the
    /// dropped entries stats every path, possibly on a sleeping disk, so a
    /// worker does, in their lane. What it found then runs as a drop from a
    /// pane does: the command with the entries named, its plan applied
    /// whole, a name taken there asked about (ADR-054).
    pub fn drop_paths_into(
        &mut self,
        paths: &[PathBuf],
        dest: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.is_own_drag(paths) {
            self.drop_into(self.pane, dest, None, window, cx);
            return;
        }
        // A folder dropped on itself: no drop.
        if paths.iter().any(|p| p == dest) {
            return;
        }
        let paths: Vec<PathBuf> = paths
            .iter()
            .filter(|p| p.parent() != Some(dest))
            .cloned()
            .collect();
        if paths.is_empty() {
            return;
        }
        let dest = dest.to_path_buf();
        let modifiers = window.modifiers();
        let asks = held(modifiers).is_none() && self.drop_asks();
        let position = window.mouse_position();
        // Unasked, another application's files are copied.
        let how = drop_op(modifiers, None);
        // The drop belongs to the pane it landed on, whatever is active later.
        let pane = self.pane;
        self.off_thread(
            crate::shell::lane_of(&paths),
            "drop-entries",
            move || Dropped::of_paths(&paths),
            move |shell, dropped, window, cx| {
                let Some(args) = dropped.args(&dest, Some(pane)) else {
                    shell.set_status("The dropped items no longer exist", true, cx);
                    return;
                };
                if asks {
                    shell.ask_drop(position, &dropped, &dest, how, window, cx);
                } else {
                    shell.run(how.command(), args, window, cx);
                }
            },
            window,
            cx,
        );
    }

    /// Drop `from`'s selection into `dest`, as `how` or the keys held say,
    /// else asked (`ask_drop`) or as `drop_op` says. The pane the drag
    /// started in says what goes, whichever pane is active when it lands.
    /// Without a key the destination's filesystem says what an unasked
    /// drop does: asking it can block on a sleeping disk, so a worker does,
    /// and the keys and the pointer are read now, when the drop happens.
    pub fn drop_into(
        &mut self,
        from: PaneId,
        dest: &Path,
        how: Option<DropOp>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // A place dragged into a folder is copied there as its location
        // says (`Verbs::export`: a tag's, a folder of its files); files go
        // as files do.
        // Dropped back on the folder that holds them, or on a folder among
        // them (its own row): nothing to do.
        let home = {
            let ws = self.app.workspace.read();
            ws.pane(from).is_some_and(|p| {
                let location = skwir_core::Location::from_path(dest);
                !p.selection.is_empty()
                    && (p.selection.iter().all(|id| p.holder(id) == Some(&location))
                        || p.selection
                            .iter()
                            .any(|id| p.path_of(id).as_deref() == Some(dest)))
            })
        };
        if home {
            return;
        }
        let (places, files) = {
            let ws = self.app.workspace.read();
            ws.pane(from).map_or((Vec::new(), 0), |p| {
                let places: Vec<skwir_core::Location> = p
                    .selection
                    .iter()
                    .filter_map(|id| p.location_of(id))
                    .collect();
                (
                    places,
                    p.selection.iter().filter(|id| !id.is_place()).count(),
                )
            })
        };
        // What a place gives a folder is a plan reviewed already: a drop
        // with places in it is not asked about.
        let ask = places.is_empty() && self.drop_asks();
        for place in places {
            if let Some(export) = self.app.verbs(&place).export {
                let mut args = export.args.clone();
                if !args.is_object() {
                    args = serde_json::json!({});
                }
                args["dest"] = serde_json::json!(PathArg::from(dest));
                self.run(&export.name, args, window, cx);
            }
        }
        if files == 0 {
            return;
        }
        let modifiers = window.modifiers();
        // What was dropped, named now: the selection may change before a
        // worker answers or the user does, and a command handed no
        // entries acts on the selection of its moment (`App::targets`).
        let dropped = self.dropped_from(from);
        let Some(args) = dropped.args(dest, Some(from)) else {
            return;
        };
        // Said (Shift+F5, Shift+F6), or by the keys held.
        if let Some(how) = how.or_else(|| held(modifiers)) {
            return self.run(how.command(), args, window, cx);
        }
        let asked = ask.then(|| (dropped, window.mouse_position()));
        let devices: std::collections::HashSet<u64> = {
            let ws = self.app.workspace.read();
            ws.pane(from)
                .map(|p| p.selection.iter().map(|id| id.file.dev).collect())
                .unwrap_or_default()
        };
        let dest = dest.to_path_buf();
        let dest_dir = dest.clone();
        self.off_thread(
            fs::reach::lane(&dest),
            "drop-device",
            move || fs::stat::id_of(&dest).map(|id| id.dev),
            move |shell, dev, window, cx| {
                let same = dev.map(|dev| devices.len() == 1 && devices.contains(&dev));
                let how = drop_op(modifiers, same);
                match asked {
                    Some((dropped, position)) => {
                        shell.ask_drop(position, &dropped, &dest_dir, how, window, cx);
                    }
                    None => shell.run(how.command(), args, window, cx),
                }
            },
            window,
            cx,
        );
    }

    /// Whether a drop with no key held asks what it does
    /// (`core.drop_asks`).
    fn drop_asks(&self) -> bool {
        self.app
            .settings
            .get(skwir_core::settings::DROP_ASKS)
            .unwrap_or(true)
    }

    /// What a drag from `from` drops: its selection, and what each entry
    /// is, read now.
    fn dropped_from(&self, from: PaneId) -> Dropped {
        let types = {
            let ws = self.app.workspace.read();
            ws.pane(from)
                .map(|p| {
                    p.selection
                        .iter()
                        .filter(|id| !id.is_place())
                        .filter_map(|id| p.entry(id))
                        .map(|e| (e.type_name().to_owned(), e.inode_type()))
                        .collect()
                })
                .unwrap_or_default()
        };
        Dropped {
            entries: self.app.selected_refs(from),
            types,
        }
    }

    /// Ask what `dropped` in `dest` does: a menu at `position` of Move,
    /// Copy and Link Here, each beside the keys that do it unasked
    /// (Dolphin's, which teach them), what plugins offer for what was
    /// dropped (the registry's `drop` items whose `when` holds for it),
    /// then Cancel. The row of what the drop would do unasked (`usual`)
    /// is highlighted, so Enter does what it always did. Each row is a
    /// command with the entries and the folder named (`entries`, `dest`),
    /// so what runs is what was dropped.
    pub fn ask_drop(
        &mut self,
        position: Point<Pixels>,
        dropped: &Dropped,
        dest: &Path,
        usual: DropOp,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let entries = &dropped.entries;
        let Some(args) = dropped.args(dest, None) else {
            return;
        };
        let (ctx, types) = dropped.when();
        let mut offered: Vec<MenuItem> = self
            .app
            .registry
            .menus
            .all()
            .into_iter()
            .filter(|item| {
                item.menu == "drop" && crate::menu::when_holds(item.when.as_deref(), ctx, &types)
            })
            .map(|item| MenuItem {
                command: item
                    .command
                    .clone()
                    .with("entries", entries)
                    .with("dest", PathArg::from(dest)),
                ..item
            })
            .collect();
        offered.sort_by_key(|item| item.order);
        let mut items: Vec<MenuItem> = DropOp::ALL
            .iter()
            .zip(0..)
            .map(|(how, order)| {
                MenuItem::new(
                    "drop",
                    how.title(),
                    how.icon(),
                    order,
                    Invocation::with_args(how.command(), &args),
                )
            })
            .collect();
        items.extend(offered);
        items.push(MenuItem::new(
            "drop",
            "Cancel",
            "close",
            900,
            Invocation::new("ui.menu_close"),
        ));
        let mut menu = crate::menu::MenuState::open(position, items, window, cx);
        menu.keys = DropOp::ALL
            .iter()
            .map(|how| Some(crate::keymap::display_keys(how.keys())))
            .collect();
        menu.highlighted = DropOp::ALL.iter().position(|how| *how == usual);
        self.menu = Some(menu);
        cx.notify();
    }

    /// The selection offered to the folder the other pane shows, from the
    /// keyboard (Shift+F7): the menu a drop asks with (`ask_drop`), hung
    /// from the cursor's row, Copy lit. What plugins offer for what is
    /// sent is there too: Extract Here for an archive. Elsewhere than a
    /// folder (a tag's place), as a drop there does.
    pub fn ask_other_pane(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(other) = self.other_pane() else {
            self.set_status("There is no other pane: split the window first", false, cx);
            return;
        };
        let Some(dest) = self.pane_folder(other) else {
            self.give_to_pane(self.pane, other, None, window, cx);
            return;
        };
        let dropped = self.dropped_from(self.pane);
        if dropped.entries.is_empty() {
            return;
        }
        if let Some(ix) = self.cursor_index() {
            self.with_view(|v, _| v.scroll_to(ix));
        }
        self.ask_drop(Point::default(), &dropped, &dest, DropOp::Copy, window, cx);
        let pane = self.pane;
        if let Some(menu) = &mut self.menu {
            menu.keyboard = Some(crate::menu::KeyboardAnchor::new(
                crate::menu::MenuHost::Pane(pane),
            ));
        }
    }

    /// `from`'s selection given to what pane `to` shows, as a drag onto
    /// it does and Shift+F5 and Shift+F6 do: into its folder, or to its
    /// location as its provider says a drop means there (`Verbs::drop`: a
    /// tag's gives its tag). `how` says copy or move; `None` lets the keys
    /// held decide, as a drag does.
    pub fn give_to_pane(
        &mut self,
        from: PaneId,
        to: PaneId,
        how: Option<DropOp>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(dest) = self.pane_folder(to) {
            self.drop_into(from, &dest, how, window, cx);
            return;
        }
        let taken = self
            .app
            .pane_location(to)
            .is_some_and(|place| self.drop_on_place(from, &place, how, window, cx));
        if !taken {
            self.set_status("Files cannot be dropped here", true, cx);
        }
    }

    /// The folder `pane` shows, if it is one.
    pub fn pane_folder(&self, pane: PaneId) -> Option<PathBuf> {
        let ws = self.app.workspace.read();
        ws.pane(pane)
            .and_then(|p| p.location().and_then(skwir_core::Location::path))
    }

    /// Paths dropped onto the file area: into the current directory.
    pub fn drop_external(
        &mut self,
        paths: &[PathBuf],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(dest) = self.pane_folder(self.pane) else {
            // A location that takes files (a tag's) says what a drop does.
            let shown = self.app.pane_location(self.pane);
            if !shown.is_some_and(|place| self.drop_paths_on_place(paths, &place, window, cx)) {
                self.set_status("Files cannot be dropped here", true, cx);
            }
            return;
        };
        self.drop_paths_into(paths, &dest, window, cx);
    }
}

/// Whether `dropped` are exactly `own`, a selection's paths (each once), in
/// any order. A drag of a hundred thousand files comes back as many paths:
/// each looked up in a set, never searched for in the others.
fn same_paths(own: &[PathBuf], dropped: &[PathBuf]) -> bool {
    if own.is_empty() || own.len() != dropped.len() {
        return false;
    }
    let dropped: std::collections::HashSet<&Path> = dropped.iter().map(PathBuf::as_path).collect();
    own.iter().all(|p| dropped.contains(p.as_path()))
}

/// Make a row or cell of `pane` draggable. Every row is, selected or not:
/// GPUI only starts a drag from an element that had a drag listener when it
/// was pressed, and the press is what selects the row. What the drag
/// carries (paths, label, icon) is built when it starts, not per frame.
pub fn draggable(el: Stateful<Div>, pane: PaneId, cx: &mut Context<Shell>) -> Stateful<Div> {
    let shell = cx.weak_entity();
    let files = shell.clone();
    el.on_drag(
        DragPayload { pane },
        move |p: &DragPayload, position, window, cx| {
            let pane = p.pane;
            let tint = cx.global::<Theme>().text_muted;
            let ((count, label), icon, look) = shell
                .update(cx, |shell, cx| {
                    // A drag is not the click that collapses a
                    // multi-selection: the row pressed is the one it took.
                    let grabbed = shell
                        .press_collapse
                        .take()
                        .and_then(|(at, id)| (at == pane).then_some(id));
                    let label = shell.drag_label(pane);
                    let mut look = shell.drag_look(pane, grabbed, tint);
                    let icon = shell.show_files_icon(look.drawn(), window, cx);
                    let waits = look.cards.iter().any(|card| card.coming.is_some());
                    (label, icon, waits.then_some(look))
                })
                .unwrap_or_default();
            cx.new(|_| DragPreview {
                count,
                label,
                position,
                icon,
                look,
                shell: Some(shell.clone()),
            })
        },
    )
    // Out of the window, the drag is the compositor's (gpui hands it over
    // as it leaves), under the same icon: other applications take the
    // files, and brought back it is this drag again.
    .external_drag_payload(move |p: &DragPayload, _, cx| {
        let shell = files.upgrade()?;
        let shell = shell.read(cx);
        let files = shell.drag_files(p.pane);
        let files = gpui::FileDragPaths::new(files);
        let files = match &shell.files_icon {
            Some(icon) => files.with_icon(icon.icon.clone()),
            None => files,
        };
        (!files.entries().is_empty()).then_some(gpui::ExternalDragPayload::Files(files))
    })
}

/// Make an element a drop target into `dir`, for in-window drags and for
/// compositor drags (from other applications, or our own that went external).
pub fn droppable_dir<E: InteractiveElement>(el: E, dir: PathBuf, cx: &mut Context<Shell>) -> E {
    let theme = cx.global::<Theme>().clone();
    let (tint, edge) = (theme.drop_target, theme.accent);
    let dir2 = dir.clone();
    el.drag_over::<DragPayload>(move |s, _, _, _| s.bg(tint).border_color(edge))
        .on_drop(cx.listener(move |this, payload: &DragPayload, window, cx| {
            this.drop_into(payload.pane, &dir, None, window, cx);
        }))
        .drag_over::<ExternalPaths>(move |s, _, _, _| s.bg(tint).border_color(edge))
        .on_drop(cx.listener(move |this, paths: &ExternalPaths, window, cx| {
            this.drop_paths_into(paths.paths(), &dir2, window, cx);
        }))
}

/// Make an element a drop target onto `place`, a location that is no
/// folder but takes files (`Verbs::drop`: a tag's gives them its tag),
/// for in-window drags and for compositor drags.
pub fn droppable_place<E: InteractiveElement>(
    el: E,
    place: skwir_core::Location,
    cx: &mut Context<Shell>,
) -> E {
    let theme = cx.global::<Theme>().clone();
    let (tint, edge) = (theme.drop_target, theme.accent);
    let place2 = place.clone();
    el.drag_over::<DragPayload>(move |s, _, _, _| s.bg(tint).border_color(edge))
        .on_drop(cx.listener(move |this, payload: &DragPayload, window, cx| {
            cx.stop_propagation();
            this.drop_on_place(payload.pane, &place, None, window, cx);
        }))
        .drag_over::<ExternalPaths>(move |s, _, _, _| s.bg(tint).border_color(edge))
        .on_drop(cx.listener(move |this, paths: &ExternalPaths, window, cx| {
            cx.stop_propagation();
            this.drop_paths_on_place(paths.paths(), &place2, window, cx);
        }))
}

impl Shell {
    /// `from`'s selection dropped on `place`, a location that is no
    /// folder: what its provider says a drop means there (`Verbs::drop`),
    /// told whether the files move (not with Ctrl: a tag's then takes
    /// them out of the one they were dragged from). Returns whether
    /// `place` takes drops. The trash keeps files, not copies of them
    /// (`Verbs::restore`): a copy asked for (Shift+F5) is refused there,
    /// while a drag puts them in the trash whatever keys are held, as
    /// every file manager's Trash takes one.
    pub fn drop_on_place(
        &mut self,
        from: PaneId,
        place: &skwir_core::Location,
        how: Option<DropOp>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let verbs = self.app.verbs(place);
        let Some(drop) = verbs.drop else {
            return false;
        };
        if verbs.restore.is_some() && how.is_some_and(|how| how != DropOp::Move) {
            self.set_status(
                "Files cannot be copied to the trash: move them there instead",
                true,
                cx,
            );
            return true;
        }
        // Where the files come from, when it is not the place the pane
        // shows: a folder or a place opened in it (a board's column), which
        // a move takes them out of.
        let held = {
            let ws = self.app.workspace.read();
            ws.pane(from).and_then(|p| {
                let first = p.selection.iter().next()?;
                let holder = p.holder(first)?.clone();
                Some((p.location() == Some(&holder), holder))
            })
        };
        if held.as_ref().is_some_and(|(_, holder)| holder == place) {
            // Back where it came from.
            return true;
        }
        // What the drop says goes only where its command takes it: a
        // tag's drop is told whether the files move and from where, the
        // trash's and the shelf's take the files alone, and an argument a
        // command does not take is refused (`CommandSpec::accepts`).
        let spec = self.app.commands.get(&drop.name);
        let takes = |name: &str| spec.as_ref().is_some_and(|spec| spec.accepts(name));
        let mut drop = drop.clone();
        if takes("pane") {
            drop = drop.with("pane", from);
        }
        if takes("move") {
            drop = drop.with(
                "move",
                how.map_or(!window.modifiers().control, |how| how == DropOp::Move),
            );
        }
        if let Some((false, holder)) = held
            && takes("from")
        {
            drop = drop.with("from", holder.to_string());
        }
        self.run(&drop.name, drop.args, window, cx);
        true
    }

    /// Paths from another application (or our own drag back through the
    /// compositor) dropped on `place`, as `drop_on_place`.
    pub fn drop_paths_on_place(
        &mut self,
        paths: &[PathBuf],
        place: &skwir_core::Location,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.is_own_drag(paths) {
            return self.drop_on_place(self.pane, place, None, window, cx);
        }
        let Some(drop) = self.app.verbs(place).drop else {
            return false;
        };
        // Who each is, read from the disk on a worker in their lane, as a
        // drop into a folder reads them.
        let paths = paths.to_vec();
        self.off_thread(
            crate::shell::lane_of(&paths),
            "drop-on-place",
            move || {
                let entries: Vec<EntryRef> = paths.iter().filter_map(EntryRef::from_path).collect();
                entries
            },
            move |shell, entries, window, cx| {
                // None named would take the selection.
                if entries.is_empty() {
                    shell.set_status("The dropped items no longer exist", true, cx);
                    return;
                }
                let mut args = drop.args.clone();
                if !args.is_object() {
                    args = serde_json::json!({});
                }
                args["entries"] = serde_json::json!(entries);
                shell.run(&drop.name, args, window, cx);
            },
            window,
            cx,
        );
        true
    }
}

/// Make an entry's row take what a dragged place gives files (a tag's
/// pill gives its tag): the row's entry, or the pane's selection when the
/// row is in it, as a drop of files acts on the selection.
pub fn takes_gifts<E: InteractiveElement>(
    el: E,
    pane: PaneId,
    id: EntryId,
    cx: &mut Context<Shell>,
) -> E {
    let theme = cx.global::<Theme>().clone();
    let (tint, edge) = (theme.drop_target, theme.accent);
    el.drag_over::<crate::sidebar::PlaceDrag>(move |s, drag, _, _| {
        if drag.gives.is_some() {
            s.bg(tint).border_color(edge)
        } else {
            s
        }
    })
    .on_drop(
        cx.listener(move |this, drag: &crate::sidebar::PlaceDrag, window, cx| {
            if let Some(gives) = &drag.gives {
                cx.stop_propagation();
                this.with_pane(pane, |this| this.give(gives, id, window, cx));
            }
        }),
    )
}

/// Make a row of an entry that is not the pane's (a column of the folders
/// above) take what a dragged place gives files, for that entry alone.
pub fn takes_gift_for<E: InteractiveElement>(
    el: E,
    entry: skwir_core::op::EntryRef,
    cx: &mut Context<Shell>,
) -> E {
    let theme = cx.global::<Theme>().clone();
    let (tint, edge) = (theme.drop_target, theme.accent);
    el.drag_over::<crate::sidebar::PlaceDrag>(move |s, drag, _, _| {
        if drag.gives.is_some() {
            s.bg(tint).border_color(edge)
        } else {
            s
        }
    })
    .on_drop(
        cx.listener(move |this, drag: &crate::sidebar::PlaceDrag, window, cx| {
            if let Some(gives) = &drag.gives {
                cx.stop_propagation();
                let mut args = gives.args.clone();
                args["entries"] = serde_json::json!([entry]);
                this.run(&gives.name, args, window, cx);
            }
        }),
    )
}

impl Shell {
    /// Run `gives` (a place's `drop`) for entry `id` of the current pane,
    /// or for its selection when the entry is in it.
    fn give(
        &mut self,
        gives: &skwir_core::command::Invocation,
        id: EntryId,
        window: &mut Window,
        cx: &mut Context<Shell>,
    ) {
        let pane = self.pane;
        let target = {
            let ws = self.app.workspace.read();
            let Some(p) = ws.pane(pane) else { return };
            // Gone since it was drawn: nothing to give to.
            let Some(entry) = p.entry(id) else {
                return;
            };
            if p.selection.contains(id) {
                None
            } else {
                let only = entry.is_only_name();
                p.path_of(id)
                    .map(|path| skwir_core::op::EntryRef::new(id.file, path).with_only_name(only))
            }
        };
        let mut args = gives.args.clone();
        match target {
            Some(entry) => args["entries"] = serde_json::json!([entry]),
            None => args["pane"] = serde_json::json!(pane),
        }
        self.run(&gives.name, args, window, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_drag_back_is_its_own_selection_in_any_order() {
        let paths = |names: &[&str]| -> Vec<PathBuf> {
            names.iter().map(|n| Path::new("/data").join(n)).collect()
        };
        let own = paths(&["a b.txt", "it's", "notes"]);
        assert!(same_paths(&own, &paths(&["notes", "a b.txt", "it's"])));
        assert!(!same_paths(&own, &paths(&["notes", "a b.txt"])));
        assert!(!same_paths(&own, &paths(&["notes", "a b.txt", "other"])));
        assert!(!same_paths(&paths(&["a", "b"]), &paths(&["a", "a"])));
        assert!(!same_paths(&[], &[]));
        // A hundred thousand, as many as a big folder selected whole: one
        // pass, where searching each in the others took minutes.
        let many: Vec<PathBuf> = (0..100_000)
            .map(|i| PathBuf::from(format!("/data/file {i}.png")))
            .collect();
        let back: Vec<PathBuf> = many.iter().rev().cloned().collect();
        assert!(same_paths(&many, &back));
    }

    #[test]
    fn keys_decide_a_drop_and_the_filesystem_decides_without_them() {
        let none = gpui::Modifiers::default();
        let ctrl = gpui::Modifiers {
            control: true,
            ..Default::default()
        };
        let shift = gpui::Modifiers {
            shift: true,
            ..Default::default()
        };
        let both = gpui::Modifiers {
            control: true,
            shift: true,
            ..Default::default()
        };
        assert_eq!(drop_op(none, Some(true)), DropOp::Move);
        assert_eq!(drop_op(none, Some(false)), DropOp::Copy);
        // Not known: nothing is taken from where it was by a guess.
        assert_eq!(drop_op(none, None), DropOp::Copy);
        assert_eq!(drop_op(ctrl, Some(true)), DropOp::Copy);
        assert_eq!(drop_op(shift, Some(false)), DropOp::Move);
        assert_eq!(drop_op(shift, None), DropOp::Move);
        assert_eq!(drop_op(both, Some(true)), DropOp::Link);
        // No key held: nothing said, the drop asks or decides.
        assert_eq!(held(none), None);
        // What the menu says each row's keys are is what they do.
        for how in DropOp::ALL {
            let keys = crate::keymap::parse_keystroke(&format!("{}-a", how.keys())).unwrap();
            assert_eq!(held(keys.modifiers), Some(how), "{how:?}");
        }
    }

    /// A drop's items ask what was dropped, by name, a folder by what it
    /// is: an archive offers Extract Here, a text or a folder named like
    /// an archive does not.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_drops_menu_reads_what_was_dropped() {
        let dropped = |types: &[(&str, bool)]| Dropped {
            entries: Vec::new(),
            types: types
                .iter()
                .map(|(name, folder)| ((*name).into(), folder.then_some("inode/directory")))
                .collect(),
        };
        let holds = |dropped: &Dropped, when: &str| {
            let (ctx, types) = dropped.when();
            crate::menu::when_holds(Some(when), ctx, &types)
        };
        let archives = "kind:archive|kind:application/x-cd-image";
        assert!(holds(&dropped(&[("it's photos.zip", false)]), archives));
        assert!(holds(
            &dropped(&[("a.tar.gz", false), ("b.7z", false)]),
            archives
        ));
        assert!(holds(&dropped(&[("disc.iso", false)]), archives));
        assert!(!holds(&dropped(&[("notes.txt", false)]), archives));
        assert!(!holds(
            &dropped(&[("a.zip", false), ("notes.txt", false)]),
            archives
        ));
        assert!(!holds(&dropped(&[("not an archive.zip", true)]), archives));
    }

    /// What a drop runs names what was dropped: the command takes those
    /// entries, whatever the pane selects by the time it runs (here,
    /// nothing). Nothing named runs nothing, since a command handed no
    /// entries takes the selection.
    #[test]
    fn a_drop_runs_on_what_was_dropped() {
        let base = scratch("named");
        let (src, dest) = (base.join("from"), base.join("into ‘here’"));
        std::fs::create_dir_all(&src).unwrap();
        std::fs::create_dir_all(&dest).unwrap();
        let file = src.join("it's a file.txt");
        std::fs::write(&file, b"hello").unwrap();
        let config = base.join("config");
        let app = skwir_core::App::new(skwir_core::app::AppConfig::ephemeral(&config)).unwrap();
        let dropped = Dropped::of_paths(std::slice::from_ref(&file));
        let args = dropped.args(&dest, Some(PaneId(7))).expect("named");
        assert_eq!(args["pane"], 7);
        let args = dropped.args(&dest, None).expect("named");
        let outcome = app.dispatch(&Invocation::with_args("copy", &args)).unwrap();
        let skwir_core::command::Outcome::Plan(plan) = outcome else {
            panic!("a plan");
        };
        assert_eq!(plan.steps.len(), 1);
        assert!(matches!(
            plan.steps[0].describe(),
            skwir_core::op::Change::Copy { from, .. } if from == file
        ));
        assert!(Dropped::default().args(&dest, None).is_none());
        let _ = std::fs::remove_dir_all(&base);
    }

    /// A scratch tree under the user's cache, never `/tmp` (see AGENTS.md).
    fn scratch(name: &str) -> PathBuf {
        // Windows' (and Wine's) home is its user's profile.
        let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"));
        let base = PathBuf::from(home.expect("home"))
            .join(".cache")
            .join("skwir-tests")
            .join(format!("dnd-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).expect("scratch");
        base
    }

    /// Another application's files dropped run the command a drop from a
    /// pane runs: a path gone drops nothing, and a name taken where they
    /// go is asked about (ADR-054), never refused whole nor replaced.
    #[test]
    fn dropped_paths_ask_what_becomes_of_a_name_taken() {
        let base = scratch("plan");
        let (src, dest) = (base.join("from"), base.join("into"));
        std::fs::create_dir_all(&src).unwrap();
        std::fs::create_dir_all(&dest).unwrap();
        let file = src.join("it's a file.txt");
        std::fs::write(&file, b"hello").unwrap();
        let taken = dest.join("it's a file.txt");
        std::fs::write(&taken, b"other contents").unwrap();
        let gone = Dropped::of_paths(&[src.join("not there ‘any more’")]);
        assert!(gone.args(&dest, None).is_none());

        let app = skwir_core::App::new(skwir_core::app::AppConfig::ephemeral(base.join("config")))
            .unwrap();
        app.set_window_shown();
        let args = Dropped::of_paths(std::slice::from_ref(&file))
            .args(&dest, None)
            .expect("named");
        let outcome = app.dispatch(&Invocation::with_args("copy", &args)).unwrap();
        let skwir_core::command::Outcome::Plan(plan) = outcome else {
            panic!("a plan");
        };
        app.apply_whole(plan);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let asked = loop {
            let asked = app.take_clash_questions();
            if !asked.is_empty() {
                break asked;
            }
            assert!(std::time::Instant::now() < deadline, "never asked");
            std::thread::sleep(std::time::Duration::from_millis(5));
        };
        assert_eq!(asked.len(), 1);
        assert_eq!(std::fs::read(&taken).unwrap(), b"other contents");
        drop(app);
        let _ = std::fs::remove_dir_all(&base);
    }
}
