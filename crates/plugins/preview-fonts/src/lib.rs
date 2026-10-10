//! Font specimens rasterized on a worker, without registering fonts globally.
use skwir_core::{
    App, CancelToken,
    inspection::{InspectRequest, Inspection},
    plugin::Plugin,
    preview::{PreviewProvider, PreviewRequest},
    thumbs::Image,
};
use skwir_platform::inspection::{self, limits};
use std::sync::Arc;
pub struct PreviewPlugin;
impl Plugin for PreviewPlugin {
    fn id(&self) -> &'static str {
        "preview-fonts"
    }
    fn title(&self) -> &'static str {
        "Preview: fonts"
    }
    fn description(&self) -> &'static str {
        "Show a specimen of font files without installing them"
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
/// Specimen canvas. Every rasterization bound derives from it: no glyph may
/// exceed the canvas and one drawing pass may not paint more than a few
/// canvases' worth of pixels, whatever the outline bounds claim.
// The sheet is drawn for the panel, which shows it at about half this width:
// a narrower sheet with larger type reads at the sizes it names.
const WIDTH: usize = 640;
const HEIGHT: usize = 900;
/// Sizes of the specimen lines, then of the character sheet.
const SIZES: [f32; 3] = [22., 34., 52.];
const SHEET_SIZE: f32 = 28.;
const GLYPH_PIXELS: usize = WIDTH * HEIGHT;
const PASS_PIXELS: usize = 16 * GLYPH_PIXELS;
/// Outline bounds are legitimate up to a few ems; beyond that a font is
/// malformed and would only produce oversized glyphs.
const BOUNDS_EMS: f32 = 8.;
/// What the font parser reads, by type: an `.otf` name is also an
/// `OpenDocument` formula template's, which the request's type, read from
/// the bytes, tells apart.
static FONTS: skwir_platform::mime::Types = skwir_platform::mime::Types::new(&[
    "font/ttf",
    "font/otf",
    "font/collection",
    "font/woff",
    "font/woff2",
]);

struct Provider;
impl PreviewProvider for Provider {
    fn id(&self) -> &'static str {
        "preview-fonts"
    }
    fn accepts(&self, req: &PreviewRequest) -> bool {
        FONTS.holds(&req.entry.path, req.mime.as_deref())
    }
    fn inspect(
        &self,
        req: &PreviewRequest,
        view: &InspectRequest,
        cancel: &CancelToken,
    ) -> anyhow::Result<Option<Inspection>> {
        let bytes =
            inspection::read_bounded(&req.entry.resolve()?, limits::BINARY_INPUT, "Font", cancel)?;
        let (font, packed) = unpacked(bytes)?;
        specimen(&font, packed, view, cancel).map(Some)
    }
}
/// A web font unpacked into the font it carries, and what it was packed
/// as, for Details; any other font as it is. Bounded like every input:
/// WOFF is inflated table by table within `limits::BINARY_INPUT` (a table
/// can declare any size), WOFF2 within its decoder's own bound (at most
/// 100 times its size, and 128 MB).
fn unpacked(bytes: Vec<u8>) -> anyhow::Result<(Vec<u8>, Option<&'static str>)> {
    let damaged = || anyhow::anyhow!("The web font is damaged or uses an unknown feature");
    match bytes.get(..4) {
        Some(b"wOFF") => {
            let mut left = limits::BINARY_INPUT;
            let mut over = false;
            let mut inflate = |data: &[u8], size: usize| {
                if size > left {
                    over = true;
                    return Err("over the bound".into());
                }
                left -= size;
                let mut table = Vec::with_capacity(size);
                flate2::Decompress::new(true).decompress_vec(
                    data,
                    &mut table,
                    flate2::FlushDecompress::Finish,
                )?;
                Ok(table)
            };
            let font = wuff::decompress_woff1_with_custom_z(&bytes, &mut inflate);
            anyhow::ensure!(
                !over,
                "Font exceeds {} preview limit",
                limits::describe(limits::BINARY_INPUT)
            );
            Ok((font.map_err(|_| damaged())?, Some("WOFF")))
        }
        Some(b"wOF2") => Ok((
            wuff::decompress_woff2(&bytes).map_err(|_| damaged())?,
            Some("WOFF2"),
        )),
        _ => Ok((bytes, None)),
    }
}
fn specimen(
    bytes: &[u8],
    packed: Option<&str>,
    view: &InspectRequest,
    cancel: &CancelToken,
) -> anyhow::Result<Inspection> {
    use skrifa::MetadataProvider;
    use skrifa::raw::TableProvider;
    use skrifa::string::StringId;
    let font = skrifa::FontRef::from_index(bytes, 0)
        .map_err(|_| anyhow::anyhow!("This font cannot be read"))?;
    check_metrics(&font)?;
    // A name has one record per platform and language: the English one,
    // else the first that decodes.
    let named = |id| {
        font.localized_strings(id)
            .english_or_first()
            .map(|name| name.to_string())
            .filter(|name| !name.trim().is_empty())
    };
    let name = named(StringId::FULL_NAME).unwrap_or_else(|| "Font specimen".into());
    let glyphs = font.maxp().map_or(0, |maxp| maxp.num_glyphs());
    // What the name table says, for the panel's Details, then what the
    // tables hold.
    let has = |tag: &[u8; 4]| font.table_data(skrifa::Tag::new(tag)).is_some();
    let outlines = if has(b"glyf") {
        Some("TrueType")
    } else if has(b"CFF ") || has(b"CFF2") {
        Some("PostScript (CFF)")
    } else if has(b"sbix") || has(b"CBDT") || has(b"EBDT") || has(b"bdat") {
        Some("Bitmaps only")
    } else {
        None
    };
    let mut details = inspection::details([
        ("Family", named(StringId::FAMILY_NAME)),
        ("Style", named(StringId::SUBFAMILY_NAME)),
        ("Version", named(StringId::VERSION_STRING)),
        ("Designer", named(StringId::DESIGNER)),
        ("License", named(StringId::LICENSE_DESCRIPTION)),
        ("Glyphs", inspection::count(usize::from(glyphs))),
        ("Outlines", outlines.map(str::to_owned)),
        (
            "Variable",
            (!font.axes().is_empty()).then(|| "Yes".to_owned()),
        ),
        ("Packed as", packed.map(str::to_owned)),
    ]);
    // The characters the font maps to a glyph of its own.
    let mut chars: Vec<char> = font
        .charmap()
        .mappings()
        .filter(|(_, glyph)| *glyph != skrifa::GlyphId::NOTDEF)
        .filter_map(|(code, _)| char::from_u32(code))
        .filter(|c| !c.is_control())
        .collect();
    chars.sort_unstable();
    chars.dedup();
    details.extend(inspection::detail("Characters", chars.len().to_string()));
    let sheet = limits::SHEET_GLYPHS;
    let offset = view.offset.min(chars.len().saturating_sub(1) as u64) as usize;
    let sample = view
        .query
        .as_deref()
        .unwrap_or("The quick brown fox jumps over the lazy dog. 0123456789");
    let sample: String = sample.chars().take(limits::SPECIMEN_CHARS).collect();
    let mut rgba = vec![255; WIDTH * HEIGHT * 4];
    let mut skipped = 0;
    // Each block starts where the one before ended: a long specimen text
    // pushes the rest down instead of being drawn over.
    let mut y = 16.;
    for size in SIZES {
        let (missed, bottom) = draw(&font, &sample, size, y, &mut rgba, cancel)?;
        skipped += missed;
        y = bottom + size * 0.6;
    }
    let glyph_text: String = chars
        .iter()
        .skip(offset)
        .take(sheet)
        .flat_map(|c| [*c, ' '])
        .collect();
    skipped += draw(&font, &glyph_text, SHEET_SIZE, y + 8., &mut rgba, cancel)?.0;
    let mut notice =
        String::from("Search sets the specimen text · font collections show their first face");
    if skipped > 0 {
        notice = format!("{skipped} oversized glyphs were not drawn · {notice}");
    }
    Ok(Inspection {
        details,
        kind: skwir_core::inspection::InspectionKind::Font,
        title: format!("Font · {name}"),
        image: Some(Image {
            width: WIDTH as u32,
            height: HEIGHT as u32,
            rgba: rgba.into(),
        }),
        previous: (offset > 0).then(|| offset.saturating_sub(sheet) as u64),
        next: (offset + sheet < chars.len()).then_some((offset + sheet) as u64),
        position: offset as u64,
        notice,
        ..Inspection::default()
    })
}
/// Reject fonts whose declared scale or bounds cannot produce sane glyphs
/// before any layout happens. Per-glyph outlines are still checked when drawn,
/// because the head table's bounds are declared, not computed.
fn check_metrics(font: &skrifa::FontRef<'_>) -> anyhow::Result<()> {
    use skrifa::raw::TableProvider;
    let head = font
        .head()
        .map_err(|_| anyhow::anyhow!("This font has no readable header"))?;
    let upem = head.units_per_em();
    // The OpenType range; outlines are scaled by `px / units_per_em`.
    anyhow::ensure!(
        (16..=16384).contains(&upem),
        "Font declares {upem} units per em (expected 16–16384)"
    );
    let (x_min, y_min, x_max, y_max) = (
        i32::from(head.x_min()),
        i32::from(head.y_min()),
        i32::from(head.x_max()),
        i32::from(head.y_max()),
    );
    let limit = f32::from(upem) * BOUNDS_EMS;
    anyhow::ensure!(
        (x_max - x_min) as f32 <= limit
            && (y_max - y_min) as f32 <= limit
            && x_min <= x_max
            && y_min <= y_max,
        "Font declares outline bounds beyond {BOUNDS_EMS} ems"
    );
    Ok(())
}
/// Whether a glyph bitmap of `width` × `height` pixels may be rasterized.
fn glyph_fits(width: usize, height: usize) -> bool {
    width
        .checked_mul(height)
        .is_some_and(|px| px <= GLYPH_PIXELS)
}
/// A glyph's outline as `zeno` draws it: the font's units scaled by the
/// outline itself, moved to where the glyph stands, `y` turned down.
struct Pen {
    commands: Vec<zeno::Command>,
    origin: (f32, f32),
}
impl Pen {
    fn at(&self, x: f32, y: f32) -> zeno::Vector {
        zeno::Vector::new(self.origin.0 + x, self.origin.1 - y)
    }
    /// The box every point lies in, control points included: it holds the
    /// glyph, whatever its curves. None for an empty outline or one with a
    /// point that is no number.
    fn bounds(&self) -> Option<(f32, f32, f32, f32)> {
        use zeno::Command as C;
        let mut points = self.commands.iter().flat_map(|c| match *c {
            C::MoveTo(p) | C::LineTo(p) => vec![p],
            C::QuadTo(a, p) => vec![a, p],
            C::CurveTo(a, b, p) => vec![a, b, p],
            C::Close => Vec::new(),
        });
        let finite = |p: zeno::Vector| p.x.is_finite() && p.y.is_finite();
        let first = points.next().filter(|p| finite(*p))?;
        points.try_fold((first.x, first.y, first.x, first.y), |(l, t, r, b), p| {
            finite(p).then(|| (l.min(p.x), t.min(p.y), r.max(p.x), b.max(p.y)))
        })
    }
}
impl skrifa::outline::OutlinePen for Pen {
    fn move_to(&mut self, x: f32, y: f32) {
        let p = self.at(x, y);
        self.commands.push(zeno::Command::MoveTo(p));
    }
    fn line_to(&mut self, x: f32, y: f32) {
        let p = self.at(x, y);
        self.commands.push(zeno::Command::LineTo(p));
    }
    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        let (c, p) = (self.at(cx0, cy0), self.at(x, y));
        self.commands.push(zeno::Command::QuadTo(c, p));
    }
    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        let (a, b, p) = (self.at(cx0, cy0), self.at(cx1, cy1), self.at(x, y));
        self.commands.push(zeno::Command::CurveTo(a, b, p));
    }
    fn close(&mut self) {
        self.commands.push(zeno::Command::Close);
    }
}
/// Draw `text` at `y`, its lines broken between words to fit the canvas;
/// returns how many glyphs were skipped for being too large, and where the
/// text ends.
fn draw(
    font: &skrifa::FontRef<'_>,
    text: &str,
    size: f32,
    y: f32,
    pixels: &mut [u8],
    cancel: &CancelToken,
) -> anyhow::Result<(usize, f32)> {
    use skrifa::MetadataProvider;
    use skrifa::instance::{LocationRef, Size};
    let (scale, location) = (Size::new(size), LocationRef::default());
    let charmap = font.charmap();
    let outlines = font.outline_glyphs();
    let glyph_metrics = font.glyph_metrics(scale, location);
    let metrics = font.metrics(scale, location);
    let line = (metrics.ascent - metrics.descent + metrics.leading).max(size);
    let advance = |c: char| {
        let glyph = charmap.map(c).unwrap_or(skrifa::GlyphId::NOTDEF);
        (
            glyph,
            glyph_metrics
                .advance_width(glyph)
                .unwrap_or(size * 0.5)
                .max(0.),
        )
    };
    let (left, right) = (16., WIDTH as f32 - 16.);
    let (mut x, mut baseline) = (left, y + metrics.ascent);
    let mut skipped = 0;
    let mut budget = PASS_PIXELS;
    for word in text.split_inclusive(' ') {
        let width: f32 = word.trim_end().chars().map(|c| advance(c).1).sum();
        if x > left && x + width > right {
            x = left;
            baseline += line;
        }
        for c in word.chars() {
            inspection::check(cancel)?;
            let (glyph, step) = advance(c);
            if x > left && x + step > right {
                x = left;
                baseline += line;
            }
            let mut pen = Pen {
                commands: Vec::new(),
                origin: (x, baseline),
            };
            x += step;
            let Some(outline) = outlines.get(glyph) else {
                continue;
            };
            let settings = skrifa::outline::DrawSettings::unhinted(scale, location);
            // read-fonts (0.41 to 0.45 at least) indexes past a simple
            // glyph's flags when it declares no points but holds bytes after
            // its instructions: that glyph is left out, as one it refuses.
            let drawn = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                outline.draw(settings, &mut pen)
            }));
            if !matches!(drawn, Ok(Ok(_))) {
                continue;
            }
            let Some((l, t, r, b)) = pen.bounds() else {
                skipped += usize::from(!pen.commands.is_empty());
                continue;
            };
            // Measured before anything is allocated: a malformed outline
            // asks for no more pixels than the canvas holds.
            let (w, h) = ((r - l).ceil() as usize + 1, (b - t).ceil() as usize + 1);
            let area = w.saturating_mul(h);
            if !glyph_fits(w, h) || area > budget {
                skipped += 1;
                continue;
            }
            budget -= area;
            let (mask, placement) = zeno::Mask::new(&pen.commands).render();
            let width = placement.width as usize;
            for (row, coverage) in mask.chunks(width.max(1)).enumerate() {
                for (col, alpha) in coverage.iter().enumerate() {
                    let px = placement.left + i32::try_from(col)?;
                    let py = placement.top + i32::try_from(row)?;
                    if let (Ok(px), Ok(py)) = (usize::try_from(px), usize::try_from(py))
                        && px < WIDTH
                        && py < HEIGHT
                    {
                        let start = (py * WIDTH + px) * 4;
                        let gray = 255 - alpha;
                        for channel in &mut pixels[start..start + 3] {
                            *channel = (*channel).min(gray);
                        }
                    }
                }
            }
        }
    }
    Ok((skipped, baseline - metrics.descent))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Types from the free desktops' database (step E, ADR-035).
    #[cfg(target_os = "linux")]
    #[test]
    fn fonts_are_known_by_type() {
        let accepts = |name: &str, mime: Option<&str>| {
            Provider.accepts(&PreviewRequest {
                entry: skwir_core::op::EntryRef::new(skwir_core::FileId { dev: 1, ino: 1 }, name),
                mime: mime.map(str::to_owned),
                page: 1,
                seconds: None,
                max_side: 64,
                text_size: None,
            })
        };
        assert!(accepts("a.ttf", None));
        assert!(accepts("a.woff2", None));
        // `.otf` names a font and a formula template: the bytes say which.
        assert!(accepts("a.otf", Some("font/otf")));
        assert!(!accepts(
            "a.otf",
            Some("application/vnd.oasis.opendocument.formula-template")
        ));
        assert!(!accepts("a.pcf", None));
    }

    /// A minimal TrueType font with two glyphs: `.notdef` and `A`, a square
    /// spanning ±`half` units. `declared` is the head table's bounding box
    /// half-extent, which a malformed font can state independently of its outlines.
    fn synthetic_ttf(units_per_em: u16, half: i16, declared: i16) -> Vec<u8> {
        fn be16(out: &mut Vec<u8>, v: i32) {
            out.extend_from_slice(&(v as u16).to_be_bytes());
        }
        fn be32(out: &mut Vec<u8>, v: u32) {
            out.extend_from_slice(&v.to_be_bytes());
        }
        let h = i32::from(half);
        let d = i32::from(declared);
        let mut head = Vec::new();
        be32(&mut head, 0x0001_0000);
        be32(&mut head, 0x0001_0000);
        be32(&mut head, 0);
        be32(&mut head, 0x5F0F_3CF5);
        be16(&mut head, 0);
        be16(&mut head, i32::from(units_per_em));
        head.extend_from_slice(&[0; 16]);
        for v in [-d, -d, d, d] {
            be16(&mut head, v);
        }
        for v in [0, 8, 2, 0, 0] {
            be16(&mut head, v);
        }
        assert_eq!(head.len(), 54);
        let mut hhea = Vec::new();
        be32(&mut hhea, 0x0001_0000);
        for v in [h, -h, 0, 2 * h, -h, 0, h, 1, 0, 0, 0, 0, 0, 0, 0, 2] {
            be16(&mut hhea, v);
        }
        assert_eq!(hhea.len(), 36);
        let mut maxp = Vec::new();
        be32(&mut maxp, 0x0001_0000);
        for v in [2, 4, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0] {
            be16(&mut maxp, v);
        }
        assert_eq!(maxp.len(), 32);
        let mut hmtx = Vec::new();
        for v in [2 * h, 0, 2 * h, -h] {
            be16(&mut hmtx, v);
        }
        let mut cmap = Vec::new();
        for v in [0, 1, 3, 1] {
            be16(&mut cmap, v);
        }
        be32(&mut cmap, 12);
        for v in [
            4,
            32,
            0,
            4,
            4,
            1,
            0,
            0x41,
            0xFFFF,
            0,
            0x41,
            0xFFFF,
            1 - 0x41,
            1,
            0,
            0,
        ] {
            be16(&mut cmap, v);
        }
        assert_eq!(cmap.len(), 44);
        let mut glyf = Vec::new();
        for v in [1, -h, -h, h, h, 3, 0] {
            be16(&mut glyf, v);
        }
        glyf.extend_from_slice(&[1, 1, 1, 1]);
        for v in [-h, 2 * h, 0, -2 * h, -h, 0, 2 * h, 0] {
            be16(&mut glyf, v);
        }
        assert_eq!(glyf.len(), 34);
        let mut loca = Vec::new();
        for v in [0, 0, 17] {
            be16(&mut loca, v);
        }
        let tables = [
            (*b"cmap", cmap),
            (*b"glyf", glyf),
            (*b"head", head),
            (*b"hhea", hhea),
            (*b"hmtx", hmtx),
            (*b"loca", loca),
            (*b"maxp", maxp),
        ];
        let mut font = Vec::new();
        be32(&mut font, 0x0001_0000);
        for v in [7, 64, 2, 48] {
            be16(&mut font, v);
        }
        let mut offset = 12 + 16 * tables.len();
        let mut body = Vec::new();
        for (tag, data) in &tables {
            font.extend_from_slice(tag);
            be32(&mut font, 0);
            be32(&mut font, offset as u32);
            be32(&mut font, data.len() as u32);
            body.extend_from_slice(data);
            while !body.len().is_multiple_of(4) {
                body.push(0);
            }
            offset = 12 + 16 * tables.len() + body.len();
        }
        font.extend(body);
        font
    }

    fn render(font: Vec<u8>, query: Option<&str>) -> anyhow::Result<Inspection> {
        let (font, packed) = unpacked(font)?;
        specimen(
            &font,
            packed,
            &InspectRequest {
                query: query.map(str::to_owned),
                ..InspectRequest::default()
            },
            &CancelToken::new(),
        )
    }

    #[test]
    fn synthetic_font_renders_a_specimen() {
        let data = render(synthetic_ttf(1000, 400, 400), Some("AAA")).unwrap();
        assert_eq!(data.kind, skwir_core::inspection::InspectionKind::Font);
        assert!(data.title.starts_with("Font · "));
        // What the font is made of is Details, never a sentence in the sheet.
        assert!(data.blocks.is_empty());
        let fact = |label: &str| {
            data.details
                .iter()
                .find(|d| d.label == label)
                .map(|d| d.value.as_str())
        };
        assert_eq!(fact("Glyphs"), Some("2"));
        assert_eq!(fact("Characters"), Some("1"));
        assert_eq!(fact("Outlines"), Some("TrueType"));
        let image = data.image.unwrap();
        assert_eq!((image.width, image.height), (WIDTH as u32, HEIGHT as u32));
        let dark = image.rgba.chunks(4).filter(|px| px[0] < 128).count();
        assert!(dark > 1000, "the square glyph paints pixels, got {dark}");
        assert!(!data.notice.contains("oversized"));
        assert_eq!(data.next, None);
    }

    #[test]
    fn oversized_outlines_are_skipped_instead_of_allocated() {
        // 16 units per em with a ±16383-unit square: 48 px would need a bitmap
        // of roughly 98k × 98k pixels. The head table lies about the bounds.
        let data = render(synthetic_ttf(16, 16383, 16), Some("AAA")).unwrap();
        // Three specimen sizes of "AAA" plus the character sheet's single glyph.
        assert!(
            data.notice.starts_with("10 oversized glyphs"),
            "{}",
            data.notice
        );
        let image = data.image.unwrap();
        assert!(image.rgba.chunks(4).all(|px| px[0] == 255));
    }

    #[test]
    fn a_glyph_the_font_reader_trips_on_is_left_out() {
        // `A` declared with no contours: its end point becomes an
        // instruction length, and what follows reads as flags for points
        // it does not have.
        let mut font = synthetic_ttf(1000, 400, 400);
        let entry = font.windows(4).position(|tag| tag == b"glyf").unwrap();
        let at = u32::from_be_bytes(font[entry + 8..entry + 12].try_into().unwrap()) as usize;
        font[at..at + 2].copy_from_slice(&0u16.to_be_bytes());
        let data = render(font, Some("AAA")).unwrap();
        let image = data.image.unwrap();
        assert!(image.rgba.chunks(4).all(|px| px[0] == 255));
    }

    proptest::proptest! {
        /// The synthetic font with bytes changed, put in or cut off, and
        /// bytes at random: a specimen or the reason there is none, never a
        /// panic.
        #[test]
        fn any_font_is_drawn_or_refused(
            input in proptest::prop_oneof![
                skwir_core::testing::mutated(synthetic_ttf(1000, 400, 400)),
                skwir_core::testing::bytes(512),
            ],
        ) {
            let _ = render(input.clone(), None);
            let _ = render(input, Some("AAA A"));
        }
    }

    #[test]
    fn malformed_metrics_are_rejected_before_layout() {
        let error = render(synthetic_ttf(16, 16383, 16383), None).unwrap_err();
        assert!(error.to_string().contains("outline bounds"), "{error}");
        // 8 units per em is outside OpenType's range.
        let error = render(synthetic_ttf(8, 4, 4), None).unwrap_err();
        assert!(error.to_string().contains("units per em"), "{error}");
        assert!(render(b"not a font".to_vec(), None).is_err());
    }

    #[test]
    fn glyph_cap_follows_the_canvas() {
        assert!(glyph_fits(WIDTH, HEIGHT));
        assert!(!glyph_fits(WIDTH + 1, HEIGHT));
        assert!(!glyph_fits(usize::MAX, 2));
        assert!(glyph_fits(0, 0));
    }

    #[test]
    fn an_outline_with_a_point_that_is_no_number_has_no_bounds() {
        let pen = |points: &[(f32, f32)]| Pen {
            commands: points
                .iter()
                .map(|&(x, y)| zeno::Command::LineTo(zeno::Vector::new(x, y)))
                .collect(),
            origin: (0., 0.),
        };
        assert_eq!(
            pen(&[(1., 2.), (3., -4.)]).bounds(),
            Some((1., -4., 3., 2.))
        );
        assert_eq!(pen(&[]).bounds(), None);
        // `min` and `max` would pass over a NaN and leave a finite box.
        for bad in [f32::NAN, f32::INFINITY] {
            assert_eq!(pen(&[(bad, 0.), (1., 1.)]).bounds(), None);
            assert_eq!(pen(&[(0., 0.), (1., bad)]).bounds(), None);
        }
    }

    /// `font` packed as WOFF, each table compressed; a table can instead
    /// declare `declared` as its unpacked length.
    fn woff(font: &[u8], declared: Option<u32>) -> Vec<u8> {
        use std::io::Write;
        let be16 = |at: usize| usize::from(u16::from_be_bytes([font[at], font[at + 1]]));
        let be32 = |at: usize| u32::from_be_bytes(font[at..at + 4].try_into().unwrap());
        let count = be16(4);
        let mut directory = Vec::new();
        let mut data = Vec::new();
        let start = 44 + 20 * count;
        for entry in 0..count {
            let at = 12 + 16 * entry;
            let (offset, length) = (be32(at + 8) as usize, be32(at + 12));
            let table = &font[offset..offset + length as usize];
            let mut encoder =
                flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
            encoder.write_all(table).unwrap();
            let packed = encoder.finish().unwrap();
            let packed = if packed.len() < table.len() {
                packed
            } else {
                table.to_vec()
            };
            directory.extend_from_slice(&font[at..at + 4]);
            directory.extend_from_slice(&((start + data.len()) as u32).to_be_bytes());
            directory.extend_from_slice(&(packed.len() as u32).to_be_bytes());
            directory.extend_from_slice(&declared.unwrap_or(length).to_be_bytes());
            directory.extend_from_slice(&be32(at + 4).to_be_bytes());
            data.extend_from_slice(&packed);
            while !data.len().is_multiple_of(4) {
                data.push(0);
            }
        }
        let mut out = b"wOFF".to_vec();
        out.extend_from_slice(&font[..4]);
        out.extend_from_slice(&((start + data.len()) as u32).to_be_bytes());
        out.extend_from_slice(&(count as u16).to_be_bytes());
        out.extend_from_slice(&[0, 0]);
        out.extend_from_slice(&(font.len() as u32).to_be_bytes());
        out.extend_from_slice(&[0, 1, 0, 0]);
        out.extend_from_slice(&[0; 20]);
        out.extend(directory);
        out.extend(data);
        out
    }

    fn packed_as(data: &Inspection) -> Option<&str> {
        data.details
            .iter()
            .find(|d| d.label == "Packed as")
            .map(|d| d.value.as_str())
    }

    #[test]
    fn web_fonts_are_unpacked_into_the_font_they_carry() {
        let plain = render(synthetic_ttf(1000, 400, 400), Some("A")).unwrap();
        assert_eq!(packed_as(&plain), None);
        let woff = render(woff(&synthetic_ttf(1000, 400, 400), None), Some("A")).unwrap();
        assert_eq!(packed_as(&woff), Some("WOFF"));
        // The same font packed as WOFF2 by fontTools, its outlines transformed.
        let woff2 = render(
            include_bytes!("testdata/synthetic.woff2").to_vec(),
            Some("A"),
        )
        .unwrap();
        assert_eq!(packed_as(&woff2), Some("WOFF2"));
        for data in [&woff, &woff2] {
            let unpacked: Vec<_> = data
                .details
                .iter()
                .filter(|d| d.label != "Packed as")
                .cloned()
                .collect();
            assert_eq!(unpacked, plain.details);
            assert_eq!(
                data.image.as_ref().unwrap().rgba,
                plain.image.as_ref().unwrap().rgba
            );
        }
    }

    #[test]
    fn a_web_font_that_lies_about_its_size_is_refused_before_allocating() {
        let error = render(woff(&synthetic_ttf(1000, 400, 400), Some(u32::MAX)), None).unwrap_err();
        assert!(error.to_string().contains("exceeds"), "{error}");
        let error = render(b"wOF2 and nothing else".to_vec(), None).unwrap_err();
        assert!(error.to_string().contains("damaged"), "{error}");
    }
}
