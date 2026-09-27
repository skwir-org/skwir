//! A streaming walk over an XML document, for formats whose files outgrow
//! a tree in memory: a day's GPX is tens of megabytes. Elements are named by
//! their local name (`gx:Track` is `Track`); each end hands over the text
//! the element held since its last child. Nothing a document declares is
//! expanded: a DTD's entities are neither read nor fetched.
use std::io::BufRead;

use bibli_core::CancelToken;
use quick_xml::XmlVersion;
use quick_xml::events::Event;

/// Text an element may hold (a KML line's coordinates are one text).
pub const MAX_TEXT: usize = 64 * 1024 * 1024;

pub enum Tag<'a> {
    Start {
        name: &'a str,
        attributes: &'a [(String, String)],
    },
    End {
        name: &'a str,
        text: &'a str,
    },
}

/// Why a walk stopped before the end of the document. What was read before
/// is kept by the caller: a GPS logger that lost power leaves a file cut
/// short, and its track up to there is still worth showing.
pub struct Damaged(pub String);

fn local(name: &str) -> String {
    name.rsplit(':').next().unwrap_or(name).to_owned()
}

/// Walk `input`, calling `on` for every start and end tag. `Err` is the
/// caller's own error or a cancellation; `Ok(Some(_))` says the document
/// was damaged where the walk stopped.
pub fn walk<R: BufRead>(
    input: R,
    cancel: &CancelToken,
    mut on: impl FnMut(Tag<'_>) -> anyhow::Result<()>,
) -> anyhow::Result<Option<Damaged>> {
    // Text is not trimmed here: an entity splits it into several events,
    // and trimming each would eat the spaces around `&amp;`. Readers trim
    // what they use.
    let mut reader = quick_xml::Reader::from_reader(input);
    let mut buf = Vec::new();
    let mut attributes: Vec<(String, String)> = Vec::new();
    let mut names: Vec<String> = Vec::new();
    let mut text = String::new();
    let mut events = 0u32;
    loop {
        events = events.wrapping_add(1);
        if events.is_multiple_of(4096) {
            bibli_platform::inspection::check(cancel)?;
        }
        buf.clear();
        let event = match reader.read_event_into(&mut buf) {
            Ok(event) => event,
            Err(error) => {
                return Ok(Some(Damaged(format!(
                    "{error} (at byte {})",
                    reader.error_position()
                ))));
            }
        };
        match event {
            Event::Start(ref start) | Event::Empty(ref start) => {
                let empty = matches!(event, Event::Empty(_));
                let name = local(start.name().as_ref());
                attributes.clear();
                for attribute in start.attributes().flatten() {
                    let value = attribute
                        .normalized_value(XmlVersion::Implicit1_0)
                        .map(std::borrow::Cow::into_owned)
                        .unwrap_or_default();
                    attributes.push((local(attribute.key.as_ref()), value));
                }
                text.clear();
                on(Tag::Start {
                    name: &name,
                    attributes: &attributes,
                })?;
                if empty {
                    on(Tag::End {
                        name: &name,
                        text: "",
                    })?;
                } else {
                    names.push(name);
                }
            }
            Event::End(_) => {
                let Some(name) = names.pop() else {
                    return Ok(Some(Damaged("an end tag closes nothing".into())));
                };
                on(Tag::End {
                    name: &name,
                    text: &text,
                })?;
                text.clear();
            }
            Event::Text(content) => push(&mut text, &content.xml10_content())?,
            Event::CData(content) => push(&mut text, &content.xml10_content())?,
            Event::GeneralRef(reference) => {
                let resolved = if reference.is_char_ref() {
                    reference.resolve_char_ref().ok().flatten()
                } else {
                    match &*reference.xml10_content() {
                        "amp" => Some('&'),
                        "lt" => Some('<'),
                        "gt" => Some('>'),
                        "quot" => Some('"'),
                        "apos" => Some('\''),
                        // A document's own entities are not expanded.
                        _ => None,
                    }
                };
                if let Some(c) = resolved {
                    push(&mut text, c.encode_utf8(&mut [0; 4]))?;
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(None)
}

fn push(text: &mut String, more: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        text.len() + more.len() <= MAX_TEXT,
        "an element holds more than {} of text",
        bibli_platform::inspection::limits::describe(MAX_TEXT)
    );
    text.push_str(more);
    Ok(())
}

/// The local name of the root element of the XML document `head` begins,
/// and its attributes (namespace declarations among them), to recognise a
/// format from a file's first bytes. `None` when they begin no XML.
pub fn root(head: &[u8]) -> Option<(String, Vec<(String, String)>)> {
    let local = |name: &str| name.rsplit(':').next().unwrap_or(name).to_owned();
    let mut reader = quick_xml::Reader::from_reader(head);
    let mut buf = Vec::new();
    loop {
        buf.clear();
        match reader.read_event_into(&mut buf).ok()? {
            Event::Start(start) | Event::Empty(start) => {
                let attributes = start
                    .attributes()
                    .flatten()
                    .map(|attribute| {
                        let value = attribute
                            .normalized_value(XmlVersion::Implicit1_0)
                            .map(std::borrow::Cow::into_owned)
                            .unwrap_or_default();
                        (local(attribute.key.as_ref()), value)
                    })
                    .collect();
                return Some((local(start.name().as_ref()), attributes));
            }
            Event::Text(text) if !text.xml10_content().trim().is_empty() => return None,
            Event::Eof => return None,
            _ => {}
        }
    }
}
