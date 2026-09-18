//! Bulk rename dialog: template, regex, replacement, case, start; live
//! preview from `rename_bulk.preview`; Enter applies `rename_bulk`.

use gpui::{
    AnyElement, Context, Entity, MouseButton, Subscription, Window, deferred, div, prelude::*, px,
};

use crate::input::{InputEvent, TextInput};
use crate::keymap;
use crate::shell::Shell;
use crate::theme::Theme;

pub const CASES: [&str; 5] = ["none", "lower", "upper", "title", "sentence"];
const FIELDS: [(&str, &str); 4] = [
    (
        "Template",
        "{name}-{n:02}.{ext}  ({name} {stem} {ext} {n} {date} …)",
    ),
    ("Regex", "optional pattern over the name"),
    ("Replacement", "optional; $1 for groups"),
    ("Start", "1"),
];

pub struct BulkState {
    pub inputs: Vec<Entity<TextInput>>,
    pub case: usize,
    pub focused: usize,
    pub preview: Vec<(String, String)>,
    pub error: Option<String>,
    dirty: bool,
    _subs: Vec<Subscription>,
}

impl Shell {
    fn bulk_spec(&self, cx: &Context<Self>) -> serde_json::Value {
        let Some(b) = &self.bulk else {
            return serde_json::json!({});
        };
        let text = |i: usize| b.inputs[i].read(cx).text().trim().to_owned();
        let opt = |s: String| {
            if s.is_empty() {
                serde_json::Value::Null
            } else {
                serde_json::Value::String(s)
            }
        };
        let start: u64 = text(3).parse().unwrap_or(1);
        let case = if b.case == 0 {
            serde_json::Value::Null
        } else {
            serde_json::Value::String(CASES[b.case].into())
        };
        serde_json::json!({
            "template": opt(text(0)),
            "regex": opt(text(1)),
            "replacement": opt(text(2)),
            "case": case,
            "start": start,
            "pane": self.pane,
        })
    }

    pub fn open_bulk(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.selection_ctx().count == 0 {
            self.set_status("select the entries to rename first", true, cx);
            return;
        }
        let mut inputs = Vec::new();
        let mut subs = Vec::new();
        for (i, (_, placeholder)) in FIELDS.iter().enumerate() {
            let input = cx.new(|cx| TextInput::new(*placeholder, cx));
            if i == 0 {
                input.update(cx, |t, cx| {
                    t.set_text("{name}", cx);
                    t.select_all_now(cx);
                });
            }
            subs.push(
                cx.subscribe_in(
                    &input,
                    window,
                    |this, _, ev: &InputEvent, window, cx| match ev {
                        InputEvent::Submit => this.bulk_apply(window, cx),
                        InputEvent::Cancel => this.close_bulk(window, cx),
                        InputEvent::Changed => {
                            if let Some(b) = &mut this.bulk {
                                b.dirty = true;
                            }
                            cx.notify();
                        }
                    },
                ),
            );
            inputs.push(input);
        }
        inputs[0].update(cx, |i, _| i.focus(window));
        self.bulk = Some(BulkState {
            inputs,
            case: 0,
            focused: 0,
            preview: Vec::new(),
            error: None,
            dirty: true,
            _subs: subs,
        });
        cx.notify();
    }

    pub fn close_bulk(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.bulk = None;
        window.focus(&self.ui().focus);
        cx.notify();
    }

    pub fn bulk_focus_next(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(b) = &mut self.bulk {
            let n = b.inputs.len() as isize;
            b.focused = ((b.focused as isize + delta).rem_euclid(n)) as usize;
            b.inputs[b.focused].update(cx, |i, cx| {
                i.select_all_now(cx);
                i.focus(window);
            });
            cx.notify();
        }
    }

    pub fn bulk_cycle_case(&mut self, cx: &mut Context<Self>) {
        if let Some(b) = &mut self.bulk {
            b.case = (b.case + 1) % CASES.len();
            b.dirty = true;
            cx.notify();
        }
    }

    /// Recompute the preview when a field changed.
    fn bulk_refresh(&mut self, cx: &mut Context<Self>) {
        if !self.bulk.as_ref().is_some_and(|b| b.dirty) {
            return;
        }
        let spec = self.bulk_spec(cx);
        let (preview, error) = match self.app.dispatch(&bibli_core::command::Invocation {
            name: "rename_bulk.preview".into(),
            args: spec,
        }) {
            Ok(bibli_core::command::Outcome::Value(v)) => (
                v.as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|x| {
                                Some((
                                    x.get("from")?.as_str()?.to_owned(),
                                    x.get("to")?.as_str()?.to_owned(),
                                ))
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
                None,
            ),
            Ok(_) => (Vec::new(), None),
            Err(e) => (Vec::new(), Some(format!("{e:#}"))),
        };
        if let Some(b) = &mut self.bulk {
            b.preview = preview;
            b.error = error;
            b.dirty = false;
        }
    }

    fn bulk_apply(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let spec = self.bulk_spec(cx);
        if self.bulk.as_ref().is_some_and(|b| b.error.is_some()) {
            return;
        }
        self.close_bulk(window, cx);
        self.run("rename_bulk", spec, window, cx);
    }
}

pub fn render(shell: &mut Shell, cx: &mut Context<Shell>) -> Option<AnyElement> {
    shell.bulk_refresh(cx);
    let b = shell.bulk.as_ref()?;
    let theme = cx.global::<Theme>().clone();
    let mut form = div().flex().flex_col().gap_2();
    for (i, (label, _)) in FIELDS.iter().enumerate() {
        let focused = i == b.focused;
        form = form.child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .w(px(100.))
                        .text_xs()
                        .text_color(theme.text_muted)
                        .child(*label),
                )
                .child(
                    div()
                        .flex_1()
                        .h(px(28.))
                        .px_2()
                        .flex()
                        .items_center()
                        .rounded_md()
                        .border_1()
                        .border_color(if focused { theme.accent } else { theme.border })
                        .bg(theme.bg)
                        .child(b.inputs[i].clone()),
                ),
        );
    }
    form = form.child(
        div()
            .flex()
            .items_center()
            .gap_2()
            .child(
                div()
                    .w(px(100.))
                    .text_xs()
                    .text_color(theme.text_muted)
                    .child("Case"),
            )
            .child(
                div()
                    .id("bulk-case")
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .border_1()
                    .border_color(theme.border)
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| this.bulk_cycle_case(cx)))
                    .child(format!("{}  (ctrl-k cycles)", CASES[b.case])),
            ),
    );
    let mut list = div()
        .flex()
        .flex_col()
        .max_h(px(260.))
        .overflow_hidden()
        .text_xs()
        .font_family(shell.mono_font.clone());
    for (from, to) in b.preview.iter().take(40) {
        list = list.child(
            div()
                .flex()
                .gap_2()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_color(theme.text_muted)
                        .child(from.clone()),
                )
                .child(div().flex_none().child("→"))
                .child(div().flex_1().min_w_0().truncate().child(to.clone())),
        );
    }
    if b.preview.len() > 40 {
        list = list.child(
            div()
                .text_color(theme.text_muted)
                .child(format!("… {} more", b.preview.len() - 40)),
        );
    }
    Some(
        deferred(
            div()
                .absolute()
                .inset_0()
                .flex()
                .justify_center()
                .items_start()
                .pt(px(60.))
                .occlude()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, window, cx| this.close_bulk(window, cx)),
                )
                .child(
                    div()
                        .w(px(720.))
                        .p_3()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .rounded_lg()
                        .border_1()
                        .border_color(theme.border)
                        .bg(theme.surface)
                        .shadow_lg()
                        .key_context(keymap::BULK)
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .child(div().child(format!("Rename {} items", shell.selection_ctx().count)))
                        .child(form)
                        .children(
                            b.error
                                .clone()
                                .map(|e| div().text_xs().text_color(theme.error).child(e)),
                        )
                        .child(list)
                        .child(div().text_xs().text_color(theme.text_muted).child(
                            "Tab switches fields · Enter renames (undoable) · Escape cancels",
                        )),
                ),
        )
        .with_priority(4)
        .into_any_element(),
    )
}
