//! EML headers, inert HTML/text body and bounded attachment previews.
use bibli_core::{
    App, CancelToken,
    inspection::{Block, InspectRequest, Inspection, Row},
    plugin::Plugin,
    preview::{PreviewProvider, PreviewRequest},
};
use bibli_platform::{
    archive,
    inspection::{self, limits},
    rich_preview,
};
use mail_parser::MimeHeaders;
use std::sync::Arc;
pub struct PreviewPlugin;
impl Plugin for PreviewPlugin {
    fn id(&self) -> &'static str {
        "preview-email"
    }
    fn title(&self) -> &'static str {
        "Preview: email"
    }
    fn description(&self) -> &'static str {
        "Read EML messages: headers, body and attachments, with remote content blocked"
    }
    fn register(&self, app: &App) {
        app.previews.register(self.id(), Arc::new(Provider));
    }
}
struct Provider;
impl PreviewProvider for Provider {
    fn id(&self) -> &'static str {
        "preview-email"
    }
    fn accepts(&self, req: &PreviewRequest) -> bool {
        inspection::extension(&req.entry.path) == "eml"
            || req.mime.as_deref() == Some("message/rfc822")
    }
    fn extract(
        &self,
        req: &PreviewRequest,
        target: &str,
        into: &std::path::Path,
        cancel: &CancelToken,
    ) -> anyhow::Result<Option<bibli_core::preview::Member>> {
        let bytes = read(req, cancel)?;
        let message = parse(&bytes, cancel)?;
        let attachment = message
            .attachment(target.parse()?)
            .ok_or_else(|| anyhow::anyhow!("Missing attachment"))?;
        let name = attachment.attachment_name().unwrap_or("Attachment");
        let file = archive::member_file(into, name);
        std::fs::write(&file, attachment.contents())?;
        Ok(Some(bibli_core::preview::Member {
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
        let bytes = read(req, cancel)?;
        let message = parse(&bytes, cancel)?;
        let sent = message
            .date()
            .map_or_else(String::new, mail_parser::DateTime::to_rfc3339);
        let mut blocks = vec![
            Block {
                text: message.subject().unwrap_or("No subject").into(),
                heading: 1,
                ..Block::default()
            },
            Block {
                text: format!(
                    "From: {}\nTo: {}\nDate: {sent}",
                    addresses(message.from()),
                    addresses(message.to()),
                ),
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
                let part = message.parts.iter().find(|p| p.content_id() == Some(id))?;
                inspection::image(part.contents()).ok()
            });
            let page = rich_preview::html(&html, rich_preview::Markup::Html, cancel, images)?;
            truncated = page.truncated;
            blocks.extend(page.blocks);
        } else if let Some(text) = message.body_text(0) {
            blocks.push(Block {
                text: text.into_owned(),
                ..Block::default()
            });
        }
        let attachments = message.attachments().count();
        let details = inspection::details([
            ("Subject", message.subject().map(str::to_owned)),
            ("From", Some(addresses(message.from()))),
            ("To", Some(addresses(message.to()))),
            ("Cc", Some(addresses(message.cc()))),
            ("Sent", Some(sent)),
            ("Attachments", inspection::count(attachments)),
        ]);
        let rows = message
            .attachments()
            .enumerate()
            .take(limits::ATTACHMENTS)
            .map(|(i, a)| {
                let bytes = a.contents().len() as u64;
                Row {
                    cells: vec![
                        a.attachment_name().unwrap_or("Attachment").into(),
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
        let notice = [
            remote.then(|| "Remote images are not loaded".to_owned()),
            truncated.then(inspection::blocks_notice),
            (attachments > 0).then(|| "Select an attachment to preview it".to_owned()),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" · ");
        Ok(Some(Inspection {
            details,
            kind: bibli_core::inspection::InspectionKind::Mail,
            title: "Email".into(),
            blocks,
            rows,
            source_available: true,
            notice,
            ..Inspection::default()
        }))
    }
}
fn read(req: &PreviewRequest, cancel: &CancelToken) -> anyhow::Result<Vec<u8>> {
    inspection::read_bounded(&req.entry.resolve()?, limits::BINARY_INPUT, "Email", cancel)
}
/// The one parse both the message and its attachments are read from.
fn parse<'x>(bytes: &'x [u8], cancel: &CancelToken) -> anyhow::Result<mail_parser::Message<'x>> {
    let message = mail_parser::MessageParser::default()
        .parse(bytes)
        .ok_or_else(|| anyhow::anyhow!("Invalid email"))?;
    inspection::check(cancel)?;
    Ok(message)
}
/// `Name <address>`, or the bare address when there is no name.
fn addresses(value: Option<&mail_parser::Address<'_>>) -> String {
    value
        .map(|v| {
            v.iter()
                .take(limits::ADDRESSES)
                .map(|a| {
                    let address = a.address.as_deref().unwrap_or("");
                    match a.name.as_deref().map(str::trim) {
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

    fn read(message: &str) -> Inspection {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Synthetic message.eml");
        std::fs::write(&path, message).unwrap();
        let request = PreviewRequest {
            entry: bibli_core::op::EntryRef::from_path(&path).unwrap(),
            mime: Some("message/rfc822".into()),
            page: 1,
            seconds: None,
            max_side: 64,
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
        assert!(data.notice.is_empty(), "{}", data.notice);
    }

    #[test]
    fn remote_images_and_attachments_are_mentioned_when_there_are_some() {
        let data = read(
            "From: Example <sender@example.invalid>\r\nSubject: Rich\r\nMIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=sample\r\n\r\n--sample\r\nContent-Type: text/html; charset=UTF-8\r\n\r\n<p>Hi</p><img src='https://example.invalid/pixel.png'>\r\n--sample\r\nContent-Type: application/octet-stream\r\nContent-Disposition: attachment; filename=data.bin\r\nContent-Transfer-Encoding: base64\r\n\r\nAAAA\r\n--sample--\r\n",
        );
        assert_eq!(
            data.notice,
            "Remote images are not loaded · Select an attachment to preview it"
        );
        assert_eq!(data.rows[0].cells, ["data.bin", "3 B"]);
        assert_eq!(data.rows[0].bytes, Some(3));
    }
}
