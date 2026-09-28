//! Mermaid diagrams in fenced code blocks (```` ```mermaid ````), drawn as
//! the pictures they describe, when `bibli-mermaid-worker` is installed
//! (`bibli_platform::diagram`). Its SVG is drawn like any drawing in a
//! document (`inspection::image`), on white: Mermaid's default theme is
//! dark text on nothing, which a dark panel would swallow.
use bibli_core::{CancelToken, thumbs::Image};
use bibli_platform::{diagram, inspection};

/// The diagram `source` describes, or `None` when it cannot be drawn (no
/// helper, a kind merman does not know, a mistake): the block stays code.
pub fn mermaid(source: &str, cancel: &CancelToken) -> Option<Image> {
    let svg = diagram::mermaid(source, cancel)
        .inspect_err(|error| tracing::debug!("mermaid diagram left as code: {error:#}"))
        .ok()?;
    let mut image = inspection::image(svg.as_bytes()).ok()?;
    on_white(&mut image);
    Some(image)
}

/// `image`'s pixels laid over white.
fn on_white(image: &mut Image) {
    let mut rgba = image.rgba.to_vec();
    for pixel in rgba.as_chunks_mut::<4>().0 {
        let alpha = u32::from(pixel[3]);
        for channel in &mut pixel[..3] {
            *channel = ((u32::from(*channel) * alpha + 255 * (255 - alpha)) / 255) as u8;
        }
        pixel[3] = 255;
    }
    image.rgba = rgba.into();
}
