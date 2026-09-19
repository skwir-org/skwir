//! Range-based hex viewer; whole-file searches stream and support cancellation.
use bibli_core::{
    App, CancelToken,
    inspection::{InspectRequest, Inspection, Row},
    plugin::Plugin,
    preview::{PreviewProvider, PreviewRequest},
};
use bibli_platform::inspection::{check, read};
use std::{io::Read, sync::Arc};
pub struct PreviewPlugin;
impl Plugin for PreviewPlugin {
    fn id(&self) -> &'static str {
        "preview-hex"
    }
    fn title(&self) -> &'static str {
        "Preview: hexadecimal"
    }
    fn register(&self, app: &App) {
        app.previews.register(self.id(), Arc::new(Provider));
    }
}
struct Provider;
const PAGE: u64 = 4096;
impl PreviewProvider for Provider {
    fn priority(&self) -> i32 {
        -100
    }
    fn id(&self) -> &'static str {
        "preview-hex"
    }
    fn accepts(&self, req: &PreviewRequest) -> bool {
        req.mime.as_deref() == Some("application/octet-stream")
    }
    fn inspect(
        &self,
        req: &PreviewRequest,
        view: &InspectRequest,
        cancel: &CancelToken,
    ) -> anyhow::Result<Option<Inspection>> {
        let path = req.entry.resolve()?;
        let len = path.metadata()?.len();
        let mut offset = view.offset.min(len.saturating_sub(1)) / 16 * 16;
        let mut notice = String::new();
        if let Some(query) = &view.query {
            let needle = if let Some(hex) = query.strip_prefix("hex:") {
                let hex: String = hex.chars().filter(|c| !c.is_whitespace()).collect();
                anyhow::ensure!(
                    hex.is_ascii() && hex.len() % 2 == 0,
                    "Use pairs of hexadecimal digits"
                );
                (0..hex.len())
                    .step_by(2)
                    .map(|i| u8::from_str_radix(&hex[i..i + 2], 16))
                    .collect::<Result<Vec<_>, _>>()?
            } else {
                query.as_bytes().to_vec()
            };
            anyhow::ensure!(
                !needle.is_empty() && needle.len() <= 4096,
                "Search must contain 1–4096 bytes"
            );
            if let Some(found) = find(&path, &needle, view.offset, cancel)? {
                offset = found / 16 * 16;
                notice = format!("Match at 0x{found:X}");
            } else {
                notice = "No further match".into();
            }
        }
        let bytes = read(&path, offset, PAGE as usize, cancel)?;
        let rows = bytes
            .chunks(16)
            .enumerate()
            .map(|(i, chunk)| Row {
                cells: vec![
                    format!("{:016X}", offset + i as u64 * 16),
                    hex_bytes(chunk),
                    chunk
                        .iter()
                        .map(|b| {
                            if b.is_ascii_graphic() || *b == b' ' {
                                char::from(*b)
                            } else {
                                '.'
                            }
                        })
                        .collect(),
                ],
                ..Row::default()
            })
            .collect();
        Ok(Some(Inspection {
            kind: bibli_core::inspection::InspectionKind::Hex,
            title: format!("Hex · {len} bytes"),
            rows,
            columns: vec![
                "Offset".into(),
                hex_bytes(&(0..16).collect::<Vec<_>>()),
                "ASCII".into(),
            ],
            position: offset,
            previous: (offset > 0).then(|| offset.saturating_sub(PAGE)),
            next: (offset + PAGE < len).then_some(offset + PAGE),
            notice,
            ..Inspection::default()
        }))
    }
}
fn find(
    path: &std::path::Path,
    needle: &[u8],
    offset: u64,
    cancel: &CancelToken,
) -> anyhow::Result<Option<u64>> {
    use std::io::{Seek, SeekFrom};
    let mut file = std::fs::File::open(path)?;
    file.seek(SeekFrom::Start(offset))?;
    let mut bytes = vec![0; 64 * 1024 + needle.len()];
    let mut keep = 0;
    let mut base = offset;
    loop {
        check(cancel)?;
        let n = file.read(&mut bytes[keep..])?;
        if n == 0 {
            return Ok(None);
        }
        let available = keep + n;
        if let Some(i) = bytes[..available]
            .windows(needle.len())
            .position(|w| w == needle)
        {
            return Ok(Some(base + i as u64));
        }
        keep = (needle.len() - 1).min(available);
        bytes.copy_within(available - keep..available, 0);
        base += (available - keep) as u64;
    }
}

fn hex_bytes(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 {
            out.push(' ');
            if i % 8 == 0 {
                out.push(' ');
            }
        }
        let _ = write!(out, "{b:02X}");
    }
    out
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn search_crosses_chunks_and_cancels() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let mut data = vec![0; 65537];
        data.extend(b"needle");
        std::fs::write(tmp.path(), data).unwrap();
        assert_eq!(
            find(tmp.path(), b"needle", 0, &CancelToken::new()).unwrap(),
            Some(65537)
        );
        let c = CancelToken::new();
        c.cancel();
        assert!(find(tmp.path(), b"x", 0, &c).is_err());
    }
}
