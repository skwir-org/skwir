//! A HEIC and an AVIF read where any picture is (ADR-148): through
//! `image`, taught by `picture::formats`, by the helper this crate builds.

use std::path::{Path, PathBuf};

use skwir_platform::picture::{self, Threads};

/// The helper named, then the formats taught.
fn taught() {
    // SAFETY: set before anything of this test's process reads its
    // environment on another thread: nextest runs each test in a process
    // of its own.
    #[allow(unsafe_code)]
    unsafe {
        std::env::set_var("SKWIR_HEIF_WORKER", env!("CARGO_BIN_EXE_skwir-heif-worker"));
    }
    picture::formats();
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data")
        .join(name)
}

/// A HEIC's size, then its pixels shrunk as a thumbnail is: a gradient
/// from red at the top to blue at the bottom.
#[test]
fn a_heic_is_read_as_any_picture() {
    taught();
    assert!(skwir_platform::heif::reads("image/heic"));
    let path = fixture("synthetic-gradient.heic");
    assert_eq!(
        picture::size(picture::open(&path).unwrap()).unwrap(),
        (96, 64)
    );
    let fitted = picture::decode_fitted(picture::open(&path).unwrap(), 48, Threads::Caller)
        .unwrap()
        .to_rgb8();
    assert_eq!(fitted.dimensions(), (48, 32));
    let [r, _, b] = fitted.get_pixel(24, 1).0;
    assert!(r > 170 && b < 90, "the top is red: {r} {b}");
    let [r, _, b] = fitted.get_pixel(24, 30).0;
    assert!(r < 90 && b > 170, "the bottom is blue: {r} {b}");
}

/// An AVIF keeps its alpha: opaque at the top, clear at the bottom.
#[test]
fn an_avif_keeps_its_alpha() {
    taught();
    let path = fixture("synthetic-fade.avif");
    let decoded = picture::decode(picture::open(&path).unwrap(), image::Limits::default())
        .unwrap()
        .to_rgba8();
    assert_eq!(decoded.dimensions(), (96, 64));
    assert!(decoded.get_pixel(48, 1).0[3] > 230);
    assert!(decoded.get_pixel(48, 62).0[3] < 25);
}

/// Without the helper, a HEIC is no type read here: the desktop's
/// thumbnailer draws it.
#[test]
fn without_the_helper_a_heic_is_left_to_the_desktop() {
    // SAFETY: as in `taught`.
    #[allow(unsafe_code)]
    unsafe {
        std::env::set_var("SKWIR_HEIF_WORKER", fixture("no-such-helper"));
    }
    assert!(!skwir_platform::heif::reads("image/heic"));
    assert!(!skwir_platform::thumb_providers::decodes("image/avif"));
}
