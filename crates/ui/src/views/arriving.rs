//! What a plan brings into the folder shown, drawn where it will land
//! before it does (ADR-135): its row or its tile as its file's will be,
//! dimmed, a tint behind it from its start to how far it has come, and a
//! pill saying how far ("42 %", "waiting"). It is no entry: never selected
//! nor a stop of the keys (the views place it among their rows, not their
//! positions, `groups::RowMap`); pointed at, it says where it comes from
//! and how long its action has left; pressed, it opens its action's card in
//! Activity. An entry already listed under its name (a folder merged into)
//! is drawn as itself, with the tint and the pill.

use gpui::{Context, Div, InteractiveElement, Stateful, div, prelude::*, px, relative};
use skwir_core::op::arriving::{Arrives, Arriving};
use skwir_core::words;
use skwir_core::words::Folder;

use crate::shell::Shell;
use crate::sizes::TextSizes;
use crate::theme::Theme;

/// How much of what arrives shows through: readable, behind what is there.
pub const OPACITY: f32 = 0.6;

/// How far it has come, from 0 to 1, for its tint.
pub fn fraction(arriving: &Arriving) -> f32 {
    match arriving.state {
        Arrives::Moving(fraction) | Arrives::Copying(fraction) => fraction,
        Arrives::Landed => 1.,
        Arrives::Waiting | Arrives::Starting => 0.,
    }
}

/// Its pill's words: how far it has come, or why it has not.
pub fn words(arriving: &Arriving) -> String {
    match arriving.state {
        _ if arriving.paused => words!(
            views_arriving_paused = "paused",
            note = "On a pill beside a file a copy brings into the folder: its copy is paused."
        )
        .say(),
        Arrives::Waiting => words!(
            views_arriving_waiting = "waiting",
            note = "On a pill beside a file a copy brings into the folder: its copy waits its turn."
        )
        .say(),
        Arrives::Starting => words!(
            views_arriving_starting = "starting",
            note = "On a pill beside a file a copy brings into the folder: its copy is starting."
        )
        .say(),
        Arrives::Copying(_) => words!(
            views_arriving_copying = "copying",
            note = "On a pill beside a file a copy brings into the folder: it is being copied, how far not known."
        )
        .say(),
        Arrives::Moving(fraction) => crate::activity::percent_words(fraction).say(),
        Arrives::Landed => crate::activity::percent_words(1.).say(),
    }
}

/// What a screen reader hears of it beside its name, as its pill says it.
pub fn said(arriving: &Arriving) -> String {
    match arriving.state {
        _ if arriving.paused => words!(
            views_arriving_said_paused = "arriving, paused at { $percent }",
            percent = percent_of(arriving),
            note = "What a screen reader hears after the name of a file a copy brings into the folder. percent: how far it came (42%)."
        ),
        Arrives::Waiting => words!(
            views_arriving_said_waiting = "arriving, waiting",
            note = "What a screen reader hears after the name of a file a copy brings into the folder: its copy waits its turn."
        ),
        Arrives::Starting => words!(
            views_arriving_said_starting = "arriving, starting",
            note = "What a screen reader hears after the name of a file a copy brings into the folder."
        ),
        Arrives::Copying(_) => words!(
            views_arriving_said_copying = "arriving, copying",
            note = "What a screen reader hears after the name of a file a copy brings into the folder."
        ),
        Arrives::Moving(_) | Arrives::Landed => words!(
            views_arriving_said_at = "arriving, { $percent }",
            percent = percent_of(arriving),
            note = "What a screen reader hears after the name of a file a copy brings into the folder. percent: how far it came (42%)."
        ),
    }
    .say()
}

fn percent_of(arriving: &Arriving) -> skwir_core::words::Words {
    crate::activity::percent_words(fraction(arriving))
}

/// The pill: Small, a count; the accent's fill under a share moving, the
/// muted ink on the files' ground otherwise.
pub fn pill(arriving: &Arriving, theme: &Theme) -> Div {
    let moving = matches!(arriving.state, Arrives::Moving(_) | Arrives::Landed) && !arriving.paused;
    div()
        .flex_none()
        .px(crate::sizes::sized(6.))
        .rounded_full()
        .text_small()
        .whitespace_nowrap()
        .map(|d| {
            if moving {
                d.bg(theme.accent).text_color(theme.on_accent)
            } else {
                d.border_1()
                    .border_color(theme.border)
                    .text_color(theme.text_muted)
            }
        })
        .child(words(arriving))
}

/// How far short of its end the tint stops, whole: a row or a tile tinted
/// end to end reads as one chosen.
const SHORT: f32 = 12.;

/// The pill of a file another program writes still (ADR-135,
/// `Look::writing`): Small, in the muted ink, as what waits says it.
pub fn writing_pill(theme: &Theme) -> Div {
    div()
        .flex_none()
        .px(crate::sizes::sized(6.))
        .rounded_full()
        .text_small()
        .whitespace_nowrap()
        .border_1()
        .border_color(theme.border)
        .text_color(theme.text_muted)
        .child(
            words!(
                views_writing = "writing",
                note = "On a pill beside a file another program is still writing."
            )
            .say(),
        )
}

/// The tint behind it, from its start to how far it has come, stopping
/// `SHORT` before its end: laid under what it draws, as the highlight of a
/// line found is (`highlight`).
pub fn tint(arriving: &Arriving, theme: &Theme) -> Div {
    div()
        .absolute()
        .top_0()
        .bottom_0()
        .left_0()
        .right(px(SHORT))
        .child(
            div()
                .h_full()
                .w(relative(fraction(arriving)))
                .bg(theme.highlight),
        )
}

/// What pointing at it says: where it comes from and how its action goes,
/// read as it is pointed at (`acting`), never as each row is drawn.
fn tip(app: &skwir_core::App, arriving: &Arriving) -> String {
    let arrival = &arriving.arrival;
    let from = arrival
        .from
        .as_deref()
        .and_then(std::path::Path::parent)
        .map(|folder| {
            let name = folder.file_name().map_or_else(
                || folder.display().to_string(),
                |name| name.to_string_lossy().into_owned(),
            );
            words!(
                views_arriving_from = "From { $folder }",
                folder = Folder(name),
                note =
                    "Pointing at a file a copy brings into the folder: the folder it comes from."
            )
            .say()
        });
    let left = app
        .executor
        .transfer(arrival.plan)
        .and_then(|transfer| transfer.left)
        .map(|left| crate::format::time_left_words(left).say());
    let how = match arriving.state {
        _ if arriving.paused => words!(
            views_arriving_tip_paused = "paused",
            note = "Pointing at a file a copy brings into the folder: its copy is paused."
        )
        .say(),
        Arrives::Waiting => words!(
            views_arriving_tip_waits = "waits its turn",
            note = "Pointing at a file a copy brings into the folder: its copy waits for others before it."
        )
        .say(),
        Arrives::Starting => words!(
            views_arriving_tip_starting = "getting ready",
            note = "Pointing at a file a copy brings into the folder: its copy is starting."
        )
        .say(),
        // Its server's tool says nothing of each file until it is whole.
        Arrives::Copying(_) => words!(
            views_arriving_tip_copying = "copying with the others",
            note = "Pointing at a file a copy brings into the folder: a tool copies it with the others, how far not known."
        )
        .say(),
        Arrives::Moving(_) => percent_of(arriving).say(),
        Arrives::Landed => words!(
            views_arriving_tip_landed = "in place",
            note = "Pointing at a file a copy brings into the folder: it has arrived."
        )
        .say(),
    };
    [from, Some(how), left]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" · ")
}

/// What it does, given to `el`: its tooltip, and a press that opens its
/// action's card in Activity (`ui.activity`, `plan`). Never selected: the
/// press goes no further.
pub fn acting(
    el: Stateful<Div>,
    arriving: &Arriving,
    shell: &Shell,
    cx: &mut Context<Shell>,
) -> Stateful<Div> {
    let plan = arriving.arrival.plan;
    let pane = shell.pane;
    let (app, it) = (std::sync::Arc::clone(&shell.app), arriving.clone());
    el.cursor_pointer()
        .tooltip(move |_, cx| crate::toolbar::tooltip(tip(&app, &it).into(), cx))
        .on_mouse_down(
            gpui::MouseButton::Left,
            cx.listener(move |this, _: &gpui::MouseDownEvent, window, cx| {
                cx.stop_propagation();
                this.activate_pane(pane, cx);
                this.run(
                    "ui.activity",
                    serde_json::json!({ "plan": plan.0, "pointer": true }),
                    window,
                    cx,
                );
            }),
        )
        .on_mouse_down(gpui::MouseButton::Right, |_, _, cx| cx.stop_propagation())
}

/// As a screen reader hears it, where one listens: an option of the list
/// that cannot be chosen, its name, and how far it has come.
pub fn expose(el: Stateful<Div>, name: &str, arriving: &Arriving, a11y: bool) -> Stateful<Div> {
    if !a11y {
        return el;
    }
    el.role(gpui::Role::ListBoxOption)
        .aria_label(name.to_owned())
        .aria_description(said(arriving))
        .aria_disabled(true)
}

impl Shell {
    /// What arrives as every view draws it, its icon at `icon_px`: as its
    /// file will be, chosen by nothing.
    pub fn arriving_look(
        &mut self,
        arriving: &Arriving,
        icon_px: u16,
    ) -> crate::views::entry::Look {
        let arrival = &arriving.arrival;
        let e = &arrival.like;
        let path = arrival.folder.join(&arrival.name);
        crate::views::entry::Look {
            id: e.id,
            name: e.name_str().into_owned(),
            is_dir: e.is_dir(),
            selected: false,
            cursor: false,
            cut: false,
            only_leads: false,
            hidden: e.hidden_unsaid(),
            icon: self.icons.for_entry(e, || Some(path), icon_px),
            link: crate::icons::Link::of(e),
            tags: Vec::new(),
            rating: None,
            ring: None,
            place: None,
            path: None,
            plays: None,
            vcs: None,
            writing: false,
        }
    }
}

/// What arrives as a row of a column, its name alone (the columns view's):
/// its picture and name dimmed, the tint behind it, its pill after it.
pub fn compact_row(
    shell: &mut Shell,
    arriving: &Arriving,
    i: usize,
    theme: &Theme,
    cx: &mut Context<Shell>,
) -> Stateful<Div> {
    let icon_px = shell.row_zoom;
    let height = crate::views::list::row_height(icon_px, crate::views::list::files_scale(theme));
    let look = shell.arriving_look(arriving, u16::try_from(icon_px).unwrap_or(u16::MAX));
    let el = div()
        .id(("arriving-row", i))
        .relative()
        .h(px(height))
        .w_full()
        .flex()
        .items_center()
        .gap_2()
        .px_2()
        .child(tint(arriving, theme))
        .child(
            div()
                .flex_none()
                .opacity(OPACITY)
                .child(look.picture(crate::views::list::icon_px(icon_px), theme)),
        )
        .child(crate::views::entry::name(
            div()
                .id(("arriving-row-name", i))
                .flex_1()
                .min_w_0()
                .opacity(OPACITY),
            &look,
        ))
        .child(pill(arriving, theme));
    let el = acting(el, arriving, shell, cx);
    expose(el, &look.name, arriving, shell.a11y)
}
