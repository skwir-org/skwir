//! Pictures and drawings: each picture on a page of its own, fitted to
//! the text, or several to a page with their names, as a contact sheet;
//! a document's pages drawn as pictures; an SVG drawing kept vector. A
//! JPEG is embedded as it is, other pictures as krilla takes them in;
//! each is a figure with its alternative text for a screen reader.

use std::io::Read;
use std::path::Path;

use anyhow::{Context, Result, bail};
use krilla::geom::{PathBuilder, Size, Transform};
use krilla::image::Image;
use krilla::paint::Stroke;
use krilla::surface::Surface;
use krilla::tagging::Tag;
use krilla_svg::{SurfaceExt, SvgSettings};
use parley::Alignment;
use skwir_core::print::Picture;

use crate::fonts::{Fonts, Role};
use crate::geometry::{Area, CAPTION, Frame, Grid, fit};
use crate::out::{PageCx, Part, Structure, key};
use crate::text::{self, GREY, RULE, Spec, Wrap};

/// The largest picture read, and the largest drawing.
const PICTURE_LIMIT: u64 = 256 * 1024 * 1024;
const DRAWING_LIMIT: u64 = 64 * 1024 * 1024;

pub struct Pictures<'a> {
    all: &'a [Picture],
    sheet: bool,
    frame: Frame,
    grid: Grid,
    structure: Structure,
}

impl<'a> Pictures<'a> {
    pub fn plan(pictures: &'a [Picture], sheet: bool, frame: Frame) -> Pictures<'a> {
        Pictures {
            all: pictures,
            sheet,
            frame,
            grid: Grid::new(frame.text),
            structure: Structure::default(),
        }
    }
}

impl Part for Pictures<'_> {
    fn pages(&self) -> usize {
        let per_page = if self.sheet { self.grid.per_page() } else { 1 };
        self.all.len().div_ceil(per_page).max(1)
    }

    fn draw(&mut self, page: usize, cx: &mut PageCx) -> Result<()> {
        if !self.sheet {
            if let Some(picture) = self.all.get(page) {
                self.structure
                    .goto(cx, &[(key(1, page), figure(&picture.alt))]);
                let area = picture_area(picture, self.frame.text);
                cx.content(|surface, _| draw_picture(surface, &picture.path, area));
                self.structure.close_all(cx);
            }
            return Ok(());
        }
        let per_page = self.grid.per_page();
        let first = page * per_page;
        let end = (first + per_page).min(self.all.len());
        for (slot, index) in (first..end).enumerate() {
            let picture = &self.all[index];
            let cell = self.grid.cell(self.frame.text, slot);
            let room = Area {
                height: self.grid.picture,
                ..cell
            };
            self.structure
                .goto(cx, &[(key(1, index), figure(&picture.alt))]);
            // On its name, however much higher the room goes.
            let mut area = picture_area(picture, room);
            area.y = room.y + room.height - area.height;
            cx.content(|surface, _| draw_picture(surface, &picture.path, area));
            if let Some(caption) = picture.caption.as_deref().filter(|c| !c.is_empty()) {
                let mut spec = Spec::new(Role::Text, 7.0);
                spec.color = GREY;
                spec.width = Some(cell.width);
                spec.align = Alignment::Center;
                spec.wrap = Wrap::Anywhere;
                spec.line_height = 1.2;
                let (layout, text) = text::fitted(cx.fonts, caption, &spec, 1);
                self.structure.goto(
                    cx,
                    &[
                        (key(1, index), figure(&picture.alt)),
                        (key(2, index), Tag::Caption.into()),
                    ],
                );
                let y = cell.y + self.grid.picture + (CAPTION - 7.0 * 1.2) / 2.0;
                cx.lines(&layout, &text, &[], 0..layout.len(), (cell.x, y));
            }
            self.structure.close_all(cx);
        }
        Ok(())
    }

    fn finish(&mut self, cx: &mut PageCx) {
        self.structure.close_all(cx);
    }
}

fn figure(alt: &str) -> krilla::tagging::TagKind {
    let alt = alt.trim();
    Tag::Figure((!alt.is_empty()).then(|| alt.to_owned())).into()
}

/// Where a picture goes in `room`: its size as skwir wrote it fitted,
/// centred.
fn picture_area(picture: &Picture, room: Area) -> Area {
    let (width, height) = if picture.width > 0 && picture.height > 0 {
        (picture.width as f32, picture.height as f32)
    } else {
        (room.width, room.height)
    };
    fit(width, height, room, f32::INFINITY)
}

/// The picture at `path` in `area`, a grey frame where it cannot be read.
pub fn draw_picture(surface: &mut Surface, path: &Path, area: Area) {
    let Some(size) = Size::from_wh(area.width.max(0.1), area.height.max(0.1)) else {
        return;
    };
    if let Some(image) = read_picture(path) {
        surface.push_transform(&Transform::from_translate(area.x, area.y));
        surface.draw_image(image, size);
        surface.pop();
        return;
    }
    let mut frame = PathBuilder::new();
    if let Some(rect) = krilla::geom::Rect::from_xywh(area.x, area.y, area.width, area.height) {
        frame.push_rect(rect);
    }
    if let Some(path) = frame.finish() {
        surface.set_fill(None);
        surface.set_stroke(Some(Stroke {
            paint: krilla::color::rgb::Color::new(RULE[0], RULE[1], RULE[2]).into(),
            width: 0.75,
            ..Stroke::default()
        }));
        surface.draw_path(&path);
        surface.set_stroke(None);
    }
}

/// A picture skwir wrote, by what its bytes say it is.
fn read_picture(path: &Path) -> Option<Image> {
    let data = read_bounded(path, PICTURE_LIMIT).ok()?;
    let data_of = |data: Vec<u8>| krilla::Data::from(data);
    if data.starts_with(&[0xff, 0xd8, 0xff]) {
        Image::from_jpeg(data_of(data), true).ok()
    } else if data.starts_with(b"\x89PNG") {
        Image::from_png(data_of(data), true).ok()
    } else if data.starts_with(b"GIF8") {
        Image::from_gif(data_of(data), true).ok()
    } else if data.len() > 12 && &data[..4] == b"RIFF" && &data[8..12] == b"WEBP" {
        Image::from_webp(data_of(data), true).ok()
    } else {
        None
    }
}

fn read_bounded(path: &Path, limit: u64) -> Result<Vec<u8>> {
    let file = std::fs::File::open(path)?;
    let size = file.metadata()?.len();
    if size > limit {
        bail!("{} is {size} bytes, more than {limit}", path.display());
    }
    let mut data = Vec::with_capacity(size as usize);
    file.take(limit).read_to_end(&mut data)?;
    Ok(data)
}

/// An SVG drawing on a page of its own, fitted to the text.
pub struct Drawing {
    tree: usvg::Tree,
    alt: String,
    frame: Frame,
    structure: Structure,
}

impl Drawing {
    pub fn plan(svg: &Path, alt: &str, fonts: &Fonts, frame: Frame) -> Result<Drawing> {
        let data = read_bounded(svg, DRAWING_LIMIT)
            .with_context(|| format!("the drawing {} could not be read", svg.display()))?;
        let tree = usvg::Tree::from_data(&data, &fonts.svg_options())
            .with_context(|| format!("the drawing {} could not be read", svg.display()))?;
        Ok(Drawing {
            tree,
            alt: alt.to_owned(),
            frame,
            structure: Structure::default(),
        })
    }
}

impl Part for Drawing {
    fn pages(&self) -> usize {
        1
    }

    fn draw(&mut self, _page: usize, cx: &mut PageCx) -> Result<()> {
        let size = self.tree.size();
        let area = fit(size.width(), size.height(), self.frame.text, f32::INFINITY);
        self.structure.goto(cx, &[(key(1, 0), figure(&self.alt))]);
        let tree = &self.tree;
        cx.content(|surface, _| {
            if let Some(size) = Size::from_wh(area.width.max(0.1), area.height.max(0.1)) {
                surface.push_transform(&Transform::from_translate(area.x, area.y));
                surface.draw_svg(tree, size, SvgSettings::default());
                surface.pop();
            }
        });
        self.structure.close_all(cx);
        Ok(())
    }

    fn finish(&mut self, cx: &mut PageCx) {
        self.structure.close_all(cx);
    }
}
