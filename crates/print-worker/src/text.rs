//! Text: a string and its runs laid out by parley, and the lines of a
//! layout drawn by krilla as glyphs that carry the text they stand for,
//! so the PDF's text is selected and copied as it reads.

use std::ops::Range;

use krilla::color::rgb;
use krilla::geom::{PathBuilder, Point, Rect, Transform};
use krilla::num::NormalizedF32;
use krilla::paint::{Fill, Stroke};
use krilla::surface::Surface;
use krilla::text::{GlyphId, KrillaGlyph};
use parley::{
    Alignment, AlignmentOptions, FontFeature, FontFeatures, Language, Layout, LineHeight,
    OverflowWrap, StyleProperty,
};
use skwir_core::print::Run;

use crate::fonts::{Fonts, Role};

/// Text near-black, as the light look writes it.
pub const INK: [u8; 3] = [0x1f, 0x23, 0x28];
/// What is said beside the text: the name above, numbers, captions.
pub const GREY: [u8; 3] = [0x65, 0x6d, 0x76];
pub const LINK: [u8; 3] = [0x09, 0x69, 0xda];
pub const CODE_BACK: [u8; 3] = [0xf6, 0xf8, 0xfa];
pub const RULE: [u8; 3] = [0xd0, 0xd7, 0xde];

/// How a stretch of glyphs is painted, parley's brush: its color, the
/// link it is part of (an index in the layout's links), raised or
/// lowered, on the code background.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Ink {
    pub color: [u8; 3],
    pub link: Option<u32>,
    pub shift: i8,
    pub back: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Wrap {
    /// Between words; a word longer than the line where it must.
    Words,
    /// Anywhere a line is full, between words where it can (code).
    Anywhere,
    /// On one line, however long.
    None,
}

/// What a layout is written in, before its runs.
#[derive(Clone, Debug)]
pub struct Spec {
    pub role: Role,
    pub size: f32,
    pub bold: bool,
    pub italic: bool,
    pub color: [u8; 3],
    /// Line height against the size.
    pub line_height: f32,
    pub wrap: Wrap,
    pub align: Alignment,
    pub width: Option<f32>,
    pub language: Option<Language>,
}

impl Spec {
    pub fn new(role: Role, size: f32) -> Spec {
        Spec {
            role,
            size,
            bold: false,
            italic: false,
            color: INK,
            line_height: 1.4,
            wrap: Wrap::Words,
            align: Alignment::Start,
            width: None,
            language: None,
        }
    }
}

/// The runs of a text merged where they overlap: each stretch with what
/// every run over it says, in order, without gaps between them filled.
#[derive(Clone, Debug, Default, PartialEq)]
#[allow(clippy::struct_excessive_bools)] // A run's styles, each on or off alone.
struct Merged {
    color: Option<[u8; 3]>,
    bold: bool,
    italic: bool,
    underline: bool,
    strike: bool,
    code: bool,
    script: i8,
    small_caps: bool,
    link: Option<u32>,
}

/// A run's byte range within `text`, on character boundaries; `None`
/// when empty.
fn clamp(text: &str, run: &Run) -> Option<Range<usize>> {
    let start = text.floor_char_boundary((run.start as usize).min(text.len()));
    let end = text.floor_char_boundary((run.end as usize).min(text.len()));
    (start < end).then_some(start..end)
}

/// The stretches of `text` its runs cover, each with their styles merged,
/// and the links they name, numbered.
fn stretches(text: &str, runs: &[Run], links: &mut Vec<String>) -> Vec<(Range<usize>, Merged)> {
    let mut edges: Vec<usize> = Vec::with_capacity(runs.len() * 2);
    let mut spans: Vec<(Range<usize>, &Run, Option<u32>)> = Vec::with_capacity(runs.len());
    for run in runs {
        let Some(range) = clamp(text, run) else {
            continue;
        };
        let link = run.link.as_ref().map(|link| {
            if let Some(at) = links.iter().rposition(|known| known == link) {
                at as u32
            } else {
                links.push(link.clone());
                (links.len() - 1) as u32
            }
        });
        edges.push(range.start);
        edges.push(range.end);
        spans.push((range, run, link));
    }
    if spans.is_empty() {
        return Vec::new();
    }
    edges.sort_unstable();
    edges.dedup();
    spans.sort_by_key(|(range, _, _)| range.start);
    let mut out = Vec::new();
    let mut active: Vec<usize> = Vec::new();
    let mut next = 0;
    for pair in edges.windows(2) {
        let (from, to) = (pair[0], pair[1]);
        active.retain(|&i| spans[i].0.end > from);
        while next < spans.len() && spans[next].0.start <= from {
            if spans[next].0.end > from {
                active.push(next);
            }
            next += 1;
        }
        if active.is_empty() {
            continue;
        }
        let mut merged = Merged::default();
        for &i in &active {
            let (_, run, link) = &spans[i];
            merged.color = run.color.or(merged.color);
            merged.bold |= run.bold;
            merged.italic |= run.italic;
            merged.underline |= run.underline;
            merged.strike |= run.strike;
            merged.code |= run.code;
            merged.small_caps |= run.small_caps;
            if run.script != 0 {
                merged.script = run.script.signum();
            }
            merged.link = link.or(merged.link);
        }
        out.push((from..to, merged));
    }
    out
}

/// `text` laid out as `spec` and its runs say, into `layout`; the links
/// its runs name are added to `links`, which its glyphs' ink numbers.
pub fn lay_out(
    fonts: &mut Fonts,
    layout: &mut Layout<Ink>,
    text: &str,
    runs: &[Run],
    spec: &Spec,
    links: &mut Vec<String>,
) {
    let stretches = stretches(text, runs, links);
    let Fonts { cx, lcx, faces, .. } = fonts;
    let mut stacks = Vec::new();
    let (family, weight, style) = faces.stack(spec.role, spec.bold, spec.italic);
    for (_, merged) in &stretches {
        let role = if merged.code { Role::Code } else { spec.role };
        stacks.push(faces.stack(role, spec.bold || merged.bold, spec.italic || merged.italic));
    }
    let small_caps = [FontFeature::new(parley::setting::Tag::new(b"smcp"), 1)];
    let mut builder = lcx.ranged_builder(cx, text, 1.0, false);
    builder.push_default(StyleProperty::FontFamily(family));
    builder.push_default(StyleProperty::FontWeight(weight));
    builder.push_default(StyleProperty::FontStyle(style));
    builder.push_default(StyleProperty::FontSize(spec.size));
    builder.push_default(StyleProperty::LineHeight(LineHeight::FontSizeRelative(
        spec.line_height,
    )));
    builder.push_default(StyleProperty::Brush(Ink {
        color: spec.color,
        ..Ink::default()
    }));
    builder.push_default(StyleProperty::OverflowWrap(match spec.wrap {
        Wrap::Anywhere => OverflowWrap::Anywhere,
        Wrap::Words | Wrap::None => OverflowWrap::BreakWord,
    }));
    if let Some(language) = spec.language {
        builder.push_default(StyleProperty::Locale(Some(language)));
    }
    for ((range, merged), (family, weight, style)) in stretches.iter().zip(stacks) {
        if merged.code || merged.bold || merged.italic {
            builder.push(StyleProperty::FontFamily(family), range.clone());
            builder.push(StyleProperty::FontWeight(weight), range.clone());
            builder.push(StyleProperty::FontStyle(style), range.clone());
        }
        let mut size = spec.size;
        if merged.code && spec.role != Role::Code {
            size *= 0.9;
        }
        if merged.script != 0 {
            size *= 0.7;
        }
        if (size - spec.size).abs() > f32::EPSILON {
            builder.push(StyleProperty::FontSize(size), range.clone());
        }
        let color = merged
            .color
            .or(merged.link.map(|_| LINK))
            .unwrap_or(spec.color);
        let ink = Ink {
            color,
            link: merged.link,
            shift: merged.script,
            back: merged.code && spec.role != Role::Code,
        };
        if ink.color != spec.color || ink.link.is_some() || ink.shift != 0 || ink.back {
            builder.push(StyleProperty::Brush(ink), range.clone());
        }
        if merged.underline {
            builder.push(StyleProperty::Underline(true), range.clone());
        }
        if merged.strike {
            builder.push(StyleProperty::Strikethrough(true), range.clone());
        }
        if merged.small_caps {
            builder.push(
                StyleProperty::FontFeatures(FontFeatures::from(&small_caps)),
                range.clone(),
            );
        }
    }
    builder.build_into(layout, text);
    let width = match spec.wrap {
        Wrap::None => None,
        Wrap::Words | Wrap::Anywhere => spec.width,
    };
    layout.break_all_lines(width);
    layout.align(spec.align, AlignmentOptions::default());
}

/// `text` without runs laid out as `spec` says.
pub fn plain(fonts: &mut Fonts, text: &str, spec: &Spec) -> Layout<Ink> {
    let mut layout = Layout::new();
    lay_out(fonts, &mut layout, text, &[], spec, &mut Vec::new());
    layout
}

/// `text` on at most `lines` lines: cut, and an ellipsis after it, when
/// it takes more. The layout and the text it holds.
pub fn fitted(fonts: &mut Fonts, text: &str, spec: &Spec, lines: usize) -> (Layout<Ink>, String) {
    let layout = plain(fonts, text, spec);
    let lines = lines.max(1);
    if layout.len() <= lines {
        return (layout, text.to_owned());
    }
    let mut end = layout
        .get(lines - 1)
        .map_or(text.len(), |line| line.text_range().end);
    for _ in 0..24 {
        let kept = text[..text.floor_char_boundary(end)].trim_end();
        let candidate = format!("{kept}…");
        let layout = plain(fonts, &candidate, spec);
        if layout.len() <= lines || kept.is_empty() {
            return (layout, candidate);
        }
        // Drop a tenth of what is kept, a character at least.
        let drop = (kept.chars().count() / 10).max(1);
        end = kept
            .char_indices()
            .rev()
            .nth(drop - 1)
            .map_or(0, |(at, _)| at);
    }
    let layout = plain(fonts, "…", spec);
    (layout, "…".to_owned())
}

/// The widest advance of a printable ASCII character as `spec` writes
/// it: a line of `n` of them is at most `n` times as wide.
pub fn widest_ascii(fonts: &mut Fonts, spec: &Spec) -> f32 {
    let ascii: String = (0x21u8..0x7f).map(char::from).collect();
    let mut spec = spec.clone();
    spec.wrap = Wrap::None;
    let layout = plain(fonts, &ascii, &spec);
    let mut widest = 0.0f32;
    for line in layout.lines() {
        for run in line.runs() {
            for cluster in run.clusters() {
                widest = widest.max(cluster.advance());
            }
        }
    }
    widest
}

/// A stretch of a line drawn in one go: one style, one font.
struct Group {
    x: f32,
    advance: f32,
    style: u16,
    font: Option<krilla::text::Font>,
    size: f32,
    skew: Option<f32>,
    embolden: bool,
    underline: Option<(f32, f32)>,
    strike: Option<(f32, f32)>,
    glyphs: Vec<KrillaGlyph>,
    text: Range<usize>,
}

/// What drawing a line tells its caller about each stretch of it, so it
/// can mark the text for a screen reader and make its links: a stretch
/// starts (in a link or not); it ends, covering this rectangle and that
/// text.
pub enum Mark {
    Start(Option<u32>),
    End(Rect, Range<usize>),
}

/// Line `index` of `layout` (of `text`) drawn with its top-left corner of
/// the layout at `origin`. Backgrounds of code within the text are drawn first,
/// through `backgrounds` (an artifact for a screen reader), then the
/// glyphs, a stretch of the same link at a time between two `marks`.
#[allow(clippy::too_many_arguments)] // What a line is drawn with, each apart.
pub fn draw_line(
    surface: &mut Surface,
    fonts: &mut Fonts,
    layout: &Layout<Ink>,
    text: &str,
    index: usize,
    origin: (f32, f32),
    backgrounds: &mut dyn FnMut(&mut Surface, &[Rect]),
    marks: &mut dyn FnMut(&mut Surface, Mark),
) {
    let Some(line) = layout.get(index) else {
        return;
    };
    let metrics = line.metrics();
    let baseline = origin.1 + metrics.baseline;
    let mut x = origin.0 + metrics.offset + metrics.inline_min_coord;
    let mut groups: Vec<Group> = Vec::new();
    for run in line.runs() {
        let synthesis = run.synthesis();
        let font = fonts.krilla(run.font(), &synthesis);
        let size = run.font_size();
        let font_metrics = *run.font_metrics();
        let mut current: Option<Group> = None;
        for cluster in run.visual_clusters() {
            let advance = cluster.advance();
            if cluster.is_ligature_continuation() {
                if let Some(group) = current.as_mut() {
                    let range = cluster.text_range();
                    if let Some(glyph) = group.glyphs.last_mut() {
                        glyph.text_range.start = glyph.text_range.start.min(range.start);
                        glyph.text_range.end = glyph.text_range.end.max(range.end);
                    }
                    group.text.start = group.text.start.min(range.start);
                    group.text.end = group.text.end.max(range.end);
                    group.advance += advance;
                }
                x += advance;
                continue;
            }
            let style_index = cluster.style_index();
            if current
                .as_ref()
                .is_some_and(|group| group.style != style_index)
            {
                groups.extend(current.take());
            }
            let group = current.get_or_insert_with(|| {
                let style = &layout.styles()[usize::from(style_index)];
                let decoration = |decoration: &Option<parley::layout::Decoration<Ink>>,
                                  offset: f32,
                                  thickness: f32| {
                    decoration.as_ref().map(|d| {
                        (
                            d.offset.unwrap_or(offset),
                            d.size.unwrap_or(thickness).max(size * 0.05),
                        )
                    })
                };
                Group {
                    x,
                    advance: 0.0,
                    style: style_index,
                    font: font.clone(),
                    size,
                    skew: synthesis.skew(),
                    embolden: synthesis.embolden(),
                    underline: decoration(
                        &style.underline,
                        font_metrics.underline_offset,
                        font_metrics.underline_size,
                    ),
                    strike: decoration(
                        &style.strikethrough,
                        font_metrics.strikethrough_offset,
                        font_metrics.strikethrough_size,
                    ),
                    glyphs: Vec::new(),
                    text: cluster.text_range(),
                }
            });
            let range = cluster.text_range();
            group.text.start = group.text.start.min(range.start);
            group.text.end = group.text.end.max(range.end);
            for glyph in cluster.glyphs() {
                group.glyphs.push(krilla_glyph(&glyph, size, range.clone()));
            }
            group.advance += advance;
            x += advance;
        }
        groups.extend(current.take());
    }
    let top = origin.1 + metrics.block_min_coord;
    let bottom = origin.1 + metrics.block_max_coord;
    let backs: Vec<Rect> = groups
        .iter()
        .filter(|group| layout.styles()[usize::from(group.style)].brush.back)
        .filter_map(|group| {
            Rect::from_xywh(
                group.x - 1.0,
                top + 1.0,
                group.advance + 2.0,
                bottom - top - 2.0,
            )
        })
        .collect();
    if !backs.is_empty() {
        backgrounds(surface, &backs);
    }
    let mut at = 0;
    while at < groups.len() {
        let stretch_link = layout.styles()[usize::from(groups[at].style)].brush.link;
        let mut end = at;
        while end < groups.len()
            && layout.styles()[usize::from(groups[end].style)].brush.link == stretch_link
        {
            end += 1;
        }
        marks(surface, Mark::Start(stretch_link));
        let (mut left, mut right) = (f32::INFINITY, f32::NEG_INFINITY);
        let mut covered = Range {
            start: usize::MAX,
            end: 0,
        };
        for group in &groups[at..end] {
            let ink = &layout.styles()[usize::from(group.style)].brush;
            draw_group(surface, group, ink, baseline, text);
            left = left.min(group.x);
            right = right.max(group.x + group.advance);
            covered.start = covered.start.min(group.text.start);
            covered.end = covered.end.max(group.text.end);
        }
        let rect = Rect::from_ltrb(left, top, right.max(left + 0.1), bottom.max(top + 0.1))
            .unwrap_or_else(|| Rect::from_xywh(0.0, 0.0, 1.0, 1.0).expect("a unit square"));
        marks(surface, Mark::End(rect, covered));
        at = end;
    }
}

/// A glyph parley placed, as krilla draws it, for the text `text`.
///
/// Both take advances and horizontal offsets rightwards, but not vertical
/// offsets the same way. parley lays out downwards and turns the shaper's
/// upward offset over (`y` is `-y_offset`, `parley_engine`'s
/// `shaped_text.rs`); krilla keeps the shaper's sign, upwards, and
/// subtracts it from its own downward `y` (its own shaper hands it the
/// offset as it is). So `y` is turned over again here: a mark raised over
/// a capital stays raised. krilla also wants each in ems, not points.
fn krilla_glyph(glyph: &parley::layout::Glyph, size: f32, text: Range<usize>) -> KrillaGlyph {
    KrillaGlyph::new(
        GlyphId::new(glyph.id),
        glyph.advance / size,
        glyph.x / size,
        -glyph.y / size,
        0.0,
        text,
        None,
    )
}

pub fn fill(color: [u8; 3]) -> Fill {
    Fill {
        paint: rgb::Color::new(color[0], color[1], color[2]).into(),
        opacity: NormalizedF32::ONE,
        rule: krilla::paint::FillRule::NonZero,
    }
}

fn draw_group(surface: &mut Surface, group: &Group, ink: &Ink, baseline: f32, text: &str) {
    let shift = f32::from(ink.shift) * group.size * 0.45;
    let baseline = baseline - shift;
    if let Some(font) = &group.font {
        surface.set_fill(Some(fill(ink.color)));
        surface.set_stroke(group.embolden.then(|| Stroke {
            paint: rgb::Color::new(ink.color[0], ink.color[1], ink.color[2]).into(),
            width: group.size * 0.03,
            ..Stroke::default()
        }));
        let skewed = group.skew.map(|degrees| {
            let k = degrees.to_radians().tan();
            Transform::from_row(1.0, 0.0, -k, 1.0, k * baseline, 0.0)
        });
        if let Some(transform) = &skewed {
            surface.push_transform(transform);
        }
        surface.draw_glyphs(
            Point::from_xy(group.x, baseline),
            &group.glyphs,
            font.clone(),
            text,
            group.size,
            false,
        );
        if skewed.is_some() {
            surface.pop();
        }
        surface.set_stroke(None);
    }
    for (offset, thickness) in [group.underline, group.strike].into_iter().flatten() {
        let top = baseline - offset;
        if let Some(rect) = Rect::from_xywh(group.x, top, group.advance, thickness) {
            let mut path = PathBuilder::new();
            path.push_rect(rect);
            if let Some(path) = path.finish() {
                surface.set_fill(Some(fill(ink.color)));
                surface.draw_path(&path);
            }
        }
    }
}

/// A filled rectangle.
pub fn rect(surface: &mut Surface, area: Rect, color: [u8; 3]) {
    let mut path = PathBuilder::new();
    path.push_rect(area);
    if let Some(path) = path.finish() {
        surface.set_fill(Some(fill(color)));
        surface.set_stroke(None);
        surface.draw_path(&path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(start: u32, end: u32) -> Run {
        Run {
            start,
            end,
            ..Run::default()
        }
    }

    #[test]
    fn overlapping_runs_merge_and_links_are_numbered_once() {
        let mut links = Vec::new();
        let text = "bold and linked";
        let runs = [
            Run {
                bold: true,
                ..run(0, 8)
            },
            Run {
                link: Some("https://example.org".into()),
                ..run(5, 15)
            },
            Run {
                link: Some("https://example.org".into()),
                italic: true,
                ..run(9, 15)
            },
        ];
        let stretches = stretches(text, &runs, &mut links);
        assert_eq!(links, ["https://example.org"]);
        let ranges: Vec<_> = stretches.iter().map(|(r, _)| r.clone()).collect();
        assert_eq!(ranges, [0..5, 5..8, 8..9, 9..15]);
        assert!(stretches[1].1.bold && stretches[1].1.link == Some(0));
        assert!(!stretches[2].1.bold && stretches[2].1.link == Some(0));
        assert!(stretches[3].1.italic);
    }

    #[test]
    fn a_glyph_parley_raises_stays_raised_for_krilla() {
        // parley's 2 points up (y downwards) at 10 points: 0.2 em up.
        let raised = parley::layout::Glyph {
            id: 7,
            x: 1.0,
            y: -2.0,
            advance: 0.0,
        };
        let glyph = krilla_glyph(&raised, 10.0, 0..2);
        assert!((glyph.y_offset - 0.2).abs() < 1e-6, "{}", glyph.y_offset);
        assert!((glyph.x_offset - 0.1).abs() < 1e-6);
        assert_eq!(glyph.text_range, 0..2);
    }

    #[test]
    fn runs_outside_the_text_or_inside_a_character_are_clamped() {
        let mut links = Vec::new();
        let text = "é!";
        let stretches = stretches(text, &[run(1, 99), run(5, 9), run(0, 0)], &mut links);
        assert_eq!(stretches.len(), 1);
        assert_eq!(stretches[0].0, 0..3);
    }
}
