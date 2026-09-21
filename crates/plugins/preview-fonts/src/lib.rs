//! Font specimens rasterized on a worker, without registering fonts globally.
use bibli_core::{
    App, CancelToken,
    inspection::{Block, InspectRequest, Inspection},
    plugin::Plugin,
    preview::{PreviewProvider, PreviewRequest},
    thumbs::Image,
};
use bibli_platform::inspection::{self, limits};
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
    fn register(&self, app: &App) {
        app.previews.register(self.id(), Arc::new(Provider));
    }
}
/// Specimen canvas. Every rasterization bound derives from it: no glyph may
/// exceed the canvas and one drawing pass may not paint more than a few
/// canvases' worth of pixels, whatever the outline bounds claim.
const WIDTH: usize = 1000;
const HEIGHT: usize = 650;
const GLYPH_PIXELS: usize = WIDTH * HEIGHT;
const PASS_PIXELS: usize = 16 * GLYPH_PIXELS;
/// Outline bounds are legitimate up to a few ems; beyond that a font is
/// malformed and would only produce oversized glyphs.
const BOUNDS_EMS: f32 = 8.;
struct Provider;
impl PreviewProvider for Provider {
    fn id(&self) -> &'static str {
        "preview-fonts"
    }
    fn accepts(&self, req: &PreviewRequest) -> bool {
        matches!(
            inspection::extension(&req.entry.path).as_str(),
            "ttf" | "otf" | "ttc"
        )
    }
    fn inspect(
        &self,
        req: &PreviewRequest,
        view: &InspectRequest,
        cancel: &CancelToken,
    ) -> anyhow::Result<Option<Inspection>> {
        let bytes =
            inspection::read_bounded(&req.entry.resolve()?, limits::BINARY_INPUT, "Font", cancel)?;
        specimen(bytes, view, cancel).map(Some)
    }
}
fn specimen(
    bytes: Vec<u8>,
    view: &InspectRequest,
    cancel: &CancelToken,
) -> anyhow::Result<Inspection> {
    let face = ttf_parser::Face::parse(&bytes, 0)?;
    check_metrics(&face)?;
    // A name has one record per platform and language, and only the Unicode
    // ones decode: take the first that does, not the first with the id.
    let named = |id| {
        face.names()
            .into_iter()
            .filter(|n| n.name_id == id)
            .find_map(|n| n.to_string())
    };
    let name = named(ttf_parser::name_id::FULL_NAME).unwrap_or_else(|| "Font specimen".into());
    let glyphs = face.number_of_glyphs();
    // What the name table says, for the panel's Details.
    let mut details: Vec<_> = [
        ("Family", named(ttf_parser::name_id::FAMILY)),
        ("Style", named(ttf_parser::name_id::SUBFAMILY)),
        ("Version", named(ttf_parser::name_id::VERSION)),
        ("Designer", named(ttf_parser::name_id::DESIGNER)),
        ("License", named(ttf_parser::name_id::LICENSE)),
    ]
    .into_iter()
    .filter_map(|(label, value)| inspection::detail(label, value?))
    .collect();
    details.extend(inspection::detail("Glyphs", glyphs.to_string()));
    details.extend(inspection::detail(
        "Outlines",
        if face.tables().glyf.is_some() {
            "TrueType"
        } else {
            "PostScript (CFF)"
        },
    ));
    details.extend(
        face.is_variable()
            .then(|| inspection::detail("Variable", "Yes"))
            .flatten(),
    );
    let font = fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default())
        .map_err(anyhow::Error::msg)?;
    let mut chars: Vec<_> = font
        .chars()
        .keys()
        .copied()
        .filter(|c| !c.is_control())
        .collect();
    chars.sort_unstable();
    let sheet = limits::SHEET_GLYPHS;
    let offset = view.offset.min(chars.len().saturating_sub(1) as u64) as usize;
    let sample = view
        .query
        .as_deref()
        .unwrap_or("The quick brown fox jumps over the lazy dog. 0123456789");
    let sample: String = sample.chars().take(limits::SPECIMEN_CHARS).collect();
    let mut rgba = vec![255; WIDTH * HEIGHT * 4];
    let mut skipped = 0;
    for (y, size) in [(16., 20.), (86., 32.), (196., 48.)] {
        skipped += draw(&font, &sample, size, y, &mut rgba, cancel)?;
    }
    let glyph_text: String = chars
        .iter()
        .skip(offset)
        .take(sheet)
        .flat_map(|c| [*c, ' '])
        .collect();
    skipped += draw(&font, &glyph_text, 24., 360., &mut rgba, cancel)?;
    let mut notice =
        String::from("Search sets the specimen text · font collections show their first face");
    if skipped > 0 {
        notice = format!("{skipped} oversized glyphs were not drawn · {notice}");
    }
    Ok(Inspection {
        details,
        kind: bibli_core::inspection::InspectionKind::Font,
        title: format!("Font · {name}"),
        image: Some(Image {
            width: WIDTH as u32,
            height: HEIGHT as u32,
            rgba: rgba.into(),
        }),
        blocks: vec![Block {
            text: format!(
                "{glyphs} glyphs · {} mapped characters\nSpecimen: 20, 32 and 48 px\nCharacter sheet starts at {offset}",
                chars.len()
            ),
            ..Block::default()
        }],
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
fn check_metrics(face: &ttf_parser::Face<'_>) -> anyhow::Result<()> {
    let upem = face.units_per_em();
    // The OpenType range; fontdue scales every outline by `px / units_per_em`.
    anyhow::ensure!(
        (16..=16384).contains(&upem),
        "Font declares {upem} units per em (expected 16–16384)"
    );
    let bounds = face.global_bounding_box();
    let limit = f32::from(upem) * BOUNDS_EMS;
    anyhow::ensure!(
        f32::from(bounds.width()) <= limit
            && f32::from(bounds.height()) <= limit
            && bounds.x_min <= bounds.x_max
            && bounds.y_min <= bounds.y_max,
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
/// Draw `text` at `y`; returns how many glyphs were skipped for being too large.
fn draw(
    font: &fontdue::Font,
    text: &str,
    size: f32,
    y: f32,
    pixels: &mut [u8],
    cancel: &CancelToken,
) -> anyhow::Result<usize> {
    use fontdue::layout::{CoordinateSystem, Layout, LayoutSettings, TextStyle};
    let mut layout = Layout::new(CoordinateSystem::PositiveYDown);
    layout.reset(&LayoutSettings {
        x: 16.,
        y,
        max_width: Some(WIDTH as f32 - 32.),
        ..LayoutSettings::default()
    });
    layout.append(&[font], &TextStyle::new(text, size, 0));
    let mut skipped = 0;
    let mut budget = PASS_PIXELS;
    for glyph in layout.glyphs() {
        inspection::check(cancel)?;
        // Layout metrics come from the same outline bounds rasterization uses;
        // checking them first keeps fontdue from allocating width × height floats.
        let metrics = font.metrics_indexed(glyph.key.glyph_index, glyph.key.px);
        let area = metrics.width.saturating_mul(metrics.height);
        if !glyph_fits(metrics.width, metrics.height) || area > budget {
            skipped += 1;
            continue;
        }
        budget -= area;
        let (metrics, bitmap) = font.rasterize_config(glyph.key);
        for row in 0..metrics.height {
            for col in 0..metrics.width {
                let x = glyph.x as i32 + i32::try_from(col)?;
                let y = glyph.y as i32 + i32::try_from(row)?;
                if let (Ok(x), Ok(y)) = (usize::try_from(x), usize::try_from(y))
                    && x < WIDTH
                    && y < HEIGHT
                {
                    let start = (y * WIDTH + x) * 4;
                    let gray = 255 - bitmap[row * metrics.width + col];
                    pixels[start..start + 3].fill(gray);
                }
            }
        }
    }
    Ok(skipped)
}

#[cfg(test)]
mod tests {
    use super::*;

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
        specimen(
            font,
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
        assert_eq!(data.kind, bibli_core::inspection::InspectionKind::Font);
        assert!(data.title.starts_with("Font · "));
        assert!(data.plain_text().contains("2 glyphs · 1 mapped characters"));
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
    fn malformed_metrics_are_rejected_before_layout() {
        let error = render(synthetic_ttf(16, 16383, 16383), None).unwrap_err();
        assert!(error.to_string().contains("outline bounds"), "{error}");
        // ttf-parser already rejects the head table for 8 units per em.
        assert!(render(synthetic_ttf(8, 4, 4), None).is_err());
        assert!(render(b"not a font".to_vec(), None).is_err());
    }

    #[test]
    fn glyph_cap_follows_the_canvas() {
        assert!(glyph_fits(WIDTH, HEIGHT));
        assert!(!glyph_fits(WIDTH + 1, HEIGHT));
        assert!(!glyph_fits(usize::MAX, 2));
        assert!(glyph_fits(0, 0));
    }
}
