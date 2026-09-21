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
        let path = req.entry.resolve()?;
        let bytes = inspection::read_bounded(&path, limits::BINARY_INPUT, "Email", cancel)?;
        let message = mail_parser::MessageParser::default()
            .parse(&bytes)
            .ok_or_else(|| anyhow::anyhow!("Invalid email"))?;
        inspection::check(cancel)?;
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
    fn inspect(
        &self,
        req: &PreviewRequest,
        view: &InspectRequest,
        cancel: &CancelToken,
    ) -> anyhow::Result<Option<Inspection>> {
        let path = req.entry.resolve()?;
        if view.source {
            return inspection::source(&path, view.offset, cancel).map(Some);
        }
        let bytes = inspection::read_bounded(&path, limits::BINARY_INPUT, "Email", cancel)?;
        let message = mail_parser::MessageParser::default()
            .parse(&bytes)
            .ok_or_else(|| anyhow::anyhow!("Invalid email"))?;
        inspection::check(cancel)?;
        if let Some(target) = &view.target {
            let attachment = message
                .attachment(target.parse()?)
                .ok_or_else(|| anyhow::anyhow!("Missing attachment"))?;
            return archive::member_preview(
                attachment.attachment_name().unwrap_or("Attachment"),
                attachment.contents(),
                view.offset,
            )
            .map(Some);
        }
        let mut blocks = vec![
            Block {
                text: message.subject().unwrap_or("No subject").into(),
                heading: 1,
                ..Block::default()
            },
            Block {
                text: format!(
                    "From: {}\nTo: {}\nDate: {}",
                    addresses(message.from()),
                    addresses(message.to()),
                    message
                        .date()
                        .map_or_else(String::new, mail_parser::DateTime::to_rfc3339)
                ),
                ..Block::default()
            },
        ];
        if let Some(html) = message.body_html(0) {
            blocks.extend(rich_preview::html(&html, cancel, |src| {
                let id = src.strip_prefix("cid:")?;
                let part = message.parts.iter().find(|p| p.content_id() == Some(id))?;
                inspection::image(part.contents()).ok()
            })?);
        } else if let Some(text) = message.body_text(0) {
            blocks.push(Block {
                text: text.into_owned(),
                ..Block::default()
            });
        }
        let details = [
            ("Subject", message.subject().unwrap_or_default().to_owned()),
            ("From", addresses(message.from())),
            ("To", addresses(message.to())),
            ("Cc", addresses(message.cc())),
            (
                "Sent",
                message
                    .date()
                    .map_or_else(String::new, mail_parser::DateTime::to_rfc3339),
            ),
            (
                "Attachments",
                match message.attachments().count() {
                    0 => String::new(),
                    count => count.to_string(),
                },
            ),
        ]
        .into_iter()
        .filter_map(|(label, value)| inspection::detail(label, value))
        .collect();
        let rows = message
            .attachments()
            .enumerate()
            .take(limits::ATTACHMENTS)
            .map(|(i, a)| Row {
                cells: vec![
                    a.attachment_name().unwrap_or("Attachment").into(),
                    format!("{} bytes", a.contents().len()),
                ],
                target: Some(i.to_string()),
                bytes: Some(a.contents().len() as u64),
                ..Row::default()
            })
            .collect();
        Ok(Some(Inspection {
            details,
            kind: bibli_core::inspection::InspectionKind::Mail,
            title: "Email".into(),
            blocks,
            rows,
            source_available: true,
            notice: "Remote images are not loaded · select an attachment to preview".into(),
            ..Inspection::default()
        }))
    }
}
fn addresses(value: Option<&mail_parser::Address<'_>>) -> String {
    value
        .map(|v| {
            v.iter()
                .take(limits::ADDRESSES)
                .map(|a| {
                    format!(
                        "{} <{}>",
                        a.name.as_deref().unwrap_or(""),
                        a.address.as_deref().unwrap_or("")
                    )
                })
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default()
}
