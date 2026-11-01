//! Where a PDF's images are drawn: each page's drawing read (never
//! changed), its transforms followed (`q`, `Q`, `cm`) into the forms it
//! draws, to learn for each image the largest size it is drawn at, so its
//! dots per inch, and the pages that show it. An image drawn nowhere a
//! page's drawing reaches (an annotation's, a pattern's, a glyph's) is not
//! placed: it is never resized, only written again without loss.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use lopdf::content::Content;
use lopdf::{Dictionary, Document, Object, ObjectId};

/// How deep forms drawn inside forms are followed.
const DEPTH: usize = 12;

/// The most operations of drawing read on a page and the forms it draws:
/// a page of a million operations is a drawing, not a page of pictures,
/// and stops being followed there.
const OPERATIONS: usize = 4_000_000;

/// Where an image is drawn: the largest width and height it is drawn at,
/// in inches, and the pages (from 0) that show it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Placed {
    pub width: f64,
    pub height: f64,
    pub pages: BTreeSet<u32>,
}

impl Placed {
    /// Its dots per inch drawn at its largest: the fewer of its two
    /// sides', for `width` × `height` pixels.
    pub fn dpi(&self, width: u32, height: u32) -> Option<f64> {
        if self.width <= 0.0 || self.height <= 0.0 {
            return None;
        }
        Some((f64::from(width) / self.width).min(f64::from(height) / self.height))
    }
}

/// The images of `doc` drawn by its pages, by object.
pub fn placements(doc: &Document) -> BTreeMap<ObjectId, Placed> {
    let mut placed: BTreeMap<ObjectId, Placed> = BTreeMap::new();
    for (index, (_, &page_id)) in doc.get_pages().iter().enumerate() {
        let Ok(page) = doc.get_dictionary(page_id) else {
            continue;
        };
        let resources = resources_of(doc, page_id, page);
        let content = doc.get_page_content(page_id);
        let mut walk = Walk {
            doc,
            page: u32::try_from(index).unwrap_or(u32::MAX),
            placed: &mut placed,
            seen: HashSet::new(),
            operations: 0,
        };
        walk.drawing(
            &content,
            resources.as_ref(),
            [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            0,
        );
    }
    placed
}

/// A page's resources: its own, else those it inherits from the page tree.
fn resources_of(doc: &Document, page_id: ObjectId, page: &Dictionary) -> Option<Dictionary> {
    let mut node = page.clone();
    let mut seen = HashSet::from([page_id]);
    loop {
        if let Ok(resources) = node.get(b"Resources")
            && let Some(dict) = dict_of(doc, resources)
        {
            return Some(dict.clone());
        }
        let parent = node.get(b"Parent").ok()?.as_reference().ok()?;
        if !seen.insert(parent) {
            return None;
        }
        node = doc.get_dictionary(parent).ok()?.clone();
    }
}

/// `object` as a dictionary, through a reference.
pub fn dict_of<'a>(doc: &'a Document, object: &'a Object) -> Option<&'a Dictionary> {
    match doc.dereference(object).ok()?.1 {
        Object::Dictionary(dict) => Some(dict),
        Object::Stream(stream) => Some(&stream.dict),
        _ => None,
    }
}

type Matrix = [f64; 6];

/// `m` then `n`: a point drawn by `m` in a space `n` draws.
fn then(m: Matrix, n: Matrix) -> Matrix {
    [
        m[0] * n[0] + m[1] * n[2],
        m[0] * n[1] + m[1] * n[3],
        m[2] * n[0] + m[3] * n[2],
        m[2] * n[1] + m[3] * n[3],
        m[4] * n[0] + m[5] * n[2] + n[4],
        m[4] * n[1] + m[5] * n[3] + n[5],
    ]
}

fn number(object: &Object) -> Option<f64> {
    match object {
        Object::Integer(n) => {
            #[allow(clippy::cast_precision_loss)]
            let n = *n as f64;
            Some(n)
        }
        Object::Real(n) => Some(f64::from(*n)),
        _ => None,
    }
}

fn matrix(operands: &[Object]) -> Option<Matrix> {
    let numbers: Vec<f64> = operands.iter().map(number).collect::<Option<_>>()?;
    numbers.try_into().ok()
}

struct Walk<'a, 'b> {
    doc: &'a Document,
    page: u32,
    placed: &'b mut BTreeMap<ObjectId, Placed>,
    /// The forms being drawn, so one that draws itself stops there.
    seen: HashSet<ObjectId>,
    operations: usize,
}

impl Walk<'_, '_> {
    fn drawing(
        &mut self,
        content: &[u8],
        resources: Option<&Dictionary>,
        start: Matrix,
        depth: usize,
    ) {
        let Ok(content) = Content::decode(content) else {
            return;
        };
        let xobjects = resources
            .and_then(|resources| resources.get(b"XObject").ok())
            .and_then(|xobjects| dict_of(self.doc, xobjects))
            .cloned();
        let mut stack: Vec<Matrix> = Vec::new();
        let mut ctm = start;
        for operation in content.operations {
            self.operations += 1;
            if self.operations > OPERATIONS {
                return;
            }
            match operation.operator.as_str() {
                "q" => stack.push(ctm),
                "Q" => ctm = stack.pop().unwrap_or(start),
                "cm" => {
                    if let Some(m) = matrix(&operation.operands) {
                        ctm = then(m, ctm);
                    }
                }
                "Do" => {
                    let Some(name) = operation.operands.first().and_then(|o| o.as_name().ok())
                    else {
                        continue;
                    };
                    let Some(id) = xobjects
                        .as_ref()
                        .and_then(|xobjects| xobjects.get(name).ok())
                        .and_then(|object| object.as_reference().ok())
                    else {
                        continue;
                    };
                    self.drawn(id, ctm, resources, depth);
                }
                _ => {}
            }
        }
    }

    /// The object `id` drawn at `ctm`: an image placed, a form followed.
    fn drawn(&mut self, id: ObjectId, ctm: Matrix, outer: Option<&Dictionary>, depth: usize) {
        let Ok(Object::Stream(stream)) = self.doc.get_object(id) else {
            return;
        };
        match stream.dict.get(b"Subtype").and_then(Object::as_name) {
            Ok(b"Image") => {
                // The unit square drawn at `ctm`: its sides' lengths, in
                // points, a 72nd of an inch.
                let width = ctm[0].hypot(ctm[1]) / 72.0;
                let height = ctm[2].hypot(ctm[3]) / 72.0;
                let placed = self.placed.entry(id).or_default();
                placed.width = placed.width.max(width);
                placed.height = placed.height.max(height);
                placed.pages.insert(self.page);
            }
            Ok(b"Form") if depth < DEPTH && self.seen.insert(id) => {
                let form = stream
                    .dict
                    .get(b"Matrix")
                    .ok()
                    .and_then(|m| m.as_array().ok())
                    .and_then(|m| matrix(m))
                    .unwrap_or([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
                let own = stream
                    .dict
                    .get(b"Resources")
                    .ok()
                    .and_then(|resources| dict_of(self.doc, resources))
                    .cloned();
                // A form without resources of its own uses its page's.
                let resources = own.as_ref().or(outer);
                let content = stream
                    .decompressed_content()
                    .unwrap_or_else(|_| stream.content.clone());
                self.drawing(&content, resources, then(form, ctm), depth + 1);
                self.seen.remove(&id);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transforms_compose_as_a_page_draws_them() {
        // Scaled by 2, then moved by (10, 20): a point at 1 goes to 12.
        let scaled = [2.0, 0.0, 0.0, 2.0, 0.0, 0.0];
        let moved = [1.0, 0.0, 0.0, 1.0, 10.0, 20.0];
        let both = then(scaled, moved);
        assert_eq!(both, [2.0, 0.0, 0.0, 2.0, 10.0, 20.0]);
        let placed = Placed {
            width: 2.0,
            height: 1.0,
            pages: BTreeSet::new(),
        };
        assert_eq!(placed.dpi(600, 300), Some(300.0));
        assert_eq!(placed.dpi(600, 600), Some(300.0));
    }
}
