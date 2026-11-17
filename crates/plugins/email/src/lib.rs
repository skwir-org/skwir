//! EML headers, inert HTML/text body and bounded attachment previews.
use skwir_core::{
    App, CancelToken,
    inspection::{Block, InspectRequest, Inspection, Row},
    plugin::Plugin,
    preview::{Held, PreviewProvider, PreviewRequest},
    words,
};
use skwir_platform::{
    archive,
    inspection::{self, limits, limits::Bounds},
    rich_preview,
};
use std::borrow::Cow;
use std::sync::Arc;
pub struct PreviewPlugin;
impl Plugin for PreviewPlugin {
    fn id(&self) -> &'static str {
        "email"
    }
    fn title(&self) -> skwir_core::words::Words {
        skwir_core::words!(email_plugin_title = "Email")
    }
    fn description(&self) -> skwir_core::words::Words {
        skwir_core::words!(
            email_plugin_description =
                "Read EML messages: headers, body and attachments, with remote content blocked"
        )
    }
    fn category(&self) -> skwir_core::plugin::Category {
        skwir_core::plugin::Category::Previews
    }
    fn capabilities(&self) -> skwir_core::permissions::Capabilities {
        use skwir_core::permissions::Capability;
        skwir_core::permissions::Capabilities::of(&[Capability::Read])
    }
    fn register(&self, app: &App) {
        app.previews.register(self.id(), Arc::new(Provider));
    }
}
/// A mail, by type (AGENTS.md, "Types"): `.eml` is the database's.
static MAIL: skwir_platform::mime::Types = skwir_platform::mime::Types::new(&["message/rfc822"]);

struct Provider;
impl PreviewProvider for Provider {
    fn id(&self) -> &'static str {
        "email"
    }
    fn accepts(&self, req: &PreviewRequest) -> bool {
        MAIL.holds(&req.entry.path, req.mime.as_deref())
    }
    fn extract(
        &self,
        req: &PreviewRequest,
        view: &InspectRequest,
        target: &str,
        into: &std::path::Path,
        cancel: &CancelToken,
    ) -> anyhow::Result<Option<skwir_core::preview::Member>> {
        // The message as it was shown, within whatever bounds it was (Full
        // preview's reads past the quick ones); the attachment written out
        // as it is to be looked at.
        let held = req.held()?;
        let bytes = read(&held, Bounds::WHOLE, cancel)?;
        let message = parse(&bytes, cancel)?;
        let attachment = message.attachments().nth(target.parse()?).ok_or_else(|| {
            anyhow::anyhow!(words!(
                email_missing_attachment = "Missing attachment",
                note = "An error: the message no longer holds the attachment asked for."
            ))
        })?;
        let bounds = Bounds::of(view);
        if attachment.decoded_len() > bounds.binary_input() {
            return Err(bounds.refuse(
                words!(
                    email_attachment_too_large = "The attachment exceeds the { $size } preview limit",
                    size = words::Size(bounds.binary_input() as u64),
                    note = "Why an attachment is not previewed. size: the largest the preview reads (16 MB)."
                ),
            ));
        }
        let name = attachment.attachment_name().unwrap_or("Attachment");
        let file = archive::member_file(into, name);
        std::fs::write(&file, attachment.decoded())?;
        Ok(Some(skwir_core::preview::Member {
            own_file: false,
            name: name.to_owned(),
            path: file,
        }))
    }
    /// The message. An attachment is never inspected here: the panel
    /// extracts it and previews it as the file it is.
    fn inspect(
        &self,
        req: &PreviewRequest,
        view: &InspectRequest,
        cancel: &CancelToken,
    ) -> anyhow::Result<Option<Inspection>> {
        // The source is the shared code view (the worker's fallback).
        if view.source {
            return Ok(None);
        }
        let bounds = Bounds::of(view);
        let held = req.held()?;
        let bytes = read(&held, bounds, cancel)?;
        let message = parse(&bytes, cancel)?;
        let sent = message
            .date()
            .map_or_else(String::new, |date| date.to_rfc3339());
        let mut blocks = vec![
            Block {
                text: message.subject().map_or_else(
                    || {
                        words!(
                            email_no_subject = "No subject",
                            note = "The heading of an email that has no subject."
                        )
                        .say()
                    },
                    str::to_owned,
                ),
                heading: 1,
                ..Block::default()
            },
            Block {
                text: words!(
                    email_header = "From: { $from }
                        To: { $to }
                        Date: { $sent }",
                    from = addresses(message.from()),
                    to = addresses(message.to()),
                    sent = sent.clone(),
                    note = "The lines under an email's subject, each on its own line. from, to: the addresses as the message writes them. sent: when it was sent, as the message dates it."
                )
                .say(),
                ..Block::default()
            },
        ];
        let mut remote = false;
        let mut truncated = false;
        if let Some(html) = message.body_html(0) {
            let images = rich_preview::pictures(|src: &str| {
                let Some(id) = src.strip_prefix("cid:") else {
                    remote |= ["http:", "https:", "//"].iter().any(|p| src.starts_with(p));
                    return None;
                };
                let part = message.parts().find(|p| p.content_id() == Some(id))?;
                inspection::image(&part.decoded()).ok()
            });
            let mut page =
                rich_preview::html(&html, rich_preview::Markup::Html, bounds, cancel, images)?;
            truncated = page.truncated;
            // The parser writes a message in plain text as HTML: the addresses
            // its text gives are links, as mail readers make them.
            let written = message
                .html_body()
                .next()
                .is_some_and(|part| matches!(part.kind(), mail_parser::PartKind::Html));
            if !written {
                for block in page.blocks.iter_mut().filter(|block| !block.code) {
                    skwir_platform::links::link_block(block, false);
                }
            }
            blocks.extend(page.blocks);
        } else if let Some(text) = message.body_text(0) {
            let mut body = Block {
                text: text.into_owned(),
                ..Block::default()
            };
            skwir_platform::links::link_block(&mut body, false);
            blocks.push(body);
        }
        let attachments = message.attachments().len();
        let details = inspection::details([
            (
                words!(
                    email_detail_subject = "Subject",
                    note = "A label in the preview's Properties: the email's subject."
                ),
                message.subject().map(str::to_owned),
            ),
            (
                words!(
                    email_detail_from = "From",
                    note = "A label in the preview's Properties: who sent the email."
                ),
                Some(addresses(message.from())),
            ),
            (
                words!(
                    email_detail_to = "To",
                    note = "A label in the preview's Properties: who the email was sent to."
                ),
                Some(addresses(message.to())),
            ),
            (
                words!(
                    email_detail_cc = "Cc",
                    note = "A label in the preview's Properties: who the email was copied to (carbon copy)."
                ),
                Some(addresses(message.cc())),
            ),
            (
                words!(
                    email_detail_sent = "Sent",
                    note = "A label in the preview's Properties: when the email was sent."
                ),
                Some(sent),
            ),
            (
                words!(
                    email_detail_attachments = "Attachments",
                    note = "A label in the preview's Properties: how many files the email carries."
                ),
                inspection::count(attachments),
            ),
        ]);
        let rows = message
            .attachments()
            .enumerate()
            .take(limits::ATTACHMENTS)
            .map(|(i, a)| {
                let bytes = a.decoded_len() as u64;
                Row {
                    cells: vec![
                        a.attachment_name()
                            .map_or_else(|| unnamed().say(), str::to_owned),
                        inspection::size(bytes),
                    ],
                    target: Some(i.to_string()),
                    bytes: Some(bytes),
                    ..Row::default()
                }
            })
            .collect();
        // Only what applies to this message: a strip that always says the
        // same thing stops being read.
        let notices = [
            remote.then(|| {
                words!(
                    email_remote_images = "Remote images are not loaded",
                    note = "A note under an email: pictures it names on the web are not fetched."
                )
            }),
            truncated.then(|| inspection::blocks_notice(bounds)),
            (attachments > 0).then(|| {
                words!(
                    email_select_attachment = "Select an attachment to preview it",
                    note = "A note under an email that carries files: choosing one in the list shows it."
                )
            }),
        ]
        .into_iter()
        .flatten()
        .collect();
        Ok(Some(Inspection {
            details,
            kind: skwir_core::inspection::InspectionKind::Mail,
            title: words!(
                email_title = "Email",
                note = "The title of an email's preview."
            ),
            blocks,
            rows,
            source_available: true,
            notices,
            whole_available: truncated && bounds.cut(),
            ..Inspection::default()
        }))
    }
}
/// The message whole, from what the preview holds of it.
fn read<'a>(
    held: &'a Arc<Held>,
    bounds: Bounds,
    cancel: &CancelToken,
) -> anyhow::Result<Cow<'a, [u8]>> {
    let refusal = words!(
        email_too_large = "The email exceeds the { $size } preview limit",
        size = words::Size(bounds.binary_input() as u64),
        note = "Why an email is not previewed. size: the largest the preview reads (16 MB)."
    );
    inspection::read_file(held, bounds, refusal, cancel)
}
/// The one parse both the message and its attachments are read from.
fn parse<'x>(bytes: &'x [u8], cancel: &CancelToken) -> anyhow::Result<mail_parser::Message<'x>> {
    let message = mail_parser::MessageParser::default()
        .parse(bytes)
        .ok_or_else(|| {
            anyhow::anyhow!(words!(
                email_invalid = "Invalid email",
                note = "An error: the file could not be read as an email."
            ))
        })?;
    inspection::check(cancel)?;
    Ok(message)
}
/// What an attachment with no name is called in the list of attachments.
fn unnamed() -> words::Words {
    words!(
        email_unnamed_attachment = "Attachment",
        note = "An attachment the email gives no name, in its list of attachments."
    )
}
/// `Name <address>`, or the bare address when there is no name.
fn addresses(value: Option<mail_parser::AddressList<'_>>) -> String {
    value
        .map(|v| {
            v.mailboxes()
                .take(limits::ADDRESSES)
                .map(|a| {
                    let address = a.address().unwrap_or("");
                    match a.name().map(str::trim) {
                        Some(name) if !name.is_empty() => format!("{name} <{address}>"),
                        _ => address.to_owned(),
                    }
                })
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use skwir_core::preview::HeldSlot;

    fn read(message: &str) -> Inspection {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Synthetic message.eml");
        std::fs::write(&path, message).unwrap();
        let request = PreviewRequest {
            entry: skwir_core::op::EntryRef::from_path(&path).unwrap(),
            mime: Some("message/rfc822".into()),
            page: 1,
            seconds: None,
            max_side: 64,
            text_size: None,
            fit: None,
            held: HeldSlot::default(),
        };
        Provider
            .inspect(&request, &InspectRequest::default(), &CancelToken::new())
            .unwrap()
            .unwrap()
    }

    #[test]
    fn a_plain_message_says_nothing_it_has_no_reason_to() {
        let data = read(
            "From: sender@example.invalid\r\nTo: Reader <reader@example.invalid>\r\nSubject: Plain\r\n\r\nJust text.\r\n",
        );
        assert!(data.plain_text().contains("From: sender@example.invalid\n"));
        assert!(
            data.plain_text()
                .contains("To: Reader <reader@example.invalid>")
        );
        assert!(data.plain_text().contains("Just text."));
        assert!(data.notices.is_empty(), "{:?}", data.notices);
    }

    #[test]
    fn a_plain_message_links_the_addresses_it_writes() {
        let data = read(
            "From: sender@example.invalid\r\nSubject: Plain\r\n\r\nRead https://example.invalid/notes.\r\n",
        );
        let link = data
            .blocks
            .iter()
            .flat_map(|block| block.runs.iter().map(move |run| (block, run)))
            .find_map(|(block, run)| {
                Some((
                    &block.text[run.range.clone()],
                    run.link.as_deref()?,
                    run.found,
                ))
            });
        assert_eq!(
            link,
            Some((
                "https://example.invalid/notes",
                "https://example.invalid/notes",
                false
            ))
        );
    }

    #[test]
    fn remote_images_and_attachments_are_mentioned_when_there_are_some() {
        let data = read(
            "From: Example <sender@example.invalid>\r\nSubject: Rich\r\nMIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=sample\r\n\r\n--sample\r\nContent-Type: text/html; charset=UTF-8\r\n\r\n<p>Hi</p><img src='https://example.invalid/pixel.png'>\r\n--sample\r\nContent-Type: application/octet-stream\r\nContent-Disposition: attachment; filename=data.bin\r\nContent-Transfer-Encoding: base64\r\n\r\nAAAA\r\n--sample--\r\n",
        );
        assert_eq!(
            data.notice().map(|said| said.english()).as_deref(),
            Some("Remote images are not loaded · Select an attachment to preview it")
        );
        assert_eq!(data.rows[0].cells, ["data.bin", "3 B"]);
        assert_eq!(data.rows[0].bytes, Some(3));
    }

    /// A preview reads its file once (ADR-101): the message and its
    /// attachments come from what the request holds, named where no file
    /// is, so a read of the path would fail.
    #[test]
    fn a_message_and_its_attachments_come_from_what_is_held() {
        let dir = tempfile::tempdir().unwrap();
        let request = PreviewRequest::holding(
            dir.path().join("gone").join("l’été message.eml"),
            b"From: sender@example.invalid\r\nSubject: Held\r\nMIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=sample\r\n\r\n--sample\r\nContent-Type: text/plain\r\n\r\nJust text.\r\n--sample\r\nContent-Type: application/octet-stream\r\nContent-Disposition: attachment; filename=data.bin\r\nContent-Transfer-Encoding: base64\r\n\r\nAQID\r\n--sample--\r\n".to_vec(),
            Some("message/rfc822"),
        );
        assert!(Provider.accepts(&request));
        let view = InspectRequest::default();
        let data = Provider
            .inspect(&request, &view, &CancelToken::new())
            .unwrap()
            .unwrap();
        assert!(data.plain_text().contains("Just text."));
        assert_eq!(data.rows[0].cells, ["data.bin", "3 B"]);
        let member = Provider
            .extract(&request, &view, "0", dir.path(), &CancelToken::new())
            .unwrap()
            .unwrap();
        assert_eq!(member.name, "data.bin");
        assert_eq!(std::fs::read(&member.path).unwrap(), [1, 2, 3]);
    }
}
