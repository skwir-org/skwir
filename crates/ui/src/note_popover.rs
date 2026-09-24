//! A note opened beside the text it annotates.
//!
//! A reference to a note (`Run::note`: marked as one by the book, or raised
//! like a note's number) opens the note in a popover under the reference,
//! where it is read without losing one's place; "Go to note" still goes there.
//! A note on the page shown is its blocks (`Block::note`, every block of the
//! note the reference names); one in another chapter, as endnotes usually
//! are, is read from that chapter on a worker, through the provider that read
//! the page, and a reference that turns out to lead to no note goes where it
//! says. The popover is a popup like the toolbar's (`toolbar::popup_panel`):
//! it takes the keyboard and gives it back, Escape or a click outside closes
//! it, and so do scrolling the text and replacing it.
use crate::{
    inspector_text, markdown::MarkdownStyle, preview::PreviewContent, shell::Shell, theme::Theme,
    toolbar,
};
use bibli_core::{
    CancelToken,
    inspection::{Block, InspectRequest, InspectionKind},
    scheduler::Priority,
};
use gpui::{AnyElement, Context, Pixels, Point, StyledText, Window, div, prelude::*, px};
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
    generation: u64,
    target: String,
    at: Point<Pixels>,
    blocks: Option<Vec<Block>>,
}

pub struct Notes {
    pub(crate) open: Option<Open>,
    pending: Option<CancelToken>,
    generation: u64,
    tx: flume::Sender<Reply>,
    rx: flume::Receiver<Reply>,
}
impl Default for Notes {
    fn default() -> Self {
        let (tx, rx) = flume::unbounded();
        Self {
            open: None,
            pending: None,
            generation: 0,
            tx,
            rx,
        }
    }
}
impl Drop for Notes {
    fn drop(&mut self) {
        if let Some(pending) = self.pending.take() {
            pending.cancel();
        }
    }
}
impl Notes {
    pub(crate) fn holds_focus(&self, window: &Window, cx: &gpui::App) -> bool {
        self.open
            .as_ref()
            .is_some_and(|open| open.popup.focus.contains_focused(window, cx))
    }
    /// Close the note and forget one on its way, giving the keyboard back.
    pub(crate) fn close(&mut self, window: &mut Window) {
        if let Some(pending) = self.pending.take() {
            pending.cancel();
        }
        self.generation = self.generation.wrapping_add(1);
        if let Some(open) = self.open.take() {
            open.popup.restore(window);
        }
    }
}

/// The blocks of the note `fragment` names in `blocks`: every block of that
/// note, or the named block alone when it is part of none (a book that sets
/// its notes as numbered paragraphs). Pictures stay in the text.
pub(crate) fn note_blocks(blocks: &[Block], fragment: &str) -> Option<Vec<Block>> {
    let index = blocks
        .iter()
        .position(|block| block.anchors.iter().any(|name| name == fragment))?;
    let text = |block: &Block| Block {
        picture: None,
        ..block.clone()
    };
    Some(match blocks[index].note {
        Some(note) => blocks
            .iter()
            .filter(|block| block.note == Some(note))
            .take(NOTE_BLOCKS)
            .map(text)
            .collect(),
        None => vec![text(&blocks[index])],
    })
}

impl Shell {
    /// A reference to a note was clicked: open the note under it. Returns
    /// whether it is opening; a link that names no note is not, and goes
    /// where it says.
    pub(crate) fn open_note(
        &mut self,
        target: &str,
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
        if chapter.is_empty() {
            let Some(blocks) = note_blocks(&display.data.blocks, fragment) else {
                return false;
            };
            self.show_note(target, at, blocks, window, cx);
            return true;
        }
        // A note in another chapter, as endnotes are: read it there.
        if display.data.kind != InspectionKind::Book {
            return false;
        }
        let Some((request, view)) = display.origin.clone() else {
            return false;
        };
        self.preview.inspection.notes.close(window);
        let notes = &mut self.preview.inspection.notes;
        let cancel = self.app.lifetime().child();
        notes.pending = Some(cancel.clone());
        let generation = notes.generation;
        let tx = notes.tx.clone();
        let previews = Arc::clone(&self.app.previews);
        let wake = self.app.waker_handle();
        let (target, fragment) = (target.to_owned(), fragment.to_owned());
        self.app.scheduler.spawn(
            Priority::Visible,
            "preview-note",
            cancel.clone(),
            move || {
                let chapter = InspectRequest {
                    provider: view.provider,
                    target: Some(target.clone()),
                    ..InspectRequest::default()
                };
                let blocks = previews
                    .inspect(&request, &chapter, &cancel)
                    .ok()
                    .flatten()
                    .and_then(|data| note_blocks(&data.blocks, &fragment));
                if !cancel.is_cancelled() {
                    let _ = tx.send(Reply {
                        generation,
                        target,
                        at,
                        blocks,
                    });
                    wake();
                }
            },
        );
        true
    }

    /// Take a note read from another chapter, as the core wakes the UI.
    pub(crate) fn sync_notes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        while let Ok(reply) = self.preview.inspection.notes.rx.try_recv() {
            let notes = &mut self.preview.inspection.notes;
            if reply.generation != notes.generation {
                continue;
            }
            notes.pending = None;
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
        self.preview.inspection.notes.close(window);
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
        self.preview.inspection.notes.close(window);
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
        let (shown, highlights) = inspector_text::styled(block, 0..block.text.len(), 0..0, style);
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
                                shell.preview.inspection.notes.close(window);
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
            note_blocks(&blocks, fragment)
                .map(|found| found.into_iter().map(|b| b.text).collect::<Vec<_>>())
        };
        assert_eq!(texts("n1").unwrap(), ["First", "Second"]);
        assert_eq!(texts("n2").unwrap(), ["Other"]);
        assert_eq!(texts("p9").unwrap(), ["A plain paragraph"]);
        assert!(texts("missing").is_none());
    }
}
