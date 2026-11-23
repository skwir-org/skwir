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
    // As the thumbnails' discovery asks it, the program started.
    assert!(skwir_platform::heif::probe(), "the helper works");
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
    assert!(!skwir_platform::heif::probe());
    assert!(!skwir_platform::heif::reads("image/heic"));
    assert!(!skwir_platform::thumb_providers::decodes("image/avif"));
}

/// A helper that is found but cannot run (its library missing, as where
/// Arch's `libheif` is not installed) answers no probe: its types stay
/// the thumbnailer's.
#[cfg(unix)]
#[test]
fn a_helper_that_cannot_run_is_left_out() {
    use std::os::unix::fs::PermissionsExt;
    let folder = tempfile_folder();
    let helper = folder.join("skwir-heif-worker");
    std::fs::write(&helper, "#!/bin/sh\nexit 127\n").unwrap();
    std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o755)).unwrap();
    // SAFETY: as in `taught`.
    #[allow(unsafe_code)]
    unsafe {
        std::env::set_var("SKWIR_HEIF_WORKER", &helper);
    }
    assert!(
        !skwir_platform::heif::reads("image/heic"),
        "not before it answers"
    );
    assert!(!skwir_platform::heif::probe());
    assert!(!skwir_platform::heif::reads("image/heic"));
    let _ = std::fs::remove_dir_all(&folder);
}

/// A folder of this test's own, under the tests' cache (never `/tmp`).
fn tempfile_folder() -> PathBuf {
    let cache = std::env::var_os("XDG_CACHE_HOME").map_or_else(std::env::temp_dir, PathBuf::from);
    let folder = cache.join(format!("heif-helper-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    folder
}

/// What the helper says, run directly on the fixture `name`: the size it
/// decoded, its alpha, and for a region the tiles it decoded.
fn said(name: &str, args: &[&str]) -> String {
    // Tests run from a console: no console window to keep away.
    #[allow(clippy::disallowed_methods)]
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_skwir-heif-worker"))
        .arg(fixture(name))
        .arg("/dev/null")
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

/// A picture of more pixels than asked is refused in every mode, by its
/// area as shown (96 × 64 is 6144), before anything of it is decoded.
#[test]
fn a_picture_larger_than_asked_is_refused_by_its_area() {
    for (most, refused) in [("6143", true), ("6144", false)] {
        for mode in [
            vec!["size", most],
            vec!["decode", most],
            vec!["thumbnail", most],
            vec!["region", most, "0", "0", "10", "10"],
        ] {
            // Tests run from a console: no console window to keep away.
            #[allow(clippy::disallowed_methods)]
            let ran = std::process::Command::new(env!("CARGO_BIN_EXE_skwir-heif-worker"))
                .arg(fixture("synthetic-gradient.heic"))
                .arg("/dev/null")
                .args(&mode)
                .output()
                .unwrap();
            assert_eq!(!ran.status.success(), refused, "{mode:?}");
            if refused {
                let said = String::from_utf8_lossy(&ran.stderr);
                assert!(said.contains("larger than asked"), "{mode:?}: {said}");
            }
        }
    }
}

/// How far two pictures of one size differ: the pixels whose channels
/// differ by more than 3, of every pixel.
fn differing(a: &image::RgbImage, b: &image::RgbImage) -> (usize, usize) {
    assert_eq!(a.dimensions(), b.dimensions());
    let differs = a
        .pixels()
        .zip(b.pixels())
        .filter(|(a, b)| a.0.iter().zip(b.0).any(|(a, b)| a.abs_diff(b) > 3))
        .count();
    (differs, a.pixels().len())
}

/// A region of a HEIC stored in tiles is decoded from the tiles it covers
/// alone (ADR-148 amended): four of the six for one across their corner,
/// as the whole decoded and cut.
#[test]
fn a_heic_region_is_decoded_from_its_tiles() {
    taught();
    // 384 × 256, tiles of 128: three across, two down.
    assert_eq!(
        said(
            "synthetic-tiles.heic",
            &["region", "1000000", "100", "100", "100", "60"]
        ),
        "100 60 0 4"
    );
    let path = fixture("synthetic-tiles.heic");
    let whole = picture::decode(picture::open(&path).unwrap(), image::Limits::default())
        .unwrap()
        .to_rgb8();
    for region in [[0, 0, 100, 40], [100, 100, 100, 60], [300, 200, 84, 56]] {
        let cut = picture::decode_region(
            picture::open(&path).unwrap(),
            (384, 256),
            region,
            384,
            Threads::Caller,
        )
        .unwrap()
        .to_rgb8();
        let [x, y, width, height] = region;
        let expected = image::imageops::crop_imm(&whole, x, y, width, height).to_image();
        let (differs, of) = differing(&cut, &expected);
        assert!(differs * 100 < of, "{region:?}: {differs} of {of} differ");
    }
}

/// A HEIC turned a quarter counter-clockwise, its tiles reaching past its
/// top as it is shown: a region is cut where it is shown, from its tiles.
#[test]
fn a_turned_heic_region_is_cut_as_shown() {
    taught();
    // 300 × 200 as stored, red at the top and blue at the bottom; shown
    // 200 × 300, red on the left.
    // Its size, then its carried thumbnail's, as shown.
    assert_eq!(
        said("synthetic-turned-tiles.heic", &["size"]),
        "200 300 0 64 96"
    );
    assert_eq!(
        said("synthetic-gradient.heic", &["size"]),
        "96 64 0 0 0",
        "none"
    );
    assert_eq!(
        said(
            "synthetic-turned-tiles.heic",
            &["region", "1000000", "0", "0", "50", "300"]
        ),
        "50 300 0 3"
    );
    let path = fixture("synthetic-turned-tiles.heic");
    let whole = picture::decode(picture::open(&path).unwrap(), image::Limits::default())
        .unwrap()
        .to_rgb8();
    for (region, red) in [([0, 0, 50, 300], true), ([150, 0, 50, 300], false)] {
        let cut = picture::decode_region(
            picture::open(&path).unwrap(),
            (200, 300),
            region,
            300,
            Threads::Caller,
        )
        .unwrap()
        .to_rgb8();
        let [x, y, width, height] = region;
        let expected = image::imageops::crop_imm(&whole, x, y, width, height).to_image();
        let (differs, of) = differing(&cut, &expected);
        assert!(differs * 100 < of, "{region:?}: {differs} of {of} differ");
        let [r, _, b] = cut.get_pixel(25, 150).0;
        assert_eq!(r > b, red, "{region:?}: {r} {b}");
    }
}

/// A HEIC whose tiles are one (not stored in tiles) has its region cut
/// from the whole, in the helper.
#[test]
fn a_heic_in_one_piece_has_its_region_cut_from_the_whole() {
    taught();
    assert_eq!(
        said(
            "synthetic-gradient.heic",
            &["region", "1000000", "10", "0", "20", "64"]
        ),
        "20 64 0 0"
    );
    let cut = picture::decode_region(
        picture::open(&fixture("synthetic-gradient.heic")).unwrap(),
        (96, 64),
        [10, 0, 20, 64],
        96,
        Threads::Caller,
    )
    .unwrap()
    .to_rgb8();
    assert_eq!(cut.dimensions(), (20, 64));
    let [r, _, b] = cut.get_pixel(10, 1).0;
    assert!(r > 170 && b < 90, "the top is red: {r} {b}");
}

/// The thumbnail a HEIC carries stands for it while it decodes, turned
/// as it is shown; none where it carries none, or where it would not be
/// smaller than what is asked.
#[test]
fn a_heic_s_thumbnail_stands_in_for_it() {
    taught();
    let stand_in = |name: &str, side: u32| {
        picture::carried(
            picture::open(&fixture(name)).unwrap(),
            |longer| longer < side,
            Threads::Caller,
        )
        .unwrap()
    };
    let thumbnail = stand_in("synthetic-tiles.heic", 1000).unwrap();
    assert_eq!((thumbnail.width(), thumbnail.height()), (128, 84));
    assert!(
        stand_in("synthetic-tiles.heic", 128).is_none(),
        "not smaller"
    );
    let turned = stand_in("synthetic-turned-tiles.heic", 1000)
        .unwrap()
        .to_rgb8();
    assert_eq!(turned.dimensions(), (64, 96));
    let [r, _, b] = turned.get_pixel(4, 48).0;
    assert!(r > 170 && b < 90, "the left is red: {r} {b}");
    let [r, _, b] = turned.get_pixel(60, 48).0;
    assert!(r < 90 && b > 170, "the right is blue: {r} {b}");
    assert!(
        stand_in("synthetic-gradient.heic", 1000).is_none(),
        "it carries none"
    );
    assert_eq!(said("synthetic-gradient.heic", &["thumbnail"]), "0 0 0");
}

/// A region of a HEIC in tiles, in each of the eight ways a picture is
/// turned and mirrored (EXIF's, which `heif-enc` writes as `irot` and
/// `imir`), its tiles reaching past its right and bottom as stored: cut
/// where it is shown, from the tiles it covers alone, as libheif decodes
/// and turns the whole.
#[test]
fn a_heic_region_is_cut_as_shown_however_it_is_turned() {
    taught();
    for orientation in 1..=8 {
        let name = format!("synthetic-orientation-{orientation}.heic");
        let path = fixture(&name);
        let whole = picture::decode(picture::open(&path).unwrap(), image::Limits::default())
            .unwrap()
            .to_rgb8();
        let own = whole.dimensions();
        // 132 × 76 as stored, tiles of 64: three across, two down.
        assert_eq!(
            own,
            if orientation >= 5 {
                (76, 132)
            } else {
                (132, 76)
            },
            "{name}"
        );
        for region in [
            [0, 0, 20, 20],
            [10, 30, 50, 40],
            [own.0 - 17, own.1 - 9, 17, 9],
            [0, 0, own.0, own.1],
        ] {
            let [x, y, width, height] = region;
            let numbers = region.map(|n| n.to_string());
            let mut args = vec!["region", "1000000"];
            args.extend(numbers.iter().map(String::as_str));
            let tiles: u32 = said(&name, &args)
                .split_whitespace()
                .nth(3)
                .unwrap()
                .parse()
                .unwrap();
            assert!(tiles > 0, "{name} {region:?}: decoded whole");
            let cut = picture::decode_region(
                picture::open(&path).unwrap(),
                own,
                region,
                own.0.max(own.1),
                Threads::Caller,
            )
            .unwrap()
            .to_rgb8();
            let expected = image::imageops::crop_imm(&whole, x, y, width, height).to_image();
            let (differs, of) = differing(&cut, &expected);
            assert!(
                differs * 50 < of,
                "{name} {region:?}: {differs} of {of} differ"
            );
        }
    }
}

/// The thumbnail a HEIC carries makes the grid's thumbnails of a side it
/// covers (ADR-148 amended): its whole is not decoded for them.
#[test]
fn a_heic_s_grid_thumbnail_is_made_from_the_one_it_carries() {
    use skwir_core::thumbs::ThumbnailProvider;
    taught();
    let carried = |name: &str, side: u32| {
        picture::carried(
            picture::open(&fixture(name)).unwrap(),
            |longer| longer >= side,
            Threads::Caller,
        )
        .unwrap()
    };
    // 384 × 256, carrying 128 × 84.
    let thumbnail = carried("synthetic-tiles.heic", 128).unwrap();
    assert_eq!((thumbnail.width(), thumbnail.height()), (128, 84));
    assert!(
        carried("synthetic-tiles.heic", 129).is_none(),
        "smaller than asked"
    );
    assert!(
        carried("synthetic-gradient.heic", 16).is_none(),
        "it carries none"
    );
    let turned = carried("synthetic-turned-tiles.heic", 96).unwrap();
    assert_eq!(
        (turned.width(), turned.height()),
        (64, 96),
        "turned as shown"
    );
    // The image decoder's thumbnail at the grid's smallest side, 128: the
    // one carried, to the pixel.
    let path = fixture("synthetic-tiles.heic");
    let request = skwir_core::thumbs::ThumbRequest {
        entry: skwir_core::op::EntryRef::from_path(&path).unwrap(),
        mime: Some("image/heic".into()),
        mtime: None,
        size: skwir_core::thumbs::ThumbSize::Normal,
    };
    let decoder = skwir_platform::thumb_providers::ImageDecoder;
    assert!(decoder.accepts(&request));
    let made = decoder
        .thumbnail(&request, &skwir_core::cancel::CancelToken::new())
        .unwrap()
        .unwrap();
    assert_eq!((made.width, made.height), (128, 84));
    assert!(
        *made.rgba == *thumbnail.to_rgba8().into_raw(),
        "not the carried one"
    );
}
