//! Shared bounded reads and embedded image decoding, called only by workers.
use bibli_core::{
    CancelToken,
    inspection::{Block, Inspection},
    thumbs::Image,
};
use std::{
    io::{Read, Seek, SeekFrom},
    path::Path,
};
pub const TEXT_LIMIT: usize = 2 * 1024 * 1024;
pub const IMAGE_LIMIT: usize = 16 * 1024 * 1024;
pub fn check(cancel: &CancelToken) -> anyhow::Result<()> {
    anyhow::ensure!(!cancel.is_cancelled(), "preview cancelled");
    Ok(())
}
pub fn read(
    path: &Path,
    offset: u64,
    limit: usize,
    cancel: &CancelToken,
) -> anyhow::Result<Vec<u8>> {
    check(cancel)?;
    let mut file = std::fs::File::open(path)?;
    anyhow::ensure!(file.metadata()?.is_file(), "preview needs a regular file");
    file.seek(SeekFrom::Start(offset))?;
    let mut out = Vec::new();
    file.take(limit as u64).read_to_end(&mut out)?;
    check(cancel)?;
    Ok(out)
}
pub fn text(path: &Path, cancel: &CancelToken) -> anyhow::Result<String> {
    let bytes = read(path, 0, TEXT_LIMIT + 1, cancel)?;
    anyhow::ensure!(
        bytes.len() <= TEXT_LIMIT,
        "Text preview limited to 2 MiB; use Source or Hex for ranges"
    );
    Ok(String::from_utf8(bytes)?)
}
pub fn source(path: &Path, offset: u64, cancel: &CancelToken) -> anyhow::Result<Inspection> {
    let bytes = read(path, offset, 32 * 1024 + 1, cancel)?;
    let more = bytes.len() > 32 * 1024;
    let end = bytes.len().min(32 * 1024);
    Ok(Inspection {
        title: format!("Source · byte {offset}"),
        blocks: vec![Block {
            text: String::from_utf8_lossy(&bytes[..end]).into_owned(),
            code: true,
            ..Block::default()
        }],
        position: offset,
        previous: (offset > 0).then(|| offset.saturating_sub(32 * 1024)),
        next: more.then_some(offset + end as u64),
        source_available: true,
        ..Inspection::default()
    })
}
pub fn image(bytes: &[u8]) -> anyhow::Result<Image> {
    anyhow::ensure!(bytes.len() <= IMAGE_LIMIT, "Embedded image exceeds 16 MiB");
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    let img = reader.decode()?.thumbnail(1600, 1600).into_rgba8();
    Ok(Image {
        width: img.width(),
        height: img.height(),
        rgba: img.into_raw().into(),
    })
}
pub fn extension(path: &Path) -> String {
    path.extension()
        .unwrap_or_default()
        .to_string_lossy()
        .to_ascii_lowercase()
}
