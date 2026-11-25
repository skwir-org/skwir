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
    preview::{FileSource, PreviewProvider, PreviewRequest},
    thumbs::{Image, Stage, ThumbRequest, ThumbnailProvider},
    words,
    words::Words,
};
use skwir_platform::inspection::{self, limits::Bounds};
use std::path::Path;
use std::sync::Arc;

const ID: &str = "circuits";
/// The largest file drawn. A board is read an item at a time, but its
/// text is held whole, and its drawing is as long.
const INPUT: usize = 32 * 1024 * 1024;
/// The largest under Full preview (`Bounds`).
const WHOLE_INPUT: usize = 128 * 1024 * 1024;
/// The side a drawing is rasterized at for the panel: fine traces stay
/// visible on a dense screen.
const PANEL_SIDE: u32 = 2048;

pub struct PreviewPlugin;

impl Plugin for PreviewPlugin {
    fn id(&self) -> &'static str {
        ID
    }
    fn title(&self) -> skwir_core::words::Words {
        skwir_core::words!(circuits_plugin_title = "Circuit boards and schematics")
    }
    fn description(&self) -> skwir_core::words::Words {
        skwir_core::words!(
            circuits_plugin_description = "Draw KiCad boards, footprints, schematics and symbol libraries, and Gerber and drill files; board size, layers, footprints, nets, symbols and holes in Properties"
        )
    }
    fn for_viewer(&self) -> bool {
        true
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
    fn title(self) -> Words {
        match self {
            Format::Board => words!(
                circuits_title_board = "KiCad board",
                note = "The title of the preview of a printed circuit board made with KiCad."
            ),
            Format::Footprint => words!(
                circuits_title_footprint = "KiCad footprint",
                note = "The title of the preview of a KiCad footprint: the copper pads and outline of one component, as placed on a board."
            ),
            Format::Schematic => words!(
                circuits_title_schematic = "KiCad schematic",
                note = "The title of the preview of a circuit's schematic drawn with KiCad."
            ),
            Format::Symbols => words!(
                circuits_title_symbols = "KiCad symbol library",
                note = "The title of the preview of a KiCad library of schematic symbols."
            ),
            Format::Gerber => words!(
                circuits_title_gerber = "Gerber layer",
                note = "The title of the preview of a Gerber file: one layer of a circuit board (copper, solder mask…), as sent to the board maker."
            ),
            Format::Drill => words!(
                circuits_title_drill = "Drill file",
                note = "The title of the preview of an Excellon file: where a circuit board is drilled."
            ),
        }
    }

    /// Whether its name is shared with other formats, and its start says
    /// whether the file is this one (`ours`).
    fn shared(self) -> bool {
        matches!(self, Format::Gerber | Format::Drill)
    }

    /// Whether the file is what its name says: a `.gbr` may be a GIMP
    /// brush, left to the other previews. Reads its start: of what a
    /// preview holds, nothing again.
    fn ours<S: FileSource + ?Sized>(self, source: &S) -> anyhow::Result<bool> {
        if !self.shared() {
            return Ok(true);
        }
        let head = inspection::read(source, 0, 64 * 1024, &CancelToken::new())?;
        if head.contains(&0) {
            return Ok(false);
        }
        let head = String::from_utf8_lossy(&head);
        Ok(match self {
            Format::Gerber => gerber::looks_like(&head),
            _ => drill::looks_like(&head),
        })
    }

    fn read<S: FileSource + ?Sized>(
        self,
        source: &S,
        bounds: Bounds,
        cancel: &CancelToken,
    ) -> anyhow::Result<Found> {
        let most = bounds.choose(INPUT, WHOLE_INPUT);
        let refusal = words!(
            circuits_file_exceeds = "The file exceeds the { $size } preview limit",
            size = words::Size(most as u64),
            note = "Why a board, schematic or layer is not drawn: it grew past what is drawn while it was read. size: that bound."
        );
        let bytes = inspection::read_within(source, most, bounds, refusal, cancel)?;
        let text = String::from_utf8_lossy(&bytes);
        match self {
            Format::Board => board::board(&text, cancel),
            Format::Footprint => board::footprint_file(&text),
            Format::Schematic => schematic::schematic(&text, cancel),
            Format::Symbols => schematic::library(&text),
            Format::Gerber => {
                let name = source
                    .path()
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy();
                let drawn = gerber::draw(&text, &name, cancel)?;
                Ok(Found {
                    svg: drawn.svg,
                    details: drawn.details,
                    notice: drawn.left_out.then(|| {
                        words!(
                            circuits_gerber_drawn_in_part = "Drawn in part: the layer holds more than is drawn here",
                            note = "A notice under a Gerber layer's picture: some of its shapes were left out."
                        )
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

/// Whether the file is larger than what is drawn within `bounds`.
fn too_large<S: FileSource + ?Sized>(source: &S, bounds: Bounds) -> anyhow::Result<bool> {
    Ok(source.length()? > bounds.choose(INPUT, WHOLE_INPUT) as u64)
}

/// Properties' label for the program a file was made with.
fn made_with() -> Words {
    words!(
        circuits_detail_made_with = "Made with",
        note = "A label in the preview's Properties: the program, and its version, that wrote the file (KiCad 9.0)."
    )
}

/// A title block's fields, by their keys in the file, each with its label
/// in Properties.
fn title_block() -> [(&'static str, Words); 4] {
    [
        (
            "title",
            words!(
                circuits_detail_title = "Title",
                note = "A label in the preview's Properties: the title the board or schematic gives itself in its title block."
            ),
        ),
        (
            "rev",
            words!(
                circuits_detail_revision = "Revision",
                note = "A label in the preview's Properties: the revision of the design, from its title block."
            ),
        ),
        (
            "company",
            words!(
                circuits_detail_company = "Company",
                note = "A label in the preview's Properties: the company the design is from, in its title block."
            ),
        ),
        (
            "date",
            words!(
                circuits_detail_date = "Date",
                note = "A label in the preview's Properties: the date in the design's title block, as written there."
            ),
        ),
    ]
}

/// A length in millimetres, as Properties says it (`value` rounded by the
/// caller).
fn millimetres(value: f64) -> String {
    words!(
        circuits_millimetres = "{ $mm }\u{a0}mm",
        mm = number(value),
        note = "A length in Properties (a board's thickness, a drill's size). mm: the number of millimetres."
    )
    .say()
}

/// A width by a height in millimetres, as Properties says it.
fn millimetres_by(width: f64, height: f64) -> String {
    words!(
        circuits_millimetres_by = "{ $width }\u{a0}×\u{a0}{ $height }\u{a0}mm",
        width = number(width),
        height = number(height),
        note = "A size in Properties: a board's or a layer's width by its height. width, height: numbers of millimetres."
    )
    .say()
}

/// `value`, rounded by the caller to thousandths at most, as a number the
/// reader's language writes (« 1,6 »): with the digits after the point it
/// has, as a length is written on a drawing (1.6, 0.25, 2).
pub(crate) fn number(value: f64) -> words::Decimal {
    let digits = (0..3u8)
        .find(|&digits| {
            let scaled = value * 10f64.powi(i32::from(digits));
            (scaled - scaled.round()).abs() < 1e-6
        })
        .unwrap_or(3);
    words::Decimal::of(value, digits)
}

/// What a file holds: its drawing, and facts in the order shown.
struct Found {
    svg: Option<String>,
    /// Facts, each label and its value.
    details: Vec<(Words, String)>,
    /// What was left out, and why.
    notice: Option<Words>,
}

#[cfg(test)]
impl Found {
    /// Its facts, each label in English.
    fn english_details(&self) -> Vec<(String, String)> {
        self.details
            .iter()
            .map(|(label, value)| (label.english(), value.clone()))
            .collect()
    }
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
    /// be two things (a `.gbr` brush) is settled by its start, as the
    /// preview holds it: the first provider that accepts owns the file, and
    /// one that is not ours is left to the others.
    fn accepts(&self, req: &PreviewRequest) -> bool {
        Format::of(&req.entry.path).is_some_and(|format| {
            !format.shared()
                || req
                    .held()
                    .is_ok_and(|held| format.ours(&held).unwrap_or(false))
        })
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
        let held = req.held()?;
        let Some(format) = Format::of(held.path()) else {
            return Ok(None);
        };
        // Not drawn: the panel shows its source instead, saying why
        // (`preview::source_instead`), and offers Full preview while the
        // bound is the quick one.
        let bounds = Bounds::of(view);
        if too_large(&held, bounds)? {
            return Err(bounds.refuse(
                words!(
                    circuits_too_large = "Too large to draw (more than { $size })",
                    size = words::Size(bounds.choose(INPUT, WHOLE_INPUT) as u64),
                    note = "Why a board or schematic is shown as its text rather than drawn. size: the largest file drawn."
                ),
            ));
        }
        let found = format.read(&held, bounds, cancel)?;
        let image = match &found.svg {
            Some(svg) => Some(raster(svg, PANEL_SIDE, cancel)?),
            None => None,
        };
        let notices = match (&image, found.notice) {
            (None, _) => vec![words!(
                circuits_nothing_drawn = "Nothing is drawn in this file",
                note = "A notice in the preview of a board, schematic or layer that holds nothing to draw."
            )],
            (Some(_), notice) => notice.into_iter().collect(),
        };
        Ok(Some(Inspection {
            kind: InspectionKind::Text,
            title: format.title(),
            image,
            details: found
                .details
                .into_iter()
                .map(|(label, value)| Detail::new(label, value))
                .collect(),
            source_available: true,
            notices,
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
        if too_large(&path, Bounds::QUICK)? || !format.ours(&path)? {
            return Ok(None);
        }
        skwir_platform::thumb_providers::ensure_file(&path)?;
        match format.read(&path, Bounds::QUICK, cancel)?.svg {
            Some(svg) => Ok(Some(raster(&svg, req.size.pixels(), cancel)?)),
            None => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use skwir_core::op::EntryRef;

    /// A length is a number the reader's language writes, its digits after
    /// the point those it has (PB-5), never one written by `to_string`.
    #[test]
    fn a_length_is_written_as_a_decimal() {
        let digits = |value: f64| {
            let number = number(value);
            (number.value, number.digits)
        };
        assert_eq!(digits(1.6), (16, 1));
        assert_eq!(digits(0.25), (25, 2));
        assert_eq!(digits(2.0), (2, 0));
        assert_eq!(digits(0.127), (127, 3));
        assert_eq!(millimetres(1.6), "1.6\u{a0}mm");
        assert_eq!(millimetres_by(53.59, 2.0), "53.59\u{a0}×\u{a0}2\u{a0}mm");
        let pseudo = words!(test_length = "{ $mm }", mm = number(1_234.5))
            .say_in(&[words::PSEUDO.to_owned()]);
        assert!(pseudo.contains("1,234.5"), "grouped as a number: {pseudo}");
    }

    fn request(path: &Path) -> PreviewRequest {
        PreviewRequest {
            entry: EntryRef::from_path(path).unwrap(),
            mime: None,
            page: 1,
            seconds: None,
            max_side: 2048,
            text_size: None,
            fit: None,
            held: skwir_core::preview::HeldSlot::default(),
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
        assert_eq!(data.details[0].label.english(), "Made with");
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
    fn a_preview_reads_what_is_held_never_the_path() {
        let dir = tempfile::tempdir().unwrap();
        // Paths that lead nowhere: only what is held can be read.
        let gone = |name: &str| dir.path().join("gone").join(name);
        let board = PreviewRequest::holding(
            gone("a maker’s board.kicad_pcb"),
            br#"(kicad_pcb (version 20241229)
                (gr_rect (start 0 0) (end 40 20) (stroke (width 0.1)) (layer "Edge.Cuts")))"#
                .as_slice(),
            None,
        );
        assert!(Provider.accepts(&board));
        let data = Provider
            .inspect(&board, &InspectRequest::default(), &CancelToken::new())
            .unwrap()
            .unwrap();
        assert_eq!(data.title.english(), "KiCad board");
        assert!(data.image.is_some(), "the board drawn");
        // A name another format shares is settled by what is held.
        let layer = PreviewRequest::holding(
            gone("board-F_Cu.gbr"),
            b"%FSLAX46Y46*%\n%MOMM*%\n%ADD10C,0.250000*%\nD10*\nX0Y0D02*\nX10000000Y0D01*\nM02*\n"
                .as_slice(),
            None,
        );
        assert!(Provider.accepts(&layer));
        let data = Provider
            .inspect(&layer, &InspectRequest::default(), &CancelToken::new())
            .unwrap()
            .unwrap();
        assert_eq!(data.title.english(), "Gerber layer");
        assert!(data.image.is_some(), "the layer drawn");
        let brush = PreviewRequest::holding(
            gone("pencil.gbr"),
            b"\0\0\0\x1c\0\0\0\x02GIMP\0\0\0\x0a".as_slice(),
            None,
        );
        assert!(!Provider.accepts(&brush));
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
        assert_eq!(
            data.notice().map(|said| said.english()).as_deref(),
            Some("Nothing is drawn in this file")
        );
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
        assert_eq!(
            error.to_string(),
            "Too large to draw (more than 33.6\u{a0}MB)"
        );
        assert!(
            skwir_core::inspection::TooLarge::is(&error),
            "Full preview draws it"
        );
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
