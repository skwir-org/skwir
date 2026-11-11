//! What a file says of itself that an order reads on demand (ADR-131):
//! a picture's size in pixels and a sound's or a film's length, too
//! costly for every listing, read on a worker when a pane sorts or groups
//! by them (`skwir_core::fields`, which keeps the answers by the file).

use std::path::Path;
use std::time::Duration;

use skwir_core::cancel::CancelToken;

/// How long ffprobe may take over one file.
const PROBE_TIMEOUT: Duration = Duration::from_secs(10);

/// A picture's width and height in pixels, from its header alone: no
/// picture is decoded. None for what the header reader does not know
/// (`imagesize`: PNG, JPEG, GIF, WebP, AVIF and HEIF, TIFF, BMP, PSD, and
/// more) or a file it cannot read.
pub fn dimensions(path: &Path) -> Option<(u64, u64)> {
    let file = skwir_core::fs::file::open_regular(path).ok()?;
    let read = imagesize::reader_size(std::io::BufReader::new(file)).ok()?;
    let pixels = |n: usize| u64::try_from(n).ok().filter(|n| *n > 0);
    Some((pixels(read.width)?, pixels(read.height)?))
}

/// How long a sound or a film lasts, in milliseconds: from an MP4's or a
/// `QuickTime` file's own boxes (`mp4::duration`), else from ffprobe where it is
/// installed (ADR-000, 4), run as the video preview runs it, in a sandbox
/// where the system gives one. None where neither says, or ffprobe is not
/// there.
///
/// # Errors
/// What ffprobe's run says when it fails: a retry may answer.
pub fn duration(path: &Path, cancel: &CancelToken) -> anyhow::Result<Option<u64>> {
    if let Some(ms) = crate::mp4::duration(path) {
        return Ok(Some(ms));
    }
    let Ok(ffprobe) = crate::process::required("ffprobe") else {
        return Ok(None);
    };
    let work = tempfile::tempdir()?;
    let mut probe =
        crate::sandbox::untrusted(&ffprobe, path, work.path(), crate::sandbox::Otherwise::Run)?;
    probe
        .command
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "default=noprint_wrappers=1:nokey=1",
        ])
        .arg(&probe.input);
    let out = crate::process::run(&mut probe.command, cancel, PROBE_TIMEOUT)?;
    Ok(seconds_said(&String::from_utf8_lossy(&out)))
}

/// What ffprobe says of a duration, `12.345000` seconds, in milliseconds;
/// none for `N/A` or nothing.
fn seconds_said(text: &str) -> Option<u64> {
    let seconds: f64 = text.trim().parse().ok()?;
    let ms = (seconds * 1000.).round();
    // A length of days is a damaged header, not a film.
    (ms.is_finite() && (1. ..1e12).contains(&ms)).then_some(ms as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A PNG's header says its size; a file of another kind says none.
    #[test]
    fn a_picture_says_its_size_from_its_header() {
        let dir = tempfile::tempdir().unwrap();
        let png = dir.path().join("dunes.png");
        image::RgbImage::new(37, 21).save(&png).unwrap();
        assert_eq!(dimensions(&png), Some((37, 21)));
        let text = dir.path().join("notes.txt");
        std::fs::write(&text, "not a picture").unwrap();
        assert_eq!(dimensions(&text), None);
        assert_eq!(dimensions(&dir.path().join("gone.png")), None);
    }

    #[test]
    fn what_ffprobe_says_of_a_length_is_read_in_milliseconds() {
        assert_eq!(seconds_said("12.345000\n"), Some(12_345));
        assert_eq!(seconds_said("0.0004"), None, "rounds to nothing");
        assert_eq!(seconds_said("N/A"), None);
        assert_eq!(seconds_said(""), None);
    }
}
