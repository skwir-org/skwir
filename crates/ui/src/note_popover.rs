//! A note opened beside the text it annotates.
//!
//! A reference to a note (`Run::note`: marked as one by the book, or raised
//! like a note's number; or a link whose note links back to its name, as a
//! book that sets its references as plain `[1]` writes them) opens the note
//! in a popover under the reference, where it is read without losing one's
//! place; "Go to note" still goes there.
//! A note on the page shown is its blocks (`Block::note`, every block of the
//! note the reference names); one in another chapter, as endnotes usually
//! are, is read from that chapter on a worker, through the provider that read
//! the page, and a reference that turns out to lead to no note goes where it
//! says. The popover is a popup like the toolbar's (`toolbar::popup_panel`):
//! it takes the keyboard and gives it back, Escape or a click outside closes
//! it, and so do scrolling the text and replacing it.
use crate::{
    inspector_text, latest::Latest, markdown::MarkdownStyle, preview::PreviewContent, shell::Shell,
    theme::Theme, toolbar,
};
use gpui::{AnyElement, Context, Pixels, Point, StyledText, Window, div, prelude::*, px};
use skwir_core::{
    inspection::{Block, InspectRequest, InspectionKind},
    scheduler::Priority,
};
use std::sync::Arc;

/// Blocks a note shows at most: a note is a few paragraphs, and one that is
/// not goes on in the text.
const NOTE_BLOCKS: usize = 64;

pub struct Open {
    popup: toolbar::Popup,
    /// Under the reference, in window coordinates, where it was drawn.
    at: Point<Pixels>,
    blocks: Arc<[Block]>,
    /// Where the reference leads, for "Go to note".
    target: String,
}

struct Reply {
    target: String,
    at: Point<Pixels>,
    blocks: Option<Vec<Block>>,
}

#[derive(Default)]
pub struct Notes {
    pub(crate) open: Option<Open>,
    /// A note read from another chapter.
    asked: Latest<Reply>,
}
impl Notes {
    pub(crate) fn holds_focus(&self, window: &Window, cx: &gpui::App) -> bool {
        self.open
            .as_ref()
            .is_some_and(|open| open.popup.focus.contains_focused(window, cx))
    }
    /// Close the note and forget one on its way, giving the keyboard back.
    pub(crate) fn close(&mut self, window: &mut Window, cx: &mut gpui::App) {
        self.asked.forget();
        if let Some(open) = self.open.take() {
            open.popup.restore(window, cx);
        }
    }
}

/// What makes a link a reference to a note.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Reference {
    /// The book marks it as one, or sets it raised (`Run::note`).
    Marked,
    /// A link with a name of its own (`Run::name`), which is a reference
    /// when what it leads to links back to that name, as the notes of most
    /// books without the marks do.
    Named(String),
}
impl Reference {
    /// What a click on a link says of it; `None` for a link that cannot be
    /// a reference.
    pub(crate) fn of(args: &serde_json::Value) -> Option<Self> {
        if args["note"].as_bool() == Some(true) {
            return Some(Self::Marked);
        }
        args["name"]
            .as_str()
            .map(|name| Self::Named(name.to_owned()))
    }
}

/// The blocks of the note `fragment` names in `blocks`: every block of that
/// note, or the named block alone when it is part of none (a book that sets
/// its notes as numbered paragraphs). Pictures stay in the text. A link
/// known only by its name leads to a note when the note links back to it,
/// and never to a heading: a chapter's title linking back to the contents
/// is not a note.
pub(crate) fn note_blocks(
    blocks: &[Block],
    fragment: &str,
    reference: &Reference,
) -> Option<Vec<Block>> {
    let index = blocks
        .iter()
        .position(|block| block.anchors.iter().any(|name| name == fragment))?;
    let text = |block: &Block| Block {
        picture: None,
        ..block.clone()
    };
    let note: Vec<Block> = match blocks[index].note {
        Some(note) => blocks
            .iter()
            .filter(|block| block.note == Some(note))
            .take(NOTE_BLOCKS)
            .map(text)
            .collect(),
        None => vec![text(&blocks[index])],
    };
    if let Reference::Named(name) = reference {
        let back = |link: &str| {
            link.rsplit_once('#')
                .is_some_and(|(_, to)| decoded(to) == *name)
        };
        let leads_back = note
            .iter()
            .flat_map(|block| &block.runs)
            .filter_map(|run| run.link.as_deref())
            .any(back);
        if blocks[index].heading > 0 || !leads_back {
            return None;
        }
    }
    Some(note)
}

/// A link's fragment as the page names it, its escapes undone.
fn decoded(fragment: &str) -> String {
    String::from_utf8_lossy(&skwir_platform::uri::percent_decode(fragment)).into_owned()
}

impl Shell {
    /// A reference to a note was clicked: open the note under it. Returns
    /// whether it is opening; a link that names no note is not, and goes
    /// where it says.
    pub(crate) fn open_note(
        &mut self,
        target: &str,
        reference: Reference,
        at: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(PreviewContent::Inspection(display)) = &self.preview.content else {
            return false;
        };
        let Some((chapter, fragment)) = target.split_once('#') else {
            return false;
        };
        if fragment.is_empty() {
            return false;
        }
        let fragment = decoded(fragment);
        if chapter.is_empty() {
            let Some(blocks) = note_blocks(&display.data.blocks, &fragment, &reference) else {
                return false;
            };
            self.show_note(target, at, blocks, window, cx);
            return true;
        }
        // A note in another chapter, as endnotes are: read it there.
        if display.data.kind != InspectionKind::Book {
            return false;
        }
        let Some((request, view)) = display.origin.as_deref().cloned() else {
            return false;
        };
        self.preview.inspection.notes.close(window, cx);
        let previews = Arc::clone(&self.app.previews);
        let target = target.to_owned();
        self.preview.inspection.notes.asked.ask(
            &self.app,
            self.app.lifetime(),
            Priority::Visible,
            "preview-note",
            move |cancel| {
                let chapter = InspectRequest {
                    provider: view.provider,
                    target: Some(target.clone()),
                    ..InspectRequest::default()
                };
                let blocks = previews
                    .inspect(&request, &chapter, cancel)
                    .ok()
                    .flatten()
                    .and_then(|data| note_blocks(&data.blocks, &fragment, &reference));
                Reply { target, at, blocks }
            },
        );
        true
    }

    /// Take a note read from another chapter, as the core wakes the UI.
    pub(crate) fn sync_notes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(reply) = self.preview.inspection.notes.asked.take() {
            match reply.blocks {
                Some(blocks) => self.show_note(&reply.target, reply.at, blocks, window, cx),
                // Not a note after all: the link goes where it says.
                None => self.run(
                    "ui.inspect",
                    serde_json::json!({"action":"link","target":reply.target}),
                    window,
                    cx,
                ),
            }
        }
    }

    fn show_note(
        &mut self,
        target: &str,
        at: Point<Pixels>,
        blocks: Vec<Block>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.preview.inspection.notes.close(window, cx);
        let popup = toolbar::Popup::new("note", window, cx);
        self.preview.inspection.notes.open = Some(Open {
            popup,
            at,
            blocks: blocks.into(),
            target: target.to_owned(),
        });
        cx.notify();
    }

    /// `ui.note`: close the note (`close`), or go to it in the text (`go`).
    pub(crate) fn note_command(
        &mut self,
        args: &serde_json::Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let target = self
            .preview
            .inspection
            .notes
            .open
            .as_ref()
            .map(|open| open.target.clone());
        self.preview.inspection.notes.close(window, cx);
        if args["go"].as_bool() == Some(true)
            && let Some(target) = target
        {
            self.run(
                "ui.inspect",
                serde_json::json!({"action":"link","target":target}),
                window,
                cx,
            );
        }
        cx.notify();
    }
}

/// The open note, over the text and under its reference. A link inside it
/// goes where it says, like one in the text.
pub(crate) fn popover(
    shell: &Shell,
    style: &MarkdownStyle,
    theme: &Theme,
    cx: &mut Context<Shell>,
) -> Option<AnyElement> {
    let open = shell.preview.inspection.notes.open.as_ref()?;
    let size = (style.body_size - 1.).max(10.);
    let mut text = div()
        .id("note-text")
        .max_h(px(size * style.line_height * 12.))
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .gap_2()
        .text_size(px(size))
        .line_height(px(size * style.line_height));
    for (index, block) in open.blocks.iter().enumerate() {
        let (shown, highlights) = inspector_text::styled(
            block,
            0..block.text.len(),
            0..0,
            style,
            inspector_text::Marks::default(),
        );
        let links: Vec<_> = block
            .runs
            .iter()
            .filter_map(|run| Some((run.range.clone(), run.link.clone()?)))
            .collect();
        let ranges = links.iter().map(|(range, _)| range.clone()).collect();
        let entity = cx.entity().downgrade();
        let pane = shell.pane;
        text = text.child(
            div()
                .pl(px(
                    style.list_indent * block.list_depth.saturating_sub(1) as f32
                ))
                .child(
                    gpui::InteractiveText::new(
                        ("note-block", index),
                        StyledText::new(shown).with_highlights(highlights),
                    )
                    .on_click(ranges, move |i, window, cx| {
                        let target = links[i].1.clone();
                        let _ = entity.update(cx, |shell, cx| {
                            shell.with_pane(pane, |shell| {
                                shell.preview.inspection.notes.close(window, cx);
                                shell.run(
                                    "ui.inspect",
                                    serde_json::json!({"action":"link","target":target}),
                                    window,
                                    cx,
                                );
                            });
                        });
                    }),
                ),
        );
    }
    let panel = toolbar::popup_panel(&open.popup, "ui.note", theme, cx)
        .w(px(size * 26.))
        .font_family(shell.font.clone())
        .child(text)
        .child(
            div().flex().justify_end().child(
                toolbar::Button::new("note-go", "forward", "Go to note", "ui.note")
                    .args(serde_json::json!({"go":true}))
                    .label("Go to note")
                    .render(shell, cx),
            ),
        );
    Some(
        gpui::deferred(
            gpui::anchored()
                .position(open.at)
                .offset(gpui::point(px(0.), px(4.)))
                .snap_to_window_with_margin(px(8.))
                .child(panel),
        )
        .with_priority(3)
        .into_any_element(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(text: &str, anchors: &[&str], note: Option<u32>) -> Block {
        Block {
            text: text.into(),
            anchors: anchors.iter().map(|&a| a.to_owned()).collect(),
            note,
            ..Block::default()
        }
    }

    #[test]
    fn a_reference_opens_its_whole_note() {
        let blocks = [
            block("Text", &["r1"], None),
            block("First", &["n1"], Some(0)),
            block("Second", &[], Some(0)),
            block("Other", &["n2"], Some(1)),
            block("A plain paragraph", &["p9"], None),
        ];
        let texts = |fragment| {
            note_blocks(&blocks, fragment, &Reference::Marked)
                .map(|found| found.into_iter().map(|b| b.text).collect::<Vec<_>>())
        };
        assert_eq!(texts("n1").unwrap(), ["First", "Second"]);
        assert_eq!(texts("n2").unwrap(), ["Other"]);
        assert_eq!(texts("p9").unwrap(), ["A plain paragraph"]);
        assert!(texts("missing").is_none());
    }

    fn linking(mut block: Block, link: &str) -> Block {
        block.runs.push(skwir_core::inspection::Run {
            range: 0..1,
            link: Some(link.into()),
            ..Default::default()
        });
        block
    }

    /// A link set as plain text opens what it leads to as a note only when
    /// that links back to the link's own name: a note in a book without the
    /// marks, never a paragraph that does not, nor a chapter's title.
    #[test]
    fn a_named_link_opens_a_note_that_links_back() {
        let mut blocks = [
            linking(
                block("1 A note.", &["ftn1"], None),
                "Text/../Text/one.xhtml#bodyftn1",
            ),
            linking(block("2 Escaped.", &["ftn2"], None), "#caf%C3%A9"),
            block("A paragraph", &["p9"], None),
            linking(block("Chapter", &["c1"], None), "contents.xhtml#bodyftn1"),
            linking(block("Elsewhere", &["x1"], None), "two.xhtml#someone-else"),
        ];
        blocks[3].heading = 1;
        let named = |fragment, name: &str| {
            note_blocks(&blocks, fragment, &Reference::Named(name.into()))
                .map(|found| found.into_iter().map(|b| b.text).collect::<Vec<_>>())
        };
        assert_eq!(named("ftn1", "bodyftn1").unwrap(), ["1 A note."]);
        assert_eq!(named("ftn2", "café").unwrap(), ["2 Escaped."]);
        assert!(named("p9", "bodyftn1").is_none(), "no way back");
        assert!(named("c1", "bodyftn1").is_none(), "a heading");
        assert!(named("x1", "bodyftn1").is_none(), "back to another link");
        assert_eq!(
            Reference::of(&serde_json::json!({"note": false, "name": "r1"})),
            Some(Reference::Named("r1".into()))
        );
        assert_eq!(
            Reference::of(&serde_json::json!({"note": true, "name": "r1"})),
            Some(Reference::Marked)
        );
        assert_eq!(Reference::of(&serde_json::json!({"note": false})), None);
    }
}
