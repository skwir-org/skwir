//! Keys that wait for the next (ADR-024). A binding of several strokes
//! (`g h`, `, m`) waits after its first stroke for as long as it takes
//! (`App::set_key_sequence_timeout`, vendored patch 0010), and a strip at
//! the foot of the window lists what can follow: the hints that apply
//! where the focus is and begin with what was typed.

use gpui::{AnyElement, Context, Keystroke, Window, div, prelude::*, px};

use crate::controls;
use crate::keymap::{self, KeyHint};
use crate::shell::Shell;
use crate::theme::Theme;

/// What was typed and what may follow: the rest of each binding's keys
/// and what it does.
#[derive(Default)]
pub struct Sequence {
    typed: Vec<Keystroke>,
    next: Vec<(String, String)>,
}

/// Whether a stroke typed matches a stroke bound, as GPUI matches them:
/// by the character typed (`?`, `G`) with Ctrl and Super as they were,
/// or by the key and every modifier.
fn matches(typed: &Keystroke, bound: &Keystroke) -> bool {
    if let Some(ch) = typed.key_char.as_ref().filter(|ch| **ch != typed.key) {
        let mods = gpui::Modifiers {
            control: typed.modifiers.control,
            platform: typed.modifiers.platform,
            ..Default::default()
        };
        if bound.key == *ch && bound.modifiers == mods {
            return true;
        }
    }
    bound.modifiers == typed.modifiers && bound.key == typed.key
}

/// The rest of `hint`'s keys after `typed`, if they begin with it.
fn rest(hint: &KeyHint, typed: &[Keystroke]) -> Option<String> {
    let strokes: Vec<&str> = hint.keys.split_whitespace().collect();
    if strokes.len() <= typed.len() {
        return None;
    }
    for (stroke, typed) in strokes.iter().zip(typed) {
        let bound = Keystroke::parse(stroke).ok()?;
        if !matches(typed, &bound) {
            return None;
        }
    }
    Some(strokes[typed.len()..].join(" "))
}

impl Shell {
    /// The window's pending keys changed: a sequence began, went on, or
    /// ended (completed, or replayed by a key that continues nothing).
    pub(crate) fn on_pending_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(typed) = window.pending_input_keystrokes().map(<[Keystroke]>::to_vec) else {
            if self.sequence.take().is_some() {
                cx.notify();
            }
            return;
        };
        let stack = window.context_stack();
        let holds = |token: &str| stack.iter().any(|context| context.contains(token));
        let mut next: Vec<(String, String)> = Vec::new();
        for hint in &self.key_hints {
            if !crate::shortcuts::applies(&hint.context, &holds) {
                continue;
            }
            if let Some(rest) = rest(hint, &typed)
                && !next.iter().any(|(keys, _)| *keys == rest)
            {
                next.push((rest, keymap::title_of(&self.app, hint)));
            }
        }
        self.sequence = Some(Sequence { typed, next });
        cx.notify();
    }
}

pub fn render(shell: &Shell, cx: &mut Context<Shell>) -> Option<AnyElement> {
    let sequence = shell.sequence.as_ref()?;
    let theme = cx.global::<Theme>().clone();
    let caps = |keys: &str, theme: &Theme| {
        let mut row = div().flex().flex_none().items_center().gap(px(6.));
        for stroke in keymap::key_caps(keys) {
            let mut caps = div().flex().items_center().gap(px(3.));
            for cap in stroke {
                caps = caps.child(controls::keycap(cap, theme));
            }
            row = row.child(caps);
        }
        row
    };
    let typed: Vec<String> = sequence.typed.iter().map(Keystroke::unparse).collect();
    let mut list = div().flex().flex_wrap().gap_x_6().gap_y_1();
    for (i, (keys, title)) in sequence.next.iter().enumerate() {
        list = list.child(
            div()
                .w(px(240.))
                .flex()
                .items_center()
                .gap_2()
                .child(caps(keys, &theme))
                .child(crate::elements::clip::clipped(
                    div()
                        .id(("next-key", i))
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_color(theme.text_muted),
                    title.clone(),
                )),
        );
    }
    let card = div()
        .max_w(px(820.))
        .p_3()
        .flex()
        .flex_col()
        .gap_2()
        .rounded_lg()
        .border_1()
        .border_color(theme.border)
        .bg(theme.bg)
        .shadow_lg()
        .text_xs()
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(caps(&typed.join(" "), &theme))
                .child(if sequence.next.is_empty() {
                    "Nothing follows these keys here"
                } else {
                    "then"
                }),
        )
        .child(list);
    Some(
        div()
            .absolute()
            .bottom(px(38.))
            .left_0()
            .right_0()
            .flex()
            .justify_center()
            .child(card)
            .into_any_element(),
    )
}

#[cfg(test)]
mod tests {
    use super::rest;
    use crate::keymap::{KeyHint, Layer};
    use gpui::Keystroke;

    fn hint(keys: &str) -> KeyHint {
        KeyHint {
            keys: keys.into(),
            command: "x".into(),
            args: serde_json::json!({}),
            context: "Pane".into(),
            layer: Layer::Scheme,
        }
    }

    fn typed(key: &str, shift: bool, ch: &str) -> Keystroke {
        Keystroke {
            modifiers: gpui::Modifiers {
                shift,
                ..Default::default()
            },
            key: key.into(),
            key_char: Some(ch.into()),
        }
    }

    #[test]
    fn what_follows_is_the_rest_of_each_binding_that_begins_with_the_keys_typed() {
        let g = [typed("g", false, "g")];
        assert_eq!(rest(&hint("g h"), &g).as_deref(), Some("h"));
        assert_eq!(
            rest(&hint("g"), &g),
            None,
            "nothing follows a whole binding"
        );
        assert_eq!(rest(&hint("c c"), &g), None);
        // A comma, and a symbol typed with Shift (`?` is Shift+/ on a US
        // keyboard): matched by the character, as GPUI does.
        let comma = [typed(",", false, ",")];
        assert_eq!(rest(&hint(", M"), &comma).as_deref(), Some("M"));
        let question = [typed("/", true, "?")];
        assert_eq!(rest(&hint("? x"), &question).as_deref(), Some("x"));
    }
}
