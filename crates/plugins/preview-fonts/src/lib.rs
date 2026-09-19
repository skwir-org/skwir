//! Font specimens rasterized on a worker, without registering fonts globally.
use bibli_core::{
    App, CancelToken,
    inspection::{Block, InspectRequest, Inspection},
    plugin::Plugin,
    preview::{PreviewProvider, PreviewRequest},
    thumbs::Image,
};
use bibli_platform::inspection;
use std::sync::Arc;
pub struct PreviewPlugin;
impl Plugin for PreviewPlugin {
    fn id(&self) -> &'static str {
        "preview-fonts"
    }
    fn title(&self) -> &'static str {
        "Preview: fonts"
    }
    fn register(&self, app: &App) {
        app.previews.register(self.id(), Arc::new(Provider));
    }
}
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
        let bytes = inspection::read(
            &req.entry.resolve()?,
            0,
            inspection::IMAGE_LIMIT + 1,
            cancel,
        )?;
        anyhow::ensure!(
            bytes.len() <= inspection::IMAGE_LIMIT,
            "Font exceeds 16 MiB preview limit"
        );
        let face = ttf_parser::Face::parse(&bytes, 0)?;
        let name = face
            .names()
            .into_iter()
            .find(|n| n.name_id == ttf_parser::name_id::FULL_NAME)
            .and_then(|n| n.to_string())
            .unwrap_or_else(|| "Font specimen".into());
        let glyphs = face.number_of_glyphs();
        let font = fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default())
            .map_err(anyhow::Error::msg)?;
        let mut chars: Vec<_> = font
            .chars()
            .keys()
            .copied()
            .filter(|c| !c.is_control())
            .collect();
        chars.sort_unstable();
        let offset = view.offset.min(chars.len().saturating_sub(1) as u64) as usize;
        let sample = view
            .query
            .as_deref()
            .unwrap_or("The quick brown fox jumps over the lazy dog. 0123456789");
        let sample: String = sample.chars().take(200).collect();
        let mut rgba = vec![255; 1000 * 650 * 4];
        for (y, size) in [(16., 20.), (86., 32.), (196., 48.)] {
            draw(&font, &sample, size, y, &mut rgba, cancel)?;
        }
        let glyph_text: String = chars
            .iter()
            .skip(offset)
            .take(192)
            .flat_map(|c| [*c, ' '])
            .collect();
        draw(&font, &glyph_text, 24., 360., &mut rgba, cancel)?;
        Ok(Some(Inspection {
            kind: bibli_core::inspection::InspectionKind::Font,
            title: format!("Font · {name}"),
            image: Some(Image {
                width: 1000,
                height: 650,
                rgba: rgba.into(),
            }),
            blocks: vec![Block {
                text: format!(
                    "{glyphs} glyphs · {} mapped characters\nSpecimen: 20, 32 and 48 px\nCharacter sheet starts at {offset}",
                    chars.len()
                ),
                ..Block::default()
            }],
            previous: (offset > 0).then(|| offset.saturating_sub(192) as u64),
            next: (offset + 192 < chars.len()).then_some(offset as u64 + 192),
            position: offset as u64,
            notice: "Search sets the specimen text · font collections show their first face".into(),
            ..Inspection::default()
        }))
    }
}
fn draw(
    font: &fontdue::Font,
    text: &str,
    size: f32,
    y: f32,
    pixels: &mut [u8],
    cancel: &CancelToken,
) -> anyhow::Result<()> {
    use fontdue::layout::{CoordinateSystem, Layout, LayoutSettings, TextStyle};
    let mut layout = Layout::new(CoordinateSystem::PositiveYDown);
    layout.reset(&LayoutSettings {
        x: 16.,
        y,
        max_width: Some(968.),
        ..LayoutSettings::default()
    });
    layout.append(&[font], &TextStyle::new(text, size, 0));
    for glyph in layout.glyphs() {
        inspection::check(cancel)?;
        let (metrics, bitmap) = font.rasterize_config(glyph.key);
        for row in 0..metrics.height {
            for col in 0..metrics.width {
                let x = glyph.x as i32 + i32::try_from(col)?;
                let y = glyph.y as i32 + i32::try_from(row)?;
                if (0..1000).contains(&x) && (0..650).contains(&y) {
                    let start = (y as usize * 1000 + x as usize) * 4;
                    let gray = 255 - bitmap[row * metrics.width + col];
                    pixels[start..start + 3].fill(gray);
                }
            }
        }
    }
    Ok(())
}
