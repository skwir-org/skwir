//! Bulk rename dialog: template, regex, replacement, case, start; live
//! preview from `rename_bulk.preview`; Enter applies `rename_bulk`.

use gpui::{AnyElement, Context, Entity, Subscription, Window, div, prelude::*, px};
use skwir_core::command::{Invocation, Outcome};

use crate::elements::clip::clipped;
use crate::input::{InputEvent, TextInput};
use crate::keymap;
use crate::modal::{self, FocusReturn};
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

/// The renames the preview shows at most.
const SHOWN: usize = 40;

pub struct BulkState {
    pub inputs: Vec<Entity<TextInput>>,
    pub case: usize,
    pub focused: usize,
    /// The first names the preview shows (`SHOWN`), and how many renames
    /// there are in all.
    pub preview: Vec<(String, String)>,
    renamed: usize,
    pub error: Option<String>,
    /// Advances with each change; only the latest preview is shown, and
    /// the one asked before is cancelled (`pending`): not worked out if
    /// it has not begun, stopped where it is if it has.
    asked: u64,
    pending: Option<skwir_core::CancelToken>,
    /// The preview shown is the answer to the latest change.
    current: bool,
    count: usize,
    back: FocusReturn,
    /// Where the renames listed are scrolled to.
    scroll: gpui::ScrollHandle,
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
        let count = self.selection_ctx().count;
        if count == 0 {
            self.set_status(crate::shell::NOTHING_SELECTED, false, cx);
            return;
        }
        let back = FocusReturn::capture(window, cx);
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
                        InputEvent::Changed => this.bulk_refresh(window, cx),
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
            renamed: 0,
            error: None,
            asked: 0,
            pending: None,
            current: false,
            count,
            back,
            scroll: gpui::ScrollHandle::new(),
            _subs: subs,
        });
        self.bulk_refresh(window, cx);
        cx.notify();
    }

    pub fn close_bulk(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(bulk) = self.bulk.take() {
            if let Some(pending) = &bulk.pending {
                pending.cancel();
            }
            bulk.back.restore(window, &self.ui().focus);
        }
        cx.notify();
    }

    pub fn bulk_focus_next(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(b) = &mut self.bulk {
            let n = b.inputs.len() as isize;
            // From the field that has the keyboard, which a click may have moved.
            let current = b
                .inputs
                .iter()
                .position(|i| i.read(cx).is_focused(window))
                .unwrap_or(b.focused);
            b.focused = ((current as isize + delta).rem_euclid(n)) as usize;
            b.inputs[b.focused].update(cx, |i, cx| {
                i.select_all_now(cx);
                i.focus(window);
            });
            cx.notify();
        }
    }

    pub fn bulk_cycle_case(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(b) = &mut self.bulk {
            b.case = (b.case + 1) % CASES.len();
            self.bulk_refresh(window, cx);
        }
    }

    /// Ask for the preview again after a change, on a worker: the plugin
    /// reads every selected entry and may stat the targets. The one asked
    /// before is cancelled, and says the first `SHOWN` renames alone, with
    /// how many there are.
    fn bulk_refresh(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut spec = self.bulk_spec(cx);
        spec["limit"] = SHOWN.into();
        let cancel = self.app.lifetime().child();
        let Some(b) = &mut self.bulk else { return };
        b.asked += 1;
        b.current = false;
        let asked = b.asked;
        if let Some(before) = b.pending.replace(cancel.clone()) {
            before.cancel();
        }
        let app = std::sync::Arc::clone(&self.app);
        self.off_thread_until(
            "rename-bulk-preview",
            cancel,
            move || match app.dispatch(&Invocation::with_args("rename_bulk.preview", &spec)) {
                Ok(Outcome::Value(v)) => {
                    let shown = v["renames"]
                        .as_array()
                        .map_or(&[][..], Vec::as_slice)
                        .iter()
                        .filter_map(|x| {
                            Some((
                                x.get("from")?.as_str()?.to_owned(),
                                x.get("to")?.as_str()?.to_owned(),
                            ))
                        })
                        .collect();
                    let count = v["count"]
                        .as_u64()
                        .and_then(|n| usize::try_from(n).ok())
                        .unwrap_or_default();
                    (shown, count, None)
                }
                Ok(_) => (Vec::new(), 0, None),
                Err(e) => (Vec::new(), 0, Some(format!("{e:#}"))),
            },
            move |this, result, _, cx| {
                let (preview, renamed, error) = result;
                let Some(b) = this.bulk.as_mut().filter(|b| b.asked == asked) else {
                    return;
                };
                b.pending = None;
                b.preview = preview;
                b.renamed = renamed;
                b.error = error;
                b.current = true;
                cx.notify();
            },
            window,
            cx,
        );
        cx.notify();
    }

    fn bulk_apply(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let spec = self.bulk_spec(cx);
        // Only what the preview showed, and only if it showed no error.
        if self
            .bulk
            .as_ref()
            .is_some_and(|b| !b.current || b.error.is_some())
        {
            return;
        }
        self.close_bulk(window, cx);
        self.run("rename_bulk", spec, window, cx);
    }
}

pub fn render(shell: &Shell, window: &Window, cx: &mut Context<Shell>) -> Option<AnyElement> {
    let b = shell.bulk.as_ref()?;
    let theme = cx.global::<Theme>().clone();
    let mut form = div().flex().flex_col().gap_2();
    for (i, (label, _)) in FIELDS.iter().enumerate() {
        // Where the keyboard is, whether Tab or a click put it there.
        let focused = b.inputs[i].read(cx).is_focused(window);
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
                    .on_click(cx.listener(|this, _, window, cx| this.bulk_cycle_case(window, cx)))
                    .child(
                        match shell.keys_for("ui.rename_bulk_case", &serde_json::Value::Null) {
                            Some(keys) => format!("{}  ({keys} cycles)", CASES[b.case]),
                            None => CASES[b.case].to_owned(),
                        },
                    ),
            ),
    );
    // The renames scroll under a scrollbar; how many more there are is
    // said below them, never cut off with the rows the box cannot hold.
    let mut list = div()
        .id("rename-preview")
        .flex()
        .flex_col()
        .max_h(px(260.))
        .track_scroll(&b.scroll)
        .overflow_y_scroll()
        .text_xs()
        .font_family(shell.mono_font.clone());
    let font = shell.mono_font.clone();
    for (i, (from, to)) in b.preview.iter().enumerate() {
        list = list.child(
            div()
                .flex()
                .gap_2()
                .child(clipped(
                    div()
                        .id(("rename-from", i))
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_color(theme.text_muted),
                    from.clone(),
                    font.clone(),
                ))
                .child(div().flex_none().child("→"))
                .child(clipped(
                    div().id(("rename-to", i)).flex_1().min_w_0().truncate(),
                    to.clone(),
                    font.clone(),
                )),
        );
    }
    let list =
        div()
            .relative()
            .flex()
            .flex_col()
            .child(list)
            .child(crate::elements::scrollbar::vertical(
                "rename-preview-scrollbar",
                crate::elements::scrollbar::Scrolls::Offset(b.scroll.clone()),
            ));
    let more = (b.renamed > b.preview.len()).then(|| {
        div()
            .text_xs()
            .text_color(theme.text_muted)
            .child(format!("… {} more", b.renamed - b.preview.len()))
    });
    let panel = modal::panel(720., &theme)
        .p_3()
        .gap_2()
        .key_context(keymap::BULK)
        .child(div().child(format!("Rename {} items", b.count)))
        .child(form)
        .children(
            b.error
                .clone()
                .map(|e| div().text_xs().text_color(theme.error).child(e)),
        )
        .child(list)
        .children(more)
        .child(
            div()
                .text_xs()
                .text_color(theme.text_muted)
                .child("Tab switches fields · Enter renames (undoable) · Escape cancels"),
        );
    Some(modal::modal(panel, 60., 4, Shell::close_bulk, &theme, cx))
}
