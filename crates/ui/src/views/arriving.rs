//! What a plan brings into the folder shown, drawn where it will land
//! before it does (ADR-135): its row or its tile as its file's will be,
//! dimmed, a tint behind it from its start to how far it has come, and a
//! pill saying how far ("42 %", "waiting"). It is no entry: never selected
//! nor a stop of the keys (the views place it among their rows, not their
//! positions, `groups::RowMap`); pointed at, it says where it comes from
//! and how long its action has left; pressed, it opens its action's card in
//! Activity. An entry already listed under its name (a folder merged into)
//! is drawn as itself, with the tint and the pill.

use gpui::{Context, Div, InteractiveElement, Stateful, div, prelude::*, relative};
use skwir_core::op::arriving::{Arrives, Arriving};

use crate::shell::Shell;
use crate::sizes::TextSizes;
use crate::theme::Theme;

/// How much of what arrives shows through: readable, behind what is there.
pub const OPACITY: f32 = 0.6;

/// How far it has come, from 0 to 1, for its tint.
pub fn fraction(arriving: &Arriving) -> f32 {
    match arriving.state {
        Arrives::Moving(fraction) => fraction,
        Arrives::Waiting | Arrives::Starting => 0.,
    }
}

/// Its pill's words: how far it has come, or why it has not.
pub fn words(arriving: &Arriving) -> String {
    match arriving.state {
        _ if arriving.paused => "paused".to_owned(),
        Arrives::Waiting => "waiting".to_owned(),
        Arrives::Starting => "starting".to_owned(),
        Arrives::Moving(fraction) => crate::activity::percent(fraction),
    }
}

/// What a screen reader hears of it beside its name, as its pill says it.
pub fn said(arriving: &Arriving) -> String {
    match arriving.state {
        _ if arriving.paused => format!("arriving, paused at {}", percent_of(arriving)),
        Arrives::Waiting => "arriving, waiting".to_owned(),
        Arrives::Starting => "arriving, starting".to_owned(),
        Arrives::Moving(_) => format!("arriving, {}", percent_of(arriving)),
    }
}

fn percent_of(arriving: &Arriving) -> String {
    crate::activity::percent(fraction(arriving))
}

/// The pill: Small, a count; the accent's fill under a share moving, the
/// muted ink on the files' ground otherwise.
pub fn pill(arriving: &Arriving, theme: &Theme) -> Div {
    let moving = matches!(arriving.state, Arrives::Moving(_)) && !arriving.paused;
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

/// The tint behind it, from its start to how far it has come: laid under
/// what it draws, as the highlight of a line found is (`highlight`).
pub fn tint(arriving: &Arriving, theme: &Theme) -> Div {
    div()
        .absolute()
        .top_0()
        .left_0()
        .h_full()
        .w(relative(fraction(arriving)))
        .bg(theme.highlight)
}

/// What pointing at it says: where it comes from and how its action goes.
fn tip(shell: &Shell, arriving: &Arriving) -> String {
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
            format!("From {name}")
        });
    let left = shell
        .app
        .executor
        .transfer(arrival.plan)
        .and_then(|transfer| transfer.left)
        .map(crate::format::time_left);
    let how = match arriving.state {
        _ if arriving.paused => "paused".to_owned(),
        Arrives::Waiting => "waits its turn".to_owned(),
        Arrives::Starting => "getting ready".to_owned(),
        Arrives::Moving(_) => percent_of(arriving),
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
    let said = tip(shell, arriving);
    el.cursor_pointer()
        .tooltip(move |_, cx| crate::toolbar::tooltip(said.clone().into(), cx))
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
        }
    }
}
