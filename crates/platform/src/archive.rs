//! Small native helper for archive metadata and bounded member reads.
use crate::inspection::{self, limits};
use bibli_core::{
    CancelToken,
    inspection::{Block, Inspection},
};
use serde::Deserialize;
use std::{path::Path, process::Command};
#[derive(Clone, Debug, Deserialize)]
pub struct Member {
    pub index: u64,
    pub name: String,
    pub size: i64,
    pub directory: bool,
    pub regular: bool,
    pub encrypted: bool,
}
fn worker() -> anyhow::Result<std::path::PathBuf> {
    crate::process::find_worker("BIBLI_ARCHIVE_WORKER", "bibli-archive-worker")
        .map_or_else(|| crate::process::required("bibli-archive-worker"), Ok)
}
pub fn list(path: &Path, cancel: &CancelToken) -> anyhow::Result<Vec<Member>> {
    let bytes = crate::process::run(
        Command::new(worker()?)
            .arg(path)
            .arg("--members")
            .arg(limits::CONTAINER_MEMBERS.to_string())
            .arg("--names")
            .arg(limits::MEMBER_NAMES.to_string()),
        cancel,
        limits::HELPER_TIMEOUT,
    )?;
    Ok(serde_json::from_slice(&bytes)?)
}
/// The worker enforces the same member bound it is given here, so a member
/// larger than the preview limit fails inside the helper before any read.
pub fn read(path: &Path, index: u64, cancel: &CancelToken) -> anyhow::Result<Vec<u8>> {
    let temp = tempfile::tempdir()?;
    let output = temp.path().join("member");
    crate::process::run(
        Command::new(worker()?)
            .arg(path)
            .arg(index.to_string())
            .arg(&output)
            .arg("--bytes")
            .arg(limits::BINARY_INPUT.to_string()),
        cancel,
        limits::HELPER_TIMEOUT,
    )?;
    inspection::read_bounded(&output, limits::BINARY_INPUT, "Member preview", cancel)
}
/// Image, text prefix or paged hexadecimal view of a container member or
/// attachment. `offset` pages the byte view; text and images ignore it.
pub fn member_preview(name: &str, bytes: &[u8], offset: u64) -> anyhow::Result<Inspection> {
    if let Ok(image) = inspection::image(bytes) {
        return Ok(Inspection {
            title: name.into(),
            image: Some(image),
            ..Inspection::default()
        });
    }
    let len = bytes.len().min(limits::MEMBER_TEXT);
    if let Some(text) = inspection::text_prefix(&bytes[..len]) {
        return Ok(Inspection {
            title: name.into(),
            blocks: vec![Block {
                text: text.to_owned(),
                code: true,
                ..Block::default()
            }],
            notice: format!(
                "Member preview · first {} of text",
                limits::describe(limits::MEMBER_TEXT)
            ),
            ..Inspection::default()
        });
    }
    let total = bytes.len() as u64;
    let offset = offset.min(total.saturating_sub(1)) / inspection::HEX_ROW as u64
        * inspection::HEX_ROW as u64;
    let end = (offset + limits::HEX_PAGE).min(total);
    let mut page = inspection::hex_page(
        format!("{name} · {total} bytes"),
        &bytes[offset as usize..end as usize],
        total,
        offset,
        offset,
    );
    page.notice = "Member preview · bytes".into();
    Ok(page)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bibli_core::inspection::InspectionKind;

    #[test]
    fn member_preview_detects_image_text_and_bytes() {
        let mut png = std::io::Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(2, 2, image::Rgba([1, 2, 3, 255]))
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        let image = member_preview("cover.png", png.get_ref(), 0).unwrap();
        assert_eq!(
            image.image.as_ref().map(|i| (i.width, i.height)),
            Some((2, 2))
        );
        assert!(image.blocks.is_empty());

        let text = member_preview("notes.txt", "caf\u{e9} note\n".as_bytes(), 0).unwrap();
        assert_eq!(text.kind, InspectionKind::Text);
        assert_eq!(text.plain_text(), "caf\u{e9} note\n");
        assert!(text.blocks[0].code);

        // The text cut lands inside the last two-byte character.
        let mut long = b"a".to_vec();
        long.extend_from_slice("\u{e9}".repeat(limits::MEMBER_TEXT / 2).as_bytes());
        let cut = member_preview("long.txt", &long, 0).unwrap();
        assert_eq!(cut.kind, InspectionKind::Text);
        assert_eq!(cut.plain_text().len(), limits::MEMBER_TEXT - 1);

        let binary: Vec<u8> = (0..=255u8).cycle().take(5000).collect();
        let bytes = member_preview("blob.bin", &binary, 0).unwrap();
        assert_eq!(bytes.kind, InspectionKind::Hex);
        assert_eq!(bytes.rows.len(), 256);
        assert_eq!(bytes.next, Some(4096));
        assert_eq!(bytes.columns, inspection::hex_columns());
        let second = member_preview("blob.bin", &binary, 4100).unwrap();
        assert_eq!(second.position, 4096);
        assert_eq!(second.rows[0].cells[0], "0000000000001000");
        assert_eq!(second.rows.len(), (5000usize - 4096).div_ceil(16));
        assert_eq!(second.previous, Some(0));
        assert_eq!(second.next, None);
    }
}
