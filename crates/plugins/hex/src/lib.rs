//! Range-based hex viewer; whole-file searches stream and support cancellation.
use skwir_core::{
    App, CancelToken,
    inspection::{InspectRequest, Inspection},
    plugin::Plugin,
    preview::{PreviewProvider, PreviewRequest},
};
use skwir_platform::inspection::{self, HEX_ROW, check, limits, read};
use std::{io::Read, sync::Arc};
pub struct PreviewPlugin;
impl Plugin for PreviewPlugin {
    fn id(&self) -> &'static str {
        "hex"
    }
    fn title(&self) -> &'static str {
        "Hexadecimal"
    }
    fn description(&self) -> &'static str {
        "Inspect any file as hexadecimal bytes"
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
struct Provider;
impl PreviewProvider for Provider {
    fn priority(&self) -> i32 {
        -100
    }
    fn shows_bytes(&self) -> bool {
        true
    }
    fn id(&self) -> &'static str {
        "hex"
    }
    /// A request says `application/octet-stream` only once the bytes were
    /// read and found binary (`skwir_platform::mime::by_content`): a name
    /// the database does not know is not a claim (`mime::by_name` says
    /// nothing for it), so a `settings.cfg` of text is read as text.
    fn accepts(&self, req: &PreviewRequest) -> bool {
        req.mime.as_deref() == Some("application/octet-stream")
    }
    /// A search starts at `view.offset` and reports the match's exact byte as
    /// `position`, so a caller that resumes one byte later finds the next one.
    fn inspect(
        &self,
        req: &PreviewRequest,
        view: &InspectRequest,
        cancel: &CancelToken,
    ) -> anyhow::Result<Option<Inspection>> {
        let path = req.entry.resolve()?;
        let len = path.metadata()?.len();
        let row = HEX_ROW as u64;
        let mut position = view.offset.min(len.saturating_sub(1)) / row * row;
        let mut notice = String::new();
        if let Some(query) = &view.query {
            let needle = needle(query)?;
            if let Some(found) = find(&path, &needle, view.offset, cancel)? {
                position = found;
                notice = format!("Match at 0x{found:X}");
            } else {
                notice = "No further match".into();
            }
        }
        let offset = position / row * row;
        let bytes = read(&path, offset, limits::HEX_PAGE as usize, cancel)?;
        let mut page = inspection::hex_page(
            format!("Hex · {}", inspection::size(len)),
            &bytes,
            len,
            offset,
            position,
        );
        page.notice = notice;
        Ok(Some(page))
    }
}
/// `hex: 6e 65` byte pairs, or the query's UTF-8 bytes.
fn needle(query: &str) -> anyhow::Result<Vec<u8>> {
    let needle = if let Some(hex) = query.strip_prefix("hex:") {
        let hex: String = hex.chars().filter(|c| !c.is_whitespace()).collect();
        anyhow::ensure!(
            hex.is_ascii() && hex.len().is_multiple_of(2),
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
        !needle.is_empty() && needle.len() <= limits::HEX_NEEDLE,
        "Search must contain 1–{} bytes",
        limits::HEX_NEEDLE
    );
    Ok(needle)
}
fn find(
    path: &std::path::Path,
    needle: &[u8],
    offset: u64,
    cancel: &CancelToken,
) -> anyhow::Result<Option<u64>> {
    use std::io::{Seek, SeekFrom};
    let mut file = inspection::open(path)?;
    file.seek(SeekFrom::Start(offset))?;
    let finder = memchr::memmem::Finder::new(needle);
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
        if let Some(i) = finder.find(&bytes[..available]) {
            return Ok(Some(base + i as u64));
        }
        keep = (needle.len() - 1).min(available);
        bytes.copy_within(available - keep..available, 0);
        base += (available - keep) as u64;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use skwir_core::op::EntryRef;

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

    #[test]
    fn needles_parse_hex_pairs_and_reject_empty_or_odd_input() {
        assert_eq!(needle("hex: 6e 65 ").unwrap(), b"ne");
        assert_eq!(needle("né").unwrap(), "né".as_bytes());
        assert!(needle("hex:6").is_err());
        assert!(needle("hex:zz").is_err());
        assert!(needle("").is_err());
        assert!(needle(&"a".repeat(limits::HEX_NEEDLE + 1)).is_err());
    }

    #[test]
    fn repeated_search_resumes_after_the_previous_match() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("Synthetic bytes.bin");
        let mut bytes = vec![0u8; 9000];
        // Two occurrences on the same row, then one on a later page.
        bytes[100..106].copy_from_slice(b"needle");
        bytes[108..114].copy_from_slice(b"needle");
        bytes[8000..8006].copy_from_slice(b"needle");
        std::fs::write(&path, &bytes).unwrap();
        let request = PreviewRequest {
            entry: EntryRef::from_path(&path).unwrap(),
            mime: Some("application/octet-stream".into()),
            page: 1,
            seconds: None,
            max_side: 64,
            text_size: None,
        };
        let cancel = CancelToken::new();
        let search = |offset: u64| {
            Provider
                .inspect(
                    &request,
                    &InspectRequest {
                        query: Some("needle".into()),
                        offset,
                        ..InspectRequest::default()
                    },
                    &cancel,
                )
                .unwrap()
                .unwrap()
        };
        let first = search(0);
        assert_eq!(first.position, 100);
        assert_eq!(
            first.rows[0].cells[0], "0000000000000060",
            "match row on top"
        );
        assert_eq!(first.notice, "Match at 0x64");
        let second = search(first.position + 1);
        assert_eq!(second.position, 108, "same row, distinct match");
        let third = search(second.position + 1);
        assert_eq!(third.position, 8000);
        assert_eq!(third.rows[0].cells[0], "0000000000001F40");
        assert_eq!(third.previous, Some(8000 - 4096));
        let none = search(third.position + 1);
        assert_eq!(none.notice, "No further match");
        assert_eq!(
            none.position,
            8000 / 16 * 16,
            "view stays on the current page"
        );
    }
}
