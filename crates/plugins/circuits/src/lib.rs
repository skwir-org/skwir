//! skwir plugin: circuit boards and schematics drawn from their files.
//! `KiCad`'s boards, footprints, schematics and symbol libraries, and the
//! Gerber and Excellon drill files any design program sends to the board
//! maker, are text, read here and written as SVG, which skwir's inert
//! rasterizer draws (`inspection::drawing`): in the preview panel, with
//! what the file holds in Properties and its text one toggle away, and as
//! the grid's thumbnails. Nothing outside the file is read.
mod board;
mod drill;
mod gerber;
mod schematic;
mod sexpr;
mod svg;

use skwir_core::{
    App, CancelToken,
    inspection::{Detail, InspectRequest, Inspection, InspectionKind},
    plugin::Plugin,
    preview::{PreviewProvider, PreviewRequest},
    thumbs::{Image, Stage, ThumbRequest, ThumbnailProvider},
};
use skwir_platform::inspection;
use std::path::Path;
use std::sync::Arc;

const ID: &str = "circuits";
/// The largest file drawn. A board is read an item at a time, but its
/// text is held whole, and its drawing is as long.
const INPUT: usize = 32 * 1024 * 1024;
/// The side a drawing is rasterized at for the panel: fine traces stay
/// visible on a dense screen.
const PANEL_SIDE: u32 = 2048;

pub struct PreviewPlugin;

impl Plugin for PreviewPlugin {
    fn id(&self) -> &'static str {
        ID
    }
    fn title(&self) -> &'static str {
        "Circuit boards and schematics"
    }
    fn description(&self) -> &'static str {
        "Draw KiCad boards, footprints, schematics and symbol libraries, and Gerber and drill files; board size, layers, footprints, nets, symbols and holes in Properties"
    }
    fn category(&self) -> skwir_core::plugin::Category {
        skwir_core::plugin::Category::Previews
    }
    fn capabilities(&self) -> skwir_core::permissions::Capabilities {
        use skwir_core::permissions::Capability;
        skwir_core::permissions::Capabilities::of(&[Capability::Read])
    }
    fn register(&self, app: &App) {
        app.previews.register(ID, Arc::new(Provider));
        app.thumbnails.register(ID, Arc::new(Pictures));
    }
}

/// The formats read here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Format {
    Board,
    Footprint,
    Schematic,
    Symbols,
    /// A Gerber layer, whatever made it.
    Gerber,
    /// An Excellon drill file.
    Drill,
}

impl Format {
    /// By the file's extension (`KiCad`'s backups end in `-bak`).
    fn of(path: &Path) -> Option<Format> {
        let extension = inspection::extension(path);
        Some(match extension.trim_end_matches("-bak") {
            "kicad_pcb" => Format::Board,
            "kicad_mod" => Format::Footprint,
            "kicad_sch" => Format::Schematic,
            "kicad_sym" => Format::Symbols,
            // A layer's name says which (`.gtl`, top copper); `.gbr` is
            // also a GIMP brush's, taken only if it is Gerber (`ours`).
            "gbr" | "ger" | "gbx" | "pho" | "gtl" | "gbl" | "gts" | "gbs" | "gto" | "gbo"
            | "gtp" | "gbp" | "gta" | "gba" | "gko" | "gml" => Format::Gerber,
            // Inner layers and outlines by number: `.g2`, `.gp1`, `.gm1`.
            name if ["g", "gp", "gm"].iter().any(|prefix| {
                name.strip_prefix(prefix)
                    .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
            }) =>
            {
                Format::Gerber
            }
            "drl" | "xln" | "exc" | "xnc" => Format::Drill,
            _ => return None,
        })
    }

    /// What it is, for the toolbar.
    fn title(self) -> &'static str {
        match self {
            Format::Board => "KiCad board",
            Format::Footprint => "KiCad footprint",
            Format::Schematic => "KiCad schematic",
            Format::Symbols => "KiCad symbol library",
            Format::Gerber => "Gerber layer",
            Format::Drill => "Drill file",
        }
    }

    /// Whether the file is what its name says: a `.gbr` may be a GIMP
    /// brush, left to the other previews. Reads its start.
    fn ours(self, path: &Path) -> anyhow::Result<bool> {
        use std::io::Read as _;
        if !matches!(self, Format::Gerber | Format::Drill) {
            return Ok(true);
        }
        let mut head = Vec::new();
        inspection::open(path)?
            .take(64 * 1024)
            .read_to_end(&mut head)?;
        if head.contains(&0) {
            return Ok(false);
        }
        let head = String::from_utf8_lossy(&head);
        Ok(match self {
            Format::Gerber => gerber::looks_like(&head),
            _ => drill::looks_like(&head),
        })
    }

    fn read(self, path: &Path, cancel: &CancelToken) -> anyhow::Result<Found> {
        skwir_platform::thumb_providers::ensure_file(path)?;
        let bytes = inspection::read_bounded(path, INPUT, "The file", cancel)?;
        let text = String::from_utf8_lossy(&bytes);
        match self {
            Format::Board => board::board(&text, cancel),
            Format::Footprint => board::footprint_file(&text),
            Format::Schematic => schematic::schematic(&text, cancel),
            Format::Symbols => schematic::library(&text),
            Format::Gerber => {
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                let drawn = gerber::draw(&text, &name, cancel)?;
                Ok(Found {
                    svg: drawn.svg,
                    details: drawn.details,
                    notice: drawn.left_out.then(|| {
                        "Drawn in part: the layer holds more than is drawn here".to_owned()
                    }),
                })
            }
            Format::Drill => {
                let drawn = drill::draw(&text, cancel)?;
                Ok(Found {
                    svg: drawn.svg,
                    details: drawn.details,
                    notice: None,
                })
            }
        }
    }
}

/// Whether the file is larger than what is drawn.
fn too_large(path: &Path) -> anyhow::Result<bool> {
    Ok(std::fs::metadata(path)?.len() > INPUT as u64)
}

/// What a file holds: its drawing, and facts in the order shown.
struct Found {
    svg: Option<String>,
    details: Vec<(String, String)>,
    /// What was left out, and why.
    notice: Option<String>,
}

/// A drawing rasterized `side` pixels on its longer side.
fn raster(svg: &str, side: u32, cancel: &CancelToken) -> anyhow::Result<Image> {
    inspection::check(cancel)?;
    let drawn = inspection::drawing(svg.as_bytes(), Some(side))?;
    Ok(Image {
        width: drawn.width,
        height: drawn.height,
        rgba: drawn.rgba.into(),
    })
}

struct Provider;

impl PreviewProvider for Provider {
    fn id(&self) -> &'static str {
        ID
    }
    /// Called on a worker (the registry's dispatch), so what a name could
    /// be two things (a `.gbr` brush) is settled by reading its start: the
    /// first provider that accepts owns the file, and one that is not ours
    /// is left to the others.
    fn accepts(&self, req: &PreviewRequest) -> bool {
        Format::of(&req.entry.path)
            .is_some_and(|format| format.ours(&req.entry.path).unwrap_or(false))
    }
    fn inspect(
        &self,
        req: &PreviewRequest,
        view: &InspectRequest,
        cancel: &CancelToken,
    ) -> anyhow::Result<Option<Inspection>> {
        // The source is the shared code view (the worker's fallback).
        if view.source {
            return Ok(None);
        }
        let path = req.entry.resolve()?;
        let Some(format) = Format::of(&path) else {
            return Ok(None);
        };
        // Not drawn: the panel shows its source instead, saying why
        // (`preview::source_instead`).
        anyhow::ensure!(
            !too_large(&path)?,
            "Too large to draw (more than {})",
            inspection::limits::describe(INPUT)
        );
        let found = format.read(&path, cancel)?;
        let image = match &found.svg {
            Some(svg) => Some(raster(svg, PANEL_SIDE, cancel)?),
            None => None,
        };
        let notice = match (&image, found.notice) {
            (None, _) => "Nothing is drawn in this file".to_owned(),
            (Some(_), notice) => notice.unwrap_or_default(),
        };
        Ok(Some(Inspection {
            kind: InspectionKind::Text,
            title: format.title().into(),
            image,
            details: found
                .details
                .into_iter()
                .map(|(label, value)| Detail::new(label, value))
                .collect(),
            source_available: true,
            notice,
            ..Inspection::default()
        }))
    }
}

/// The grid's thumbnails: the same drawings.
struct Pictures;

impl ThumbnailProvider for Pictures {
    fn id(&self) -> &'static str {
        "circuit-drawings"
    }
    fn stage(&self) -> Stage {
        Stage::Fallback
    }
    fn accepts(&self, req: &ThumbRequest) -> bool {
        Format::of(&req.entry.path).is_some()
    }
    fn thumbnail(&self, req: &ThumbRequest, cancel: &CancelToken) -> anyhow::Result<Option<Image>> {
        let path = req.entry.resolve()?;
        let Some(format) = Format::of(&path) else {
            return Ok(None);
        };
        if too_large(&path)? || !format.ours(&path)? {
            return Ok(None);
        }
        match format.read(&path, cancel)?.svg {
            Some(svg) => Ok(Some(raster(&svg, req.size.pixels(), cancel)?)),
            None => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use skwir_core::op::EntryRef;

    fn request(path: &Path) -> PreviewRequest {
        PreviewRequest {
            entry: EntryRef::from_path(path).unwrap(),
            mime: None,
            page: 1,
            seconds: None,
            max_side: 2048,
            text_size: None,
        }
    }

    #[test]
    fn formats_by_name() {
        let of = |name: &str| Format::of(Path::new(name));
        assert_eq!(of("board.kicad_pcb"), Some(Format::Board));
        assert_eq!(of("board.kicad_pcb-bak"), Some(Format::Board));
        assert_eq!(of("R_0402.kicad_mod"), Some(Format::Footprint));
        assert_eq!(of("amp.kicad_sch"), Some(Format::Schematic));
        assert_eq!(of("parts.kicad_sym"), Some(Format::Symbols));
        assert_eq!(of("amp.kicad_pro"), None);
        assert_eq!(of("board-F_Cu.gtl"), Some(Format::Gerber));
        assert_eq!(of("board-In2_Cu.g2"), Some(Format::Gerber));
        assert_eq!(of("board-Edge_Cuts.gm1"), Some(Format::Gerber));
        assert_eq!(of("board.drl"), Some(Format::Drill));
        assert_eq!(of("g.txt"), None);
    }

    #[test]
    fn a_board_in_the_panel_with_its_source_one_toggle_away() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a maker’s board.kicad_pcb");
        std::fs::write(
            &path,
            r#"(kicad_pcb (version 20241229)
                (gr_rect (start 0 0) (end 40 20) (stroke (width 0.1)) (layer "Edge.Cuts"))
                (segment (start 5 5) (end 35 15) (width 0.5) (layer "F.Cu")))"#,
        )
        .unwrap();
        let request = request(&path);
        assert!(Provider.accepts(&request));
        let data = Provider
            .inspect(&request, &InspectRequest::default(), &CancelToken::new())
            .unwrap()
            .unwrap();
        let image = data.image.expect("the board drawn");
        assert_eq!(image.width, PANEL_SIDE);
        assert!(data.source_available);
        assert_eq!(data.details[0].label, "Made with");
        let source = InspectRequest {
            source: true,
            ..InspectRequest::default()
        };
        assert!(
            Provider
                .inspect(&request, &source, &CancelToken::new())
                .unwrap()
                .is_none()
        );
        let thumbnail = Pictures
            .thumbnail(
                &ThumbRequest {
                    entry: request.entry.clone(),
                    mime: None,
                    mtime: None,
                    size: skwir_core::thumbs::ThumbSize::Normal,
                },
                &CancelToken::new(),
            )
            .unwrap()
            .expect("the grid's picture");
        assert_eq!(thumbnail.width.max(thumbnail.height), 128);
    }

    #[test]
    fn an_empty_or_damaged_file_says_so() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("empty.kicad_sch");
        std::fs::write(&path, "(kicad_sch (version 20250114))").unwrap();
        let data = Provider
            .inspect(
                &request(&path),
                &InspectRequest::default(),
                &CancelToken::new(),
            )
            .unwrap()
            .unwrap();
        assert!(data.image.is_none());
        assert_eq!(data.notice, "Nothing is drawn in this file");
        // Larger than what is drawn: refused, for the panel to show the
        // source instead. The file is sparse, nothing is written.
        let large = dir.path().join("large.kicad_pcb");
        std::fs::File::create(&large)
            .unwrap()
            .set_len(INPUT as u64 + 1)
            .unwrap();
        let error = Provider
            .inspect(
                &request(&large),
                &InspectRequest::default(),
                &CancelToken::new(),
            )
            .unwrap_err();
        assert_eq!(error.to_string(), "Too large to draw (more than 32 MiB)");
        // A GIMP brush named like a Gerber layer is not ours: left to the
        // other previews.
        let brush = dir.path().join("pencil.gbr");
        std::fs::write(&brush, b"\0\0\0\x1c\0\0\0\x02GIMP\0\0\0\x0a").unwrap();
        assert!(!Provider.accepts(&request(&brush)));
        std::fs::write(&path, "(kicad_sch (wire (pts").unwrap();
        assert!(
            Provider
                .inspect(
                    &request(&path),
                    &InspectRequest::default(),
                    &CancelToken::new()
                )
                .is_err()
        );
    }
}
