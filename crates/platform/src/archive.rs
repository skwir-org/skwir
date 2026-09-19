//! Small native helper for archive metadata and bounded member reads.
use bibli_core::CancelToken;
use serde::Deserialize;
use std::{path::Path, process::Command, time::Duration};
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
    if let Some(path) = std::env::var_os("BIBLI_ARCHIVE_WORKER") {
        return Ok(path.into());
    }
    if let Some(path) = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.join("bibli-archive-worker")))
        .filter(|p| p.is_file())
    {
        return Ok(path);
    }
    crate::process::required("bibli-archive-worker")
}
pub fn list(path: &Path, cancel: &CancelToken) -> anyhow::Result<Vec<Member>> {
    let bytes = crate::process::run(
        Command::new(worker()?).arg(path),
        cancel,
        Duration::from_secs(15),
    )?;
    Ok(serde_json::from_slice(&bytes)?)
}
pub fn read(path: &Path, index: u64, cancel: &CancelToken) -> anyhow::Result<Vec<u8>> {
    let temp = tempfile::tempdir()?;
    let output = temp.path().join("member");
    crate::process::run(
        Command::new(worker()?)
            .arg(path)
            .arg(index.to_string())
            .arg(&output),
        cancel,
        Duration::from_secs(15),
    )?;
    let bytes = crate::inspection::read(&output, 0, crate::inspection::IMAGE_LIMIT + 1, cancel)?;
    anyhow::ensure!(
        bytes.len() <= crate::inspection::IMAGE_LIMIT,
        "Member preview exceeds 16 MiB"
    );
    Ok(bytes)
}
pub fn member_preview(
    name: &str,
    bytes: &[u8],
) -> anyhow::Result<bibli_core::inspection::Inspection> {
    use bibli_core::inspection::{Block, Inspection};
    if let Ok(image) = crate::inspection::image(bytes) {
        return Ok(Inspection {
            title: name.into(),
            image: Some(image),
            ..Inspection::default()
        });
    }
    let len = bytes.len().min(64 * 1024);
    let text = match std::str::from_utf8(&bytes[..len]) {
        Ok(text) if !text.contains('\0') => text.to_owned(),
        _ => {
            use std::fmt::Write;
            let mut text = String::new();
            for (i, chunk) in bytes[..len.min(4096)].chunks(16).enumerate() {
                let _ = write!(text, "{:08X}  ", i * 16);
                for byte in chunk {
                    let _ = write!(text, "{byte:02X} ");
                }
                text.push('\n');
            }
            text
        }
    };
    Ok(Inspection {
        title: name.into(),
        blocks: vec![Block {
            text,
            code: true,
            ..Block::default()
        }],
        notice: "Member preview · first 64 KiB of text or 4 KiB of bytes".into(),
        ..Inspection::default()
    })
}
