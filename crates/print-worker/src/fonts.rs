//! The fonts a job is written in: its faces by role, read from the files
//! it names, and the faces of its fallback folders that hold what those
//! lack. Each face is registered with parley's collection under a family
//! of its own, so a run names the very face it wants and parley never
//! picks among faces itself; a face missing (no italic) is the nearest
//! one, slanted or emboldened as parley says. Nothing of the system's
//! font discovery is asked. A face whose licence forbids embedding it
//! (its OS/2 `fsType`) is passed over: the PDF holds every face it
//! writes in, subset.

use std::borrow::Cow;
use std::collections::{BTreeSet, HashMap};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use parley::fontique::{Blob, FontInfoOverride, Synthesis};
use parley::{
    FontContext, FontData, FontFamily, FontFamilyName, FontStyle, FontWeight, LayoutContext,
};
use skrifa::raw::tables::cmap::Cmap;
use skrifa::raw::{FontData as RawData, FontRead, TableProvider};
use skwir_core::print::{Face, Family, Job};

use crate::text::Ink;

/// The largest font file read: a collection of CJK faces is some tens of
/// megabytes.
const FONT_LIMIT: u64 = 256 * 1024 * 1024;
/// The most bytes of fallback faces taken in, and of font files looked
/// into for them: a folder of every script's fonts is not read whole.
const FALLBACK_LIMIT: usize = 192 * 1024 * 1024;
const FALLBACK_FILES: usize = 20_000;
const FALLBACK_DEPTH: usize = 8;
/// The most characters looked for in the fallback folders: past them, a
/// text of every script there is draws the rest as the faces found have
/// them, rather than read each font file's map for each.
const LOOKED_FOR: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Role {
    Text,
    Code,
    /// The face a specimen shows.
    Specimen,
}

/// The four faces of a family, as indexes: regular, bold, italic, bold
/// italic.
const FACES: usize = 4;

fn face_index(bold: bool, italic: bool) -> usize {
    usize::from(bold) + 2 * usize::from(italic)
}

/// A face read, as one font (a face of a collection copied out).
pub struct Loaded {
    pub data: Arc<Vec<u8>>,
    /// The family registered for it.
    pub family: String,
}

pub struct Fonts {
    pub cx: FontContext,
    /// parley's scratch space, kept from one layout to the next.
    pub lcx: LayoutContext<Ink>,
    pub faces: Faces,
    /// Fonts for krilla, by the face's data, index and variation.
    krilla: HashMap<(u64, u32, Vec<(u32, u32)>), Option<krilla::text::Font>>,
}

/// The faces registered, apart from the context so a layout can borrow
/// both.
pub struct Faces {
    roles: HashMap<Role, [Option<Loaded>; FACES]>,
    fallbacks: Vec<Loaded>,
    /// The families each role asks, by weight and slant (`stack`).
    stacks: HashMap<(Role, bool, bool), Vec<FontFamilyName<'static>>>,
    /// Characters no face holds, the fallback folders looked into.
    pub missing: usize,
}

impl Fonts {
    /// The job's faces read and registered, then the fallbacks for the
    /// characters of `text` they lack.
    pub fn load(job: &Job, text: &BTreeSet<char>) -> Result<Fonts> {
        let mut cx = FontContext::default();
        let mut roles = HashMap::new();
        let mut serial = 0;
        for (role, family) in [(Role::Text, &job.fonts.text), (Role::Code, &job.fonts.code)] {
            roles.insert(role, load_family(&mut cx, family, role, &mut serial)?);
        }
        let any = |role: Role, roles: &HashMap<Role, [Option<Loaded>; FACES]>| {
            roles[&role].iter().any(Option::is_some)
        };
        if !any(Role::Text, &roles) && !any(Role::Code, &roles) {
            bail!("the job names no font");
        }
        let mut faces = Faces {
            roles,
            fallbacks: Vec::new(),
            stacks: HashMap::new(),
            missing: 0,
        };
        let missing = faces.lacking(text);
        if !missing.is_empty() {
            faces.find_fallbacks(&mut cx, &job.fallback, missing);
        }
        faces.index();
        Ok(Fonts {
            cx,
            lcx: LayoutContext::new(),
            faces,
            krilla: HashMap::new(),
        })
    }

    /// The face a specimen shows, registered as its role.
    pub fn load_specimen(&mut self, face: &Face) -> Result<()> {
        let Some(loaded) = load_face(&mut self.cx, face, "skwir-specimen")? else {
            bail!(
                "the font {} may not be embedded in a document, as its licence says",
                face.path.display()
            );
        };
        self.faces
            .roles
            .insert(Role::Specimen, [Some(loaded), None, None, None]);
        self.faces.index();
        Ok(())
    }

    /// krilla's font for the face a run was shaped with, with the
    /// variation it was asked.
    pub fn krilla(&mut self, font: &FontData, synthesis: &Synthesis) -> Option<krilla::text::Font> {
        let variation: Vec<(u32, u32)> = synthesis
            .variation_settings()
            .iter()
            .map(|(tag, value)| (u32::from_be_bytes(tag.to_be_bytes()), value.to_bits()))
            .collect();
        let key = (font.data.id(), font.index, variation);
        self.krilla
            .entry(key)
            .or_insert_with(|| {
                let (bytes, _) = font.data.clone().into_raw_parts();
                let data = krilla::Data::from(bytes);
                let settings = synthesis.variation_settings();
                if settings.is_empty() {
                    krilla::text::Font::new(data, font.index)
                } else {
                    let coords: Vec<(krilla::text::Tag, f32)> = settings
                        .iter()
                        .map(|(tag, value)| (krilla::text::Tag::new(&tag.to_be_bytes()), *value))
                        .collect();
                    krilla::text::Font::new_variable(data, font.index, &coords)
                }
            })
            .clone()
    }

    /// Options for reading an SVG: its text in the job's faces, nothing
    /// read from outside it (an image it names by path is left out).
    pub fn svg_options(&self) -> usvg::Options<'static> {
        let mut db = usvg::fontdb::Database::new();
        let name = |role: Role, db: &mut usvg::fontdb::Database| -> Option<String> {
            let mut first = None;
            for loaded in self.faces.roles.get(&role)?.iter().flatten() {
                let ids = db.load_font_source(usvg::fontdb::Source::Binary(loaded.data.clone()));
                if first.is_none() {
                    first = ids
                        .first()
                        .and_then(|id| db.face(*id))
                        .and_then(|face| face.families.first())
                        .map(|(name, _)| name.clone());
                }
            }
            first
        };
        let text = name(Role::Text, &mut db);
        let code = name(Role::Code, &mut db);
        for loaded in &self.faces.fallbacks {
            db.load_font_source(usvg::fontdb::Source::Binary(loaded.data.clone()));
        }
        let text = text.or_else(|| code.clone()).unwrap_or_default();
        db.set_serif_family(text.clone());
        db.set_sans_serif_family(text.clone());
        db.set_cursive_family(text.clone());
        db.set_fantasy_family(text.clone());
        db.set_monospace_family(code.unwrap_or_else(|| text.clone()));
        let mut options = usvg::Options {
            font_family: text,
            ..usvg::Options::default()
        };
        options.resources_dir = None;
        options.image_href_resolver = usvg::ImageHrefResolver {
            resolve_data: usvg::ImageHrefResolver::default_data_resolver(),
            resolve_string: Box::new(|_, _| None),
        };
        options.fontdb = Arc::new(db);
        options
    }
}

impl Faces {
    /// The families a run of `role` asks, the face it wants first, then
    /// the other role's regular face and the fallbacks; and the weight and
    /// style asked of it, so a face standing in for one missing is
    /// emboldened or slanted.
    pub fn stack(
        &self,
        role: Role,
        bold: bool,
        italic: bool,
    ) -> (FontFamily<'_>, FontWeight, FontStyle) {
        let names = self
            .stacks
            .get(&(role, bold, italic))
            .map_or(&[][..], Vec::as_slice);
        let weight = if bold {
            FontWeight::BOLD
        } else {
            FontWeight::NORMAL
        };
        let style = if italic {
            FontStyle::Italic
        } else {
            FontStyle::Normal
        };
        (FontFamily::List(Cow::Borrowed(names)), weight, style)
    }

    /// Every stack worked out once, as faces are registered: a layout asks
    /// one for each of its stretches.
    fn index(&mut self) {
        let mut stacks = HashMap::new();
        for role in [Role::Text, Role::Code, Role::Specimen] {
            for (bold, italic) in [(false, false), (true, false), (false, true), (true, true)] {
                let mut names: Vec<FontFamilyName<'static>> = Vec::new();
                let mut push = |name: &str| {
                    let name = FontFamilyName::Named(Cow::Owned(name.to_owned()));
                    if !names.contains(&name) {
                        names.push(name);
                    }
                };
                if let Some(face) = self.nearest(role, bold, italic) {
                    push(&face.family);
                }
                for other in [Role::Text, Role::Code] {
                    if let Some(face) = self.nearest(other, bold, italic) {
                        push(&face.family);
                    }
                }
                for fallback in &self.fallbacks {
                    push(&fallback.family);
                }
                stacks.insert((role, bold, italic), names);
            }
        }
        self.stacks = stacks;
    }

    /// The face of `role` nearest to the one asked: itself, else one with
    /// its slant, else one with its weight, else the regular, else any.
    fn nearest(&self, role: Role, bold: bool, italic: bool) -> Option<&Loaded> {
        let faces = self.roles.get(&role)?;
        let order = [
            face_index(bold, italic),
            face_index(false, italic),
            face_index(bold, false),
            face_index(false, false),
        ];
        order
            .iter()
            .find_map(|&index| faces[index].as_ref())
            .or_else(|| faces.iter().flatten().next())
    }

    /// The characters of `text` no regular face of the roles holds
    /// (spaces and marks that draw nothing left out).
    fn lacking(&self, text: &BTreeSet<char>) -> BTreeSet<char> {
        let maps: Vec<Coverage> = [Role::Text, Role::Code]
            .iter()
            .filter_map(|role| self.nearest(*role, false, false))
            .flat_map(|face| coverage(&face.data))
            .collect();
        let maps: Vec<Cmap> = maps.iter().filter_map(Coverage::cmap).collect();
        text.iter()
            .copied()
            .filter(|&c| draws(c))
            .filter(|&c| !maps.iter().any(|cmap| holds(cmap, c)))
            .collect()
    }

    /// Faces of `folders` holding the characters `missing`, registered in
    /// turn until none is missing or the folders are looked through,
    /// regular faces before others. Only their character maps are read
    /// until one is taken.
    fn find_fallbacks(
        &mut self,
        cx: &mut FontContext,
        folders: &[PathBuf],
        missing: BTreeSet<char>,
    ) {
        let mut files = Vec::new();
        for folder in folders {
            walk(folder, 0, &mut files);
            if files.len() >= FALLBACK_FILES {
                break;
            }
        }
        files.sort_by_cached_key(|path| (rank(path), path.clone()));
        files.dedup();
        // The characters looked for, a bounded number: each font file's
        // map is read once, and asked of each.
        let beyond = missing.len().saturating_sub(LOOKED_FOR);
        let mut missing: BTreeSet<char> = missing.into_iter().take(LOOKED_FOR).collect();
        let mut taken = 0usize;
        let mut serial = 0;
        for path in files {
            if missing.is_empty() || taken >= FALLBACK_LIMIT {
                break;
            }
            let Ok(faces) = file_coverage(&path) else {
                continue;
            };
            for (index, face) in faces.iter().enumerate() {
                let Some(cmap) = face.cmap() else {
                    continue;
                };
                let holds: Vec<char> = missing
                    .iter()
                    .copied()
                    .filter(|&c| holds(&cmap, c))
                    .collect();
                if holds.is_empty() {
                    continue;
                }
                let name = format!("skwir-fallback-{serial}");
                let face = Face {
                    path: path.clone(),
                    index: index as u32,
                };
                // One that cannot be read, or may not be embedded: the
                // next file.
                let Ok(Some(loaded)) = load_face(cx, &face, &name) else {
                    break;
                };
                serial += 1;
                taken += loaded.data.len();
                for c in holds {
                    missing.remove(&c);
                }
                self.fallbacks.push(loaded);
                break;
            }
        }
        self.missing = missing.len() + beyond;
    }
}

/// Whether a character draws something a font must hold, worth looking
/// for in the fallback folders: not a space, nor one a shaper hides
/// (controls, formats such as joiners and bidirectional marks, variation
/// selectors and the few other default-ignorable ones), nor one no font
/// is meant to hold (private use, unassigned, a surrogate).
fn draws(c: char) -> bool {
    use unicode_general_category::{GeneralCategory as G, get_general_category};
    if c.is_whitespace() {
        return false;
    }
    let ignorable = matches!(
        c,
        '\u{034f}'
            | '\u{115f}'..='\u{1160}'
            | '\u{17b4}'..='\u{17b5}'
            | '\u{180b}'..='\u{180f}'
            | '\u{3164}'
            | '\u{fe00}'..='\u{fe0f}'
            | '\u{ffa0}'
            | '\u{e0100}'..='\u{e01ef}'
    );
    !ignorable
        && !matches!(
            get_general_category(c),
            G::Control | G::Format | G::PrivateUse | G::Unassigned | G::Surrogate
        )
}

/// Font files below `folder`, a few levels down, links followed.
fn walk(folder: &Path, depth: usize, files: &mut Vec<PathBuf>) {
    if depth > FALLBACK_DEPTH || files.len() >= FALLBACK_FILES {
        return;
    }
    let Ok(entries) = std::fs::read_dir(folder) else {
        return;
    };
    let mut entries: Vec<PathBuf> = entries.flatten().map(|entry| entry.path()).collect();
    entries.sort();
    for path in entries {
        if files.len() >= FALLBACK_FILES {
            return;
        }
        let is_font = path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| {
                ["ttf", "otf", "ttc", "otc"]
                    .iter()
                    .any(|known| ext.eq_ignore_ascii_case(known))
            });
        if is_font {
            files.push(path);
        } else if path.is_dir() {
            walk(&path, depth + 1, files);
        }
    }
}

/// Regular faces first: a file whose name says another weight or slant
/// is looked into after them.
fn rank(path: &Path) -> u8 {
    let name = path
        .file_stem()
        .map(|name| name.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let other = [
        "bold",
        "italic",
        "oblique",
        "light",
        "thin",
        "black",
        "heavy",
        "medium",
        "semi",
        "extra",
        "condensed",
        "narrow",
        "mono",
    ]
    .iter()
    .any(|word| name.contains(word));
    u8::from(other)
}

/// A face's character map, as read from its `cmap` table.
struct Coverage {
    cmap: Vec<u8>,
}

impl Coverage {
    /// The map read, once for all the characters asked of it.
    fn cmap(&self) -> Option<Cmap<'_>> {
        Cmap::read(RawData::new(&self.cmap)).ok()
    }
}

/// Whether `cmap` gives `c` a glyph.
fn holds(cmap: &Cmap, c: char) -> bool {
    cmap.map_codepoint(c)
        .is_some_and(|glyph| glyph.to_u32() != 0)
}

/// The character maps of a font's faces, from its data.
fn coverage(data: &[u8]) -> Vec<Coverage> {
    let mut reader = std::io::Cursor::new(data);
    read_coverage(&mut reader).unwrap_or_default()
}

/// The character maps of the faces of the font file at `path`, reading
/// only its directories and maps.
fn file_coverage(path: &Path) -> Result<Vec<Coverage>> {
    let mut file = File::open(path)?;
    read_coverage(&mut file)
}

fn read_coverage<R: Read + Seek>(reader: &mut R) -> Result<Vec<Coverage>> {
    let read_at = |reader: &mut R, at: u64, len: usize| -> Result<Vec<u8>> {
        reader.seek(SeekFrom::Start(at))?;
        let mut buffer = vec![0; len];
        reader.read_exact(&mut buffer)?;
        Ok(buffer)
    };
    let head = read_at(reader, 0, 12)?;
    let offsets: Vec<u64> = if &head[..4] == b"ttcf" {
        let count = be32(&head, 8).min(256) as usize;
        let table = read_at(reader, 12, count * 4)?;
        (0..count).map(|i| u64::from(be32(&table, i * 4))).collect()
    } else {
        vec![0]
    };
    let mut faces = Vec::new();
    for offset in offsets {
        let directory = read_at(reader, offset, 12)?;
        let tables = usize::from(be16(&directory, 4)).min(512);
        let records = read_at(reader, offset + 12, tables * 16)?;
        let cmap = (0..tables)
            .map(|i| &records[i * 16..i * 16 + 16])
            .find(|record| &record[..4] == b"cmap")
            .map(|record| (be32(record, 8), be32(record, 12)));
        let cmap = match cmap {
            Some((at, len)) if len <= 16 * 1024 * 1024 => {
                read_at(reader, u64::from(at), len as usize)?
            }
            _ => Vec::new(),
        };
        faces.push(Coverage { cmap });
    }
    Ok(faces)
}

fn be16(data: &[u8], at: usize) -> u16 {
    data.get(at..at + 2)
        .map_or(0, |bytes| u16::from_be_bytes([bytes[0], bytes[1]]))
}

fn be32(data: &[u8], at: usize) -> u32 {
    data.get(at..at + 4).map_or(0, |bytes| {
        u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
    })
}

fn load_family(
    cx: &mut FontContext,
    family: &Family,
    role: Role,
    serial: &mut usize,
) -> Result<[Option<Loaded>; FACES]> {
    let mut faces: [Option<Loaded>; FACES] = Default::default();
    let named = [
        (&family.regular, false, false),
        (&family.bold, true, false),
        (&family.italic, false, true),
        (&family.bold_italic, true, true),
    ];
    for (face, bold, italic) in named {
        if let Some(face) = face {
            *serial += 1;
            let name = format!("skwir-{role:?}-{serial}").to_lowercase();
            // One that may not be embedded is as if the family lacked it.
            faces[face_index(bold, italic)] = load_face(cx, face, &name)?;
        }
    }
    Ok(faces)
}

/// A face read and registered as `family`; `None` for one whose licence
/// forbids embedding it.
fn load_face(cx: &mut FontContext, face: &Face, family: &str) -> Result<Option<Loaded>> {
    let data = read_font(face)
        .with_context(|| format!("the font {} could not be read", face.path.display()))?;
    if !embeddable(&data) {
        return Ok(None);
    }
    let data = Arc::new(data);
    let registered = cx.collection.register_fonts(
        Blob::new(data.clone()),
        Some(FontInfoOverride {
            family_name: Some(family),
            ..FontInfoOverride::default()
        }),
    );
    if registered.is_empty() {
        bail!("{} holds no font", face.path.display());
    }
    Ok(Some(Loaded {
        data,
        family: family.to_owned(),
    }))
}

/// Whether the font `data` may be embedded in a document, as its OS/2
/// `fsType` says: not when its usage is restricted (its low four bits
/// 2 alone, the least restrictive of several bits winning), nor when only
/// its bitmaps may be (bit 9). A font without the table may.
fn embeddable(data: &[u8]) -> bool {
    let Some(fs_type) = skrifa::FontRef::new(data)
        .ok()
        .and_then(|font| font.os2().ok())
        .map(|os2| os2.fs_type())
    else {
        return true;
    };
    fs_type & 0x000f != 0x0002 && fs_type & 0x0200 == 0
}

/// The face's font: the file itself, or its face copied out of a
/// collection.
fn read_font(face: &Face) -> Result<Vec<u8>> {
    let file = File::open(&face.path)?;
    let size = file.metadata()?.len();
    if size > FONT_LIMIT {
        bail!("{size} bytes, more than a font is read at");
    }
    let mut data = Vec::with_capacity(size as usize);
    file.take(FONT_LIMIT).read_to_end(&mut data)?;
    if data.starts_with(b"ttcf") {
        return face_of_collection(&data, face.index)
            .with_context(|| format!("no face {} in it", face.index));
    }
    Ok(data)
}

/// Face `index` of a font collection, as a font of its own: its table
/// directory with the tables it names after it.
pub fn face_of_collection(data: &[u8], index: u32) -> Option<Vec<u8>> {
    let count = be32(data, 8);
    if index >= count {
        return None;
    }
    let offset = be32(data, 12 + 4 * index as usize) as usize;
    let header = data.get(offset..offset + 12)?;
    let tables = usize::from(be16(header, 4));
    let records = data.get(offset + 12..offset + 12 + tables * 16)?;
    let mut out = Vec::new();
    out.extend_from_slice(header);
    let mut at = 12 + tables * 16;
    let mut placed = Vec::with_capacity(tables);
    for i in 0..tables {
        let record = &records[i * 16..i * 16 + 16];
        let (start, len) = (be32(record, 8) as usize, be32(record, 12) as usize);
        let table = data.get(start..start.checked_add(len)?)?;
        placed.push((record, at, table));
        at = at.checked_add(len.div_ceil(4) * 4)?;
        // A face's tables are parts of the file apart: ones that add up
        // to more (records naming one stretch again and again) are no
        // face, and would be copied out as many times.
        if at > data.len() + 16 * tables + 12 {
            return None;
        }
    }
    for (record, at, table) in &placed {
        out.extend_from_slice(&record[..8]);
        out.extend_from_slice(&(*at as u32).to_be_bytes());
        out.extend_from_slice(&(table.len() as u32).to_be_bytes());
    }
    for (_, _, table) in placed {
        out.extend_from_slice(table);
        out.resize(out.len().div_ceil(4) * 4, 0);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A collection of two made-up faces, each a directory of one table.
    fn collection() -> Vec<u8> {
        let mut data = b"ttcf".to_vec();
        data.extend_from_slice(&0x0001_0000u32.to_be_bytes());
        data.extend_from_slice(&2u32.to_be_bytes());
        data.extend_from_slice(&20u32.to_be_bytes());
        data.extend_from_slice(&48u32.to_be_bytes());
        // Two directories of one table each, at 20 and 48.
        for table_at in [76u32, 80] {
            data.extend_from_slice(&0x0001_0000u32.to_be_bytes());
            data.extend_from_slice(&1u16.to_be_bytes());
            data.extend_from_slice(&[0; 6]);
            data.extend_from_slice(b"name");
            data.extend_from_slice(&0u32.to_be_bytes());
            data.extend_from_slice(&table_at.to_be_bytes());
            data.extend_from_slice(&3u32.to_be_bytes());
        }
        data.extend_from_slice(b"one\0two\0");
        data
    }

    #[test]
    fn a_face_of_a_collection_becomes_a_font_of_its_own() {
        let data = collection();
        let second = face_of_collection(&data, 1).unwrap();
        assert_eq!(be16(&second, 4), 1);
        assert_eq!(&second[12..16], b"name");
        let at = be32(&second, 20) as usize;
        assert_eq!(at, 28);
        assert_eq!(&second[at..at + 3], b"two");
        assert_eq!(second.len() % 4, 0);
        assert!(face_of_collection(&data, 2).is_none());
    }

    #[test]
    fn a_collection_whose_tables_add_up_to_more_than_it_is_refused() {
        let mut data = collection();
        // The second face's one table made the whole file, named twice.
        data[52..54].copy_from_slice(&2u16.to_be_bytes());
        let whole = u32::try_from(data.len()).unwrap();
        let mut records = Vec::new();
        for _ in 0..2 {
            records.extend_from_slice(b"name");
            records.extend_from_slice(&0u32.to_be_bytes());
            records.extend_from_slice(&0u32.to_be_bytes());
            records.extend_from_slice(&whole.to_be_bytes());
        }
        data.splice(60..76, records);
        assert!(face_of_collection(&data, 1).is_none());
        assert!(face_of_collection(&collection(), 1).is_some());
    }

    #[test]
    fn only_characters_that_draw_need_a_font() {
        assert!(draws('a') && draws('漢') && draws('↪') && draws('\u{0301}'));
        assert!(!draws(' ') && !draws('\u{200d}') && !draws('\u{fe0f}') && !draws('\n'));
        // Bidirectional marks and isolates, private use, unassigned.
        assert!(!draws('\u{202b}') && !draws('\u{2067}') && !draws('\u{061c}'));
        assert!(!draws('\u{e000}') && !draws('\u{10fffd}') && !draws('\u{40000}'));
    }

    /// A font of nothing but an OS/2 table saying `fs_type`.
    fn with_fs_type(fs_type: u16) -> Vec<u8> {
        let mut os2 = vec![0u8; 78];
        os2[8..10].copy_from_slice(&fs_type.to_be_bytes());
        let mut data = 0x0001_0000u32.to_be_bytes().to_vec();
        data.extend_from_slice(&1u16.to_be_bytes());
        data.extend_from_slice(&[0; 6]);
        data.extend_from_slice(b"OS/2");
        data.extend_from_slice(&0u32.to_be_bytes());
        data.extend_from_slice(&28u32.to_be_bytes());
        data.extend_from_slice(&(os2.len() as u32).to_be_bytes());
        data.extend_from_slice(&os2);
        data
    }

    #[test]
    fn a_font_whose_licence_forbids_embedding_is_passed_over() {
        assert!(embeddable(&with_fs_type(0)));
        assert!(embeddable(&with_fs_type(0x0004)), "preview and print");
        assert!(
            embeddable(&with_fs_type(0x0006)),
            "the least restrictive wins"
        );
        assert!(!embeddable(&with_fs_type(0x0002)), "restricted");
        assert!(!embeddable(&with_fs_type(0x0200)), "bitmaps only");
        assert!(embeddable(b"not a font"));
    }

    #[test]
    fn regular_faces_are_looked_into_first() {
        assert!(rank(Path::new("NotoSans-Regular.ttf")) < rank(Path::new("NotoSans-Bold.ttf")));
    }
}
