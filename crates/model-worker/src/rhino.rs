//! Rhino models (`.3dm`): the meshes the file stores. A Rhino model holds
//! exact surfaces; Rhino keeps a mesh of each B-rep and extrusion it showed
//! shaded and writes it with the file (Save Small leaves them out, and a
//! `SubD`'s is never written). Mesh objects and those render meshes are read,
//! placed by block instances, in the colours of their vertices, their faces
//! or their materials, as Rhino's rendered view shows them (a layer's
//! display colour, black by default, is for wires). Everything else is
//! passed over by its chunk's length. Laid out as openNURBS 8 reads it
//! (`ON_BinaryArchive`), from archives of Rhino 2 to 8; Rhino 1's, and Rhino
//! 2's own B-reps, are not read.
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::Path;

use crate::mesh::{Colour, IDENTITY, Mesh, Point, Transform, compose};

const SHORT: u32 = 0x8000_0000;
const COMMENT: u32 = 0x0000_0001;
const END_OF_FILE: u32 = 0x0000_7FFF;
const END_OF_TABLE: u32 = 0xFFFF_FFFF;
const MATERIAL_TABLE: u32 = 0x1000_0010;
const LAYER_TABLE: u32 = 0x1000_0011;
const OBJECT_TABLE: u32 = 0x1000_0013;
const PROPERTIES_TABLE: u32 = 0x1000_0014;
const SETTINGS_TABLE: u32 = 0x1000_0015;
const DEFINITION_TABLE: u32 = 0x1000_0021;
const OPENNURBS_VERSION: u32 = 0xA000_0026;
const UNITS: u32 = 0x2000_8031;
const MATERIAL_RECORD: u32 = 0x2000_8040;
const LAYER_RECORD: u32 = 0x2000_8050;
const OBJECT_RECORD: u32 = 0x2000_8070;
const DEFINITION_RECORD: u32 = 0x2000_8076;
const ATTRIBUTES: u32 = 0x0200_8072;
const RECORD_END: u32 = 0x8200_007F;
const CLASS: u32 = 0x0002_7FFA;
const CLASS_ID: u32 = 0x0002_FFFB;
const CLASS_DATA: u32 = 0x0002_FFFC;
const ANONYMOUS: u32 = 0x4000_8000;
const COMPONENT: u32 = 0x4000_8002;

/// A class's id as openNURBS writes a UUID: its first three fields little-
/// endian, then its last eight bytes.
const fn id(a: u32, b: u16, c: u16, d: [u8; 8]) -> [u8; 16] {
    let (a, b, c) = (a.to_le_bytes(), b.to_le_bytes(), c.to_le_bytes());
    [
        a[0], a[1], a[2], a[3], b[0], b[1], c[0], c[1], d[0], d[1], d[2], d[3], d[4], d[5], d[6],
        d[7],
    ]
}

const MESH: [u8; 16] = id(
    0x4ED7_D4E4,
    0xE947,
    0x11D3,
    [0xBF, 0xE5, 0x00, 0x10, 0x83, 0x01, 0x22, 0xF0],
);
/// `ON_Brep`, and the ids openNURBS reads as one: Rhino 7's and 8's
/// (`TL_Brep`), and two of Rhino 2's.
const BREPS: [[u8; 16]; 4] = [
    id(
        0x60B5_DBC5,
        0xE660,
        0x11D3,
        [0xBF, 0xE4, 0x00, 0x10, 0x83, 0x01, 0x22, 0xF0],
    ),
    id(
        0xF06F_C243,
        0xA32A,
        0x4608,
        [0x9D, 0xD8, 0xA7, 0xD2, 0xC4, 0xCE, 0x2A, 0x36],
    ),
    id(
        0x0705_FDEF,
        0x3E2A,
        0x11D4,
        [0x80, 0x0E, 0x00, 0x10, 0x83, 0x01, 0x22, 0xF0],
    ),
    id(
        0x2D4C_FEDB,
        0x3E2A,
        0x11D4,
        [0x80, 0x0E, 0x00, 0x10, 0x83, 0x01, 0x22, 0xF0],
    ),
];
const EXTRUSION: [u8; 16] = id(
    0x36F5_3175,
    0x72B8,
    0x4D47,
    [0xBF, 0x1F, 0xB4, 0xE6, 0xFC, 0x24, 0xF4, 0xB9],
);
const SUBD: [u8; 16] = id(
    0xF09B_A4D9,
    0x455B,
    0x42C3,
    [0xBA, 0x3B, 0xE6, 0xCC, 0xAC, 0xEF, 0x85, 0x3B],
);
const INSTANCE: [u8; 16] = id(
    0xF9CF_B638,
    0xB9D4,
    0x4340,
    [0x87, 0xE3, 0xC5, 0x6E, 0x78, 0x65, 0xD9, 0x6A],
);
/// The render mesh among an extrusion's cached meshes.
const RENDER_MESH: [u8; 16] = id(
    0xF5E3_BAA9,
    0xA7A2,
    0x49FD,
    [0xB8, 0xA1, 0x66, 0xEB, 0x27, 0x4A, 0x5F, 0x91],
);
/// openNURBS's unset colour.
const UNSET: [u8; 4] = [0xFF; 4];
/// Block instances followed within one another at most.
const NESTING: usize = 32;

/// The lengths a file may state, by `ON::LengthUnitSystem`.
fn unit(system: i32) -> Option<&'static str> {
    Some(match system {
        1 => "µm",
        2 => "mm",
        3 => "cm",
        4 => "m",
        5 => "km",
        7 => "mil",
        8 => "in",
        9 => "ft",
        10 => "mi",
        13 => "nm",
        14 => "dm",
        19 => "yd",
        _ => return None,
    })
}

/// The model at `path`, added to `mesh`; how many of its objects store no
/// mesh and are not drawn.
pub fn read(path: &Path, most_bytes: u64, mesh: &mut Mesh) -> Result<usize, String> {
    let file = File::open(path).map_err(|e| format!("opening the file: {e}"))?;
    let len = file.metadata().map_err(|e| e.to_string())?.len();
    if len > most_bytes {
        return Err(format!("the file is larger than {most_bytes} bytes"));
    }
    let mut archive = Archive {
        file: BufReader::new(file),
        len,
        wide: false,
    };
    let mut head = [0u8; 32];
    archive
        .file
        .read_exact(&mut head)
        .map_err(|_| "not a Rhino model")?;
    let version: u32 = head
        .strip_prefix(b"3D Geometry File Format ")
        .and_then(|digits| std::str::from_utf8(digits).ok())
        .and_then(|digits| digits.trim().parse().ok())
        .ok_or("not a Rhino model")?;
    if version < 2 {
        return Err("Rhino 1 models are not read".into());
    }
    archive.wide = version >= 50;
    let comment = archive.chunk()?;
    if comment.code != COMMENT {
        return Err("not a Rhino model".into());
    }
    archive.skip(&comment)?;
    let mut model = Model {
        version,
        ..Model::default()
    };
    let most = mesh.most();
    loop {
        let table = archive.chunk()?;
        match table.code {
            END_OF_FILE => break,
            PROPERTIES_TABLE | SETTINGS_TABLE | MATERIAL_TABLE | LAYER_TABLE | DEFINITION_TABLE
            | OBJECT_TABLE => {
                while archive.position()? < table.end {
                    let record = archive.chunk()?;
                    if record.code == END_OF_TABLE {
                        break;
                    }
                    if record.short {
                        if record.code == OPENNURBS_VERSION {
                            model.opennurbs =
                                u32::try_from(record.value & 0xFFFF_FFFF).unwrap_or_default();
                        }
                        continue;
                    }
                    let wanted = matches!(
                        record.code,
                        UNITS | MATERIAL_RECORD | LAYER_RECORD | DEFINITION_RECORD | OBJECT_RECORD
                    );
                    if !wanted {
                        archive.skip(&record)?;
                        continue;
                    }
                    let bytes = archive.payload(&record)?;
                    let mut at = Cursor {
                        bytes: &bytes,
                        at: 0,
                        wide: archive.wide,
                    };
                    model.record(record.code, &mut at, most)?;
                }
                archive.seek(table.end)?;
                // Nothing a mesh needs comes after the objects.
                if table.code == OBJECT_TABLE {
                    break;
                }
            }
            _ if table.short => {}
            _ => archive.skip(&table)?,
        }
        if archive.position()? >= archive.len {
            break;
        }
    }
    mesh.unit = model.unit;
    model.draw(mesh)
}

/// A chunk's typecode and, for a long chunk, where its bytes end.
struct Chunk {
    code: u32,
    short: bool,
    /// A short chunk's value; a long one's length.
    value: i64,
    end: u64,
}

/// The file, read a chunk at a time: what is not needed is skipped by its
/// length, never read.
struct Archive {
    file: BufReader<File>,
    len: u64,
    /// Whether a chunk's value is 8 bytes (archives of Rhino 5 on) or 4.
    wide: bool,
}

impl Archive {
    fn position(&mut self) -> Result<u64, String> {
        self.file.stream_position().map_err(|e| e.to_string())
    }

    fn seek(&mut self, to: u64) -> Result<(), String> {
        self.file
            .seek(SeekFrom::Start(to))
            .map(drop)
            .map_err(|e| e.to_string())
    }

    fn chunk(&mut self) -> Result<Chunk, String> {
        let mut head = [0u8; 12];
        let width = if self.wide { 12 } else { 8 };
        self.file
            .read_exact(&mut head[..width])
            .map_err(|_| "the model ends early")?;
        let code = u32::from_le_bytes([head[0], head[1], head[2], head[3]]);
        let value = if self.wide {
            i64::from_le_bytes(head[4..12].try_into().unwrap_or_default())
        } else {
            i64::from(u32::from_le_bytes([head[4], head[5], head[6], head[7]]))
        };
        let short = code & SHORT != 0;
        let at = self.position()?;
        let end = if short {
            at
        } else {
            u64::try_from(value)
                .ok()
                .and_then(|length| at.checked_add(length))
                .filter(|end| *end <= self.len)
                .ok_or("the model is damaged: a chunk ends past the file")?
        };
        Ok(Chunk {
            code,
            short,
            value,
            end,
        })
    }

    fn skip(&mut self, chunk: &Chunk) -> Result<(), String> {
        self.seek(chunk.end)
    }

    fn payload(&mut self, chunk: &Chunk) -> Result<Vec<u8>, String> {
        let at = self.position()?;
        let length = usize::try_from(chunk.end - at).map_err(|e| e.to_string())?;
        let mut bytes = vec![0; length];
        self.file
            .read_exact(&mut bytes)
            .map_err(|_| "the model ends early")?;
        Ok(bytes)
    }
}

/// Bytes read in memory, the way openNURBS writes its values.
#[derive(Clone, Copy)]
struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
    wide: bool,
}

type Read3dm<T> = Result<T, String>;

fn ends() -> String {
    "an object ends early".into()
}

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> Read3dm<&'a [u8]> {
        let end = self.at.checked_add(n).ok_or_else(ends)?;
        let out = self.bytes.get(self.at..end).ok_or_else(ends)?;
        self.at = end;
        Ok(out)
    }

    fn u8(&mut self) -> Read3dm<u8> {
        Ok(self.take(1)?[0])
    }

    fn bool(&mut self) -> Read3dm<bool> {
        Ok(self.u8()? != 0)
    }

    fn u32(&mut self) -> Read3dm<u32> {
        Ok(u32::from_le_bytes(
            self.take(4)?.try_into().unwrap_or_default(),
        ))
    }

    fn i32(&mut self) -> Read3dm<i32> {
        Ok(i32::from_le_bytes(
            self.take(4)?.try_into().unwrap_or_default(),
        ))
    }

    fn f64(&mut self) -> Read3dm<f64> {
        Ok(f64::from_le_bytes(
            self.take(8)?.try_into().unwrap_or_default(),
        ))
    }

    fn uuid(&mut self) -> Read3dm<[u8; 16]> {
        Ok(self.take(16)?.try_into().unwrap_or_default())
    }

    fn colour(&mut self) -> Read3dm<[u8; 4]> {
        Ok(self.take(4)?.try_into().unwrap_or_default())
    }

    /// A one-byte version: its major and minor halves.
    fn version(&mut self) -> Read3dm<(u8, u8)> {
        let byte = self.u8()?;
        Ok((byte >> 4, byte & 15))
    }

    /// A counted array, its elements `size` bytes each, passed over.
    fn skip_array(&mut self, size: usize) -> Read3dm<()> {
        let count = self.u32()? as usize;
        self.take(count.checked_mul(size).ok_or_else(ends)?)?;
        Ok(())
    }

    /// A string of UTF-16 units, passed over.
    fn skip_string(&mut self) -> Read3dm<()> {
        self.skip_array(2)
    }

    /// The next chunk: its typecode and its bytes (none for a short one).
    fn chunk(&mut self) -> Read3dm<(u32, Cursor<'a>)> {
        let code = self.u32()?;
        let value = if self.wide {
            i64::from_le_bytes(self.take(8)?.try_into().unwrap_or_default())
        } else {
            i64::from(self.u32()?)
        };
        let length = if code & SHORT != 0 {
            0
        } else {
            usize::try_from(value).map_err(|_| ends())?
        };
        let bytes = self.take(length)?;
        Ok((
            code,
            Cursor {
                bytes,
                at: 0,
                wide: self.wide,
            },
        ))
    }

    /// A chunk that begins with two version numbers, as openNURBS's
    /// anonymous chunks do: the major one, and the bytes after both.
    fn versioned(&mut self, code: u32) -> Read3dm<(i32, i32, Cursor<'a>)> {
        let (found, mut inside) = self.chunk()?;
        if found != code {
            return Err(format!("a chunk {found:#x} where {code:#x} belongs"));
        }
        let major = inside.i32()?;
        let minor = inside.i32()?;
        Ok((major, minor, inside))
    }

    /// A serialized object (`ON_Object`): its class id and its data.
    fn object(&mut self) -> Read3dm<([u8; 16], Cursor<'a>)> {
        let (code, mut class) = self.chunk()?;
        if code != CLASS {
            return Err(format!("a chunk {code:#x} where an object belongs"));
        }
        let (code, mut id) = class.chunk()?;
        if code != CLASS_ID {
            return Err("an object without its class".into());
        }
        let id = id.uuid()?;
        if id == [0; 16] {
            return Ok((
                id,
                Cursor {
                    bytes: &[],
                    ..*self
                },
            ));
        }
        let (code, data) = class.chunk()?;
        if code != CLASS_DATA {
            return Err("an object without its data".into());
        }
        Ok((id, data))
    }

    /// A buffer of `expected` bytes, as `ReadCompressedBuffer` reads one:
    /// its size, its CRC, then its bytes or a chunk of zlib data. `None`
    /// when it is empty; passed over unread unless `wanted`.
    fn buffer(&mut self, expected: usize, wanted: bool) -> Read3dm<Option<Vec<u8>>> {
        let size = self.u32()? as usize;
        if size == 0 {
            return Ok(None);
        }
        if size != expected {
            return Err("a buffer is not the size its mesh says".into());
        }
        let _crc = self.u32()?;
        match self.u8()? {
            0 => {
                let bytes = self.take(size)?;
                Ok(wanted.then(|| bytes.to_vec()))
            }
            1 => {
                let (code, data) = self.chunk()?;
                if code != ANONYMOUS {
                    return Err("a compressed buffer without its data".into());
                }
                if !wanted {
                    return Ok(None);
                }
                // The chunk ends with its own CRC.
                let zlib = &data.bytes[..data.bytes.len().saturating_sub(4)];
                // Grown as it inflates: the size said is only a bound.
                let mut out = Vec::new();
                flate2::read::ZlibDecoder::new(zlib)
                    .take(size as u64)
                    .read_to_end(&mut out)
                    .map_err(|e| format!("a compressed buffer: {e}"))?;
                if out.len() != size {
                    return Err("a compressed buffer is shorter than it says".into());
                }
                Ok(Some(out))
            }
            _ => Err("a buffer compressed in an unknown way".into()),
        }
    }
}

/// A mesh as the file stores it, in the frame of its object.
struct Piece {
    positions: Vec<Point>,
    triangles: Vec<[u32; 3]>,
    /// Its vertices' colours, when it has them.
    colours: Option<Vec<Colour>>,
    /// The colour of the B-rep face it is the mesh of, when set.
    face: Option<Colour>,
}

enum Shape {
    Meshes(Vec<Piece>),
    /// A block instance: the definition placed, and where.
    Instance([u8; 16], Transform),
    /// A shape whose mesh the file does not store.
    Unmeshed,
    /// Curves, points, text: nothing to draw.
    Other,
}

#[derive(Clone, Copy)]
struct Attributes {
    layer: i32,
    material: i32,
    /// Where the material comes from: 0 the layer, 1 the object, 3 the
    /// block instance.
    material_source: u8,
    /// 1 hidden, 3 a block's member.
    mode: u8,
    visible: bool,
    /// On a layout's page, not in the model.
    page: bool,
}

impl Default for Attributes {
    fn default() -> Self {
        Attributes {
            layer: -1,
            material: -1,
            material_source: 0,
            mode: 0,
            visible: true,
            page: false,
        }
    }
}

struct Object {
    shape: Shape,
    attributes: Attributes,
}

struct Layer {
    material: i32,
    visible: bool,
    id: Option<[u8; 16]>,
    parent: Option<[u8; 16]>,
}

#[derive(Default)]
struct Model {
    version: u32,
    opennurbs: u32,
    unit: Option<&'static str>,
    /// Diffuse colours by material index.
    materials: HashMap<i32, Colour>,
    material_records: i32,
    layers: HashMap<i32, Layer>,
    layer_records: i32,
    /// Each block definition's members.
    definitions: HashMap<[u8; 16], Vec<[u8; 16]>>,
    /// The objects in table order, and those a definition may name.
    objects: Vec<Object>,
    by_id: HashMap<[u8; 16], usize>,
    /// Triangles read so far: the model's, before any is placed twice.
    triangles: usize,
}

impl Model {
    fn record(&mut self, code: u32, at: &mut Cursor<'_>, most: usize) -> Read3dm<()> {
        match code {
            UNITS => {
                let _version = at.i32()?;
                self.unit = unit(at.i32()?);
            }
            MATERIAL_RECORD => {
                let position = self.material_records;
                self.material_records += 1;
                let (_, mut data) = at.object()?;
                // A material that cannot be read leaves its objects grey.
                if let Ok((index, colour)) = self.material(&mut data) {
                    self.materials.insert(index.unwrap_or(position), colour);
                }
            }
            LAYER_RECORD => {
                let position = self.layer_records;
                self.layer_records += 1;
                let (_, mut data) = at.object()?;
                if let Ok((index, layer)) = self.layer(&mut data) {
                    let index = if index < 0 || self.layers.contains_key(&index) {
                        position
                    } else {
                        index
                    };
                    self.layers.insert(index, layer);
                }
            }
            DEFINITION_RECORD => {
                let (_, mut data) = at.object()?;
                if let Ok((id, members)) = self.definition(&mut data) {
                    self.definitions.insert(id, members);
                }
            }
            _ => self.object(at, most)?,
        }
        Ok(())
    }

    /// An object record: its geometry, then its attributes.
    fn object(&mut self, at: &mut Cursor<'_>, most: usize) -> Read3dm<()> {
        let mut shape = Shape::Other;
        let mut attributes = Attributes::default();
        let mut id = None;
        while at.at < at.bytes.len() {
            let start = at.at;
            let (code, mut inside) = at.chunk()?;
            match code {
                RECORD_END => break,
                CLASS => {
                    at.at = start;
                    let (class, mut data) = at.object()?;
                    // An object that cannot be read is not drawn, and said.
                    shape = geometry(class, &mut data, most).unwrap_or(Shape::Unmeshed);
                }
                ATTRIBUTES => {
                    if let Ok((own, read)) = self.attributes(&mut inside) {
                        id = Some(own);
                        attributes = read;
                    }
                }
                _ => {}
            }
        }
        if let Shape::Meshes(pieces) = &shape {
            self.triangles += pieces.iter().map(|p| p.triangles.len()).sum::<usize>();
            if self.triangles > most {
                return Err(format!("the model has more than {most} triangles"));
            }
        }
        if let Some(id) = id {
            self.by_id.insert(id, self.objects.len());
        }
        self.objects.push(Object { shape, attributes });
        Ok(())
    }

    /// `ON_3dmObjectAttributes`: the object's id and what decides whether
    /// and how it is drawn.
    fn attributes(&self, at: &mut Cursor<'_>) -> Read3dm<([u8; 16], Attributes)> {
        let mut out = Attributes::default();
        let (major, _) = at.version()?;
        let id = at.uuid()?;
        out.layer = at.i32()?;
        if self.version >= 5 && self.opennurbs >= 200_712_190 {
            if major != 2 {
                return Err("unknown attributes".into());
            }
            // Items by number, in order, up to the end mark.
            loop {
                match at.u8()? {
                    1 | 2 => at.skip_string()?,
                    3 | 10 | 22 => {
                        at.i32()?;
                    }
                    4 => out.material = at.i32()?,
                    5 => {
                        let (code, _) = at.chunk()?;
                        if code != ANONYMOUS {
                            return Err("unknown attributes".into());
                        }
                    }
                    6 | 7 => {
                        at.colour()?;
                    }
                    8 => {
                        at.f64()?;
                    }
                    9 | 13 | 14 | 15 | 17 => {
                        at.u8()?;
                    }
                    11 => out.visible = at.bool()?,
                    12 => out.mode = at.u8()?,
                    16 => out.material_source = at.u8()?,
                    18 => at.skip_array(4)?,
                    19 => out.page = at.u8()? == 2,
                    20 => {
                        at.uuid()?;
                    }
                    21 => at.skip_array(32)?,
                    // The end mark, or items of later versions: those
                    // read stand.
                    _ => break,
                }
            }
        } else {
            out.material = at.i32()?;
            at.colour()?;
            at.take(2 + 2 + 8 + 8 + 4)?;
            out.mode = at.u8()?;
            at.take(2)?;
            out.material_source = at.u8()?;
        }
        Ok((id, out))
    }

    /// `ON_Material`: its index, when it says it, and its diffuse colour.
    fn material(&self, at: &mut Cursor<'_>) -> Read3dm<(Option<i32>, Colour)> {
        let opaque = |[r, g, b, _]: [u8; 4]| [r, g, b, 255];
        if self.version >= 60 && self.opennurbs >= 2_348_833_910 {
            let (_, _, mut inside) = at.versioned(ANONYMOUS)?;
            let index = component_index(&mut inside)?;
            inside.uuid()?;
            inside.colour()?;
            return Ok((index, opaque(inside.colour()?)));
        }
        match at.version()?.0 {
            1 => {
                at.colour()?;
                Ok((None, opaque(at.colour()?)))
            }
            2 => {
                let (_, _, mut inside) = at.versioned(ANONYMOUS)?;
                inside.uuid()?;
                let index = inside.i32()?;
                inside.skip_string()?;
                inside.uuid()?;
                inside.colour()?;
                Ok((Some(index), opaque(inside.colour()?)))
            }
            _ => Err("unknown material".into()),
        }
    }

    /// `ON_Layer`: its index, material and visibility, and its place among
    /// its parents.
    fn layer(&self, at: &mut Cursor<'_>) -> Read3dm<(i32, Layer)> {
        let (_, minor) = at.version()?;
        let mode = at.i32()?;
        let index = at.i32()?;
        at.i32()?;
        let material = at.i32()?;
        at.take(4 + 4 + 2 + 2 + 8 + 8)?;
        at.skip_string()?;
        let mut layer = Layer {
            material,
            visible: mode != 1,
            id: None,
            parent: None,
        };
        if minor >= 1 {
            layer.visible = at.bool()?;
        }
        if minor >= 5 {
            at.take(4 + 4 + 8 + 1)?;
            layer.id = Some(at.uuid()?);
            if minor >= 6 && self.opennurbs > 200_505_110 {
                layer.parent = Some(at.uuid()?).filter(|id| *id != [0; 16]);
            }
        }
        Ok((index, layer))
    }

    /// `ON_InstanceDefinition`: its id and its members' ids.
    fn definition(&self, at: &mut Cursor<'_>) -> Read3dm<([u8; 16], Vec<[u8; 16]>)> {
        let next_is_versioned = at.bytes.get(..4) == Some(&ANONYMOUS.to_le_bytes()[..]);
        let fifth = self.version <= 50
            || (self.version <= 60 && (self.opennurbs <= 2_348_834_153 || !next_is_versioned));
        if fifth {
            if at.version()?.0 != 1 {
                return Err("unknown block".into());
            }
            let id = at.uuid()?;
            return Ok((id, ids(at)?));
        }
        let (_, _, mut inside) = at.versioned(ANONYMOUS)?;
        let (_, _, mut component) = inside.versioned(COMPONENT)?;
        if component.u8()? == 1 {
            component.take(12)?;
        }
        if component.u8()? != 1 {
            return Err("a block without its id".into());
        }
        let id = component.uuid()?;
        inside.u32()?;
        inside.chunk()?;
        for _ in 0..3 {
            inside.skip_string()?;
        }
        inside.take(48)?;
        let members = if inside.bool()? {
            ids(&mut inside)?
        } else {
            Vec::new()
        };
        Ok((id, members))
    }

    /// Every object shown, placed, in its colours; how many store no mesh.
    fn draw(&self, mesh: &mut Mesh) -> Result<usize, String> {
        let mut missing = 0;
        let mut drawn = 0;
        for object in &self.objects {
            if object.attributes.mode == 3 || !self.shown(&object.attributes) {
                continue;
            }
            let before = mesh.triangles.len();
            self.place(object, &IDENTITY, None, &mut Vec::new(), mesh, &mut missing)?;
            if mesh.triangles.len() > before {
                drawn += 1;
            }
        }
        mesh.objects = drawn;
        if mesh.triangles.is_empty() && missing > 0 {
            return Err(
                "nothing shown in this model has a stored mesh (Rhino stores one only for what it showed shaded)"
                    .into(),
            );
        }
        Ok(missing)
    }

    /// Whether an object is shown: visible, not hidden, in the model, on a
    /// layer shown with its parents.
    fn shown(&self, attributes: &Attributes) -> bool {
        if !attributes.visible || attributes.mode == 1 || attributes.page {
            return false;
        }
        let mut layer = self.layers.get(&attributes.layer);
        let mut seen = 0;
        while let Some(this) = layer {
            if !this.visible || seen > 64 {
                return false;
            }
            seen += 1;
            layer = this
                .parent
                .and_then(|parent| self.layers.values().find(|other| other.id == Some(parent)));
        }
        true
    }

    /// The material an object is drawn in: its own, its layer's or its
    /// block instance's.
    fn material_of(&self, attributes: &Attributes, parent: Option<i32>) -> i32 {
        let layer = || {
            self.layers
                .get(&attributes.layer)
                .map_or(-1, |l| l.material)
        };
        match attributes.material_source {
            1 => attributes.material,
            3 => parent.unwrap_or_else(layer),
            _ => layer(),
        }
    }

    fn place(
        &self,
        object: &Object,
        transform: &Transform,
        parent: Option<i32>,
        path: &mut Vec<[u8; 16]>,
        mesh: &mut Mesh,
        missing: &mut usize,
    ) -> Result<(), String> {
        let material = self.material_of(&object.attributes, parent);
        match &object.shape {
            Shape::Meshes(pieces) => {
                let paint = self.materials.get(&material).copied();
                let mirrored = determinant(transform) < 0.;
                for piece in pieces {
                    let colours = piece.colours.clone().or_else(|| {
                        piece
                            .face
                            .or(paint)
                            .map(|colour| vec![colour; piece.positions.len()])
                    });
                    let triangles = piece
                        .triangles
                        .iter()
                        .map(|&[a, b, c]| if mirrored { [a, c, b] } else { [a, b, c] });
                    mesh.add_coloured(&piece.positions, colours.as_deref(), triangles, transform)?;
                }
            }
            Shape::Instance(definition, placed) => {
                if path.len() >= NESTING || path.contains(definition) {
                    return Ok(());
                }
                let Some(members) = self.definitions.get(definition) else {
                    return Ok(());
                };
                let transform = compose(placed, transform);
                path.push(*definition);
                for member in members {
                    let Some(&at) = self.by_id.get(member) else {
                        continue;
                    };
                    let member = &self.objects[at];
                    if !self.shown(&member.attributes) {
                        continue;
                    }
                    self.place(member, &transform, Some(material), path, mesh, missing)?;
                }
                path.pop();
            }
            Shape::Unmeshed => *missing += 1,
            Shape::Other => {}
        }
        Ok(())
    }
}

/// A model component's index (`ON_ModelComponent`'s own attributes chunk).
fn component_index(at: &mut Cursor<'_>) -> Read3dm<Option<i32>> {
    let (_, _, mut inside) = at.versioned(ANONYMOUS)?;
    let bits = inside.u32()?;
    if bits & 1 != 0 {
        inside.uuid()?;
    }
    if bits & 2 != 0 {
        inside.uuid()?;
    }
    Ok(if bits & 4 != 0 {
        Some(inside.i32()?)
    } else {
        None
    })
}

/// A counted array of ids.
fn ids(at: &mut Cursor<'_>) -> Read3dm<Vec<[u8; 16]>> {
    let count = at.u32()? as usize;
    if count > at.bytes.len() / 16 {
        return Err(ends());
    }
    (0..count).map(|_| at.uuid()).collect()
}

/// The determinant of a transform's linear part: below zero, it mirrors.
fn determinant(t: &Transform) -> f32 {
    let [x, y, z] = [t[0], t[1], t[2]];
    x[0] * (y[1] * z[2] - y[2] * z[1]) - x[1] * (y[0] * z[2] - y[2] * z[0])
        + x[2] * (y[0] * z[1] - y[1] * z[0])
}

/// What an object of class `class` holds to be drawn.
fn geometry(class: [u8; 16], at: &mut Cursor<'_>, most: usize) -> Read3dm<Shape> {
    if class == MESH {
        return Ok(match mesh(at, most)? {
            Some(piece) => Shape::Meshes(vec![piece]),
            None => Shape::Unmeshed,
        });
    }
    if BREPS.contains(&class) {
        return brep(at, most);
    }
    if class == EXTRUSION {
        return extrusion(at, most);
    }
    if class == SUBD {
        return Ok(Shape::Unmeshed);
    }
    if class == INSTANCE {
        at.version()?;
        let definition = at.uuid()?;
        let mut m = [0f64; 16];
        for value in &mut m {
            *value = at.f64()?;
        }
        // Rows of a matrix that maps column vectors: the images of the
        // axes are its columns, the translation its last.
        let column = |c: usize| [m[c], m[4 + c], m[8 + c]].map(|v| v as f32);
        return Ok(Shape::Instance(
            definition,
            [column(0), column(1), column(2), column(3)],
        ));
    }
    Ok(Shape::Other)
}

/// `ON_Mesh`, as openNURBS has written it since Rhino 3: faces, then its
/// compressed vertices and colours. `None` for an empty one or one of
/// Rhino 2's.
fn mesh(at: &mut Cursor<'_>, most: usize) -> Read3dm<Option<Piece>> {
    let (major, _) = at.version()?;
    if major != 3 {
        return Ok(None);
    }
    let vertices = usize::try_from(at.i32()?).map_err(|_| ends())?;
    let faces = usize::try_from(at.i32()?).map_err(|_| ends())?;
    if faces > most || vertices > most.saturating_mul(4) {
        return Err(format!("the model has more than {most} triangles"));
    }
    // Its domains, scales and boxes, and whether it is closed.
    at.take(64 + 16 + 24 + 24 + 16 + 4)?;
    // Its meshing parameters and curvature statistics, when it has them.
    for _ in 0..5 {
        if at.u8()? != 0 {
            at.chunk()?;
        }
    }
    let size = at.i32()?;
    let width = match size {
        1 | 2 | 4 => size as usize,
        _ => return Err("unknown mesh faces".into()),
    };
    let raw = at.take(faces.checked_mul(4 * width).ok_or_else(ends)?)?;
    let index = |i: usize| -> u32 {
        let bytes = &raw[i * width..(i + 1) * width];
        match width {
            1 => u32::from(bytes[0]),
            2 => u32::from(u16::from_le_bytes([bytes[0], bytes[1]])),
            _ => u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
        }
    };
    if vertices == 0 || faces == 0 {
        return Ok(None);
    }
    let Some(points) = at.buffer(vertices * 12, true)? else {
        return Ok(None);
    };
    at.buffer(vertices * 12, false)?;
    at.buffer(vertices * 8, false)?;
    at.buffer(vertices * 16, false)?;
    let colours = at.buffer(vertices * 4, true)?.map(|bytes| {
        bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|&[r, g, b, _]| [r, g, b, 255])
            .collect()
    });
    let positions = points
        .as_chunks::<12>()
        .0
        .iter()
        .map(|p| {
            let at = |i: usize| f32::from_le_bytes([p[i], p[i + 1], p[i + 2], p[i + 3]]);
            [at(0), at(4), at(8)]
        })
        .collect();
    let mut triangles = Vec::with_capacity(faces);
    for face in 0..faces {
        let [a, b, c, d] = [0, 1, 2, 3].map(|k| index(face * 4 + k));
        // Mesh::add leaves out a triangle naming a vertex it has not.
        triangles.push([a, b, c]);
        if c != d {
            triangles.push([a, c, d]);
        }
    }
    Ok(Some(Piece {
        positions,
        triangles,
        colours,
        face: None,
    }))
}

/// `ON_Brep` since Rhino 3: its curves, surfaces and topology passed over
/// by their chunks' lengths, its faces' colours read, then the render mesh
/// Rhino stored for each face.
fn brep(at: &mut Cursor<'_>, most: usize) -> Read3dm<Shape> {
    let (major, minor) = at.version()?;
    if major != 3 {
        return Ok(Shape::Unmeshed);
    }
    // 2D curves, 3D curves, surfaces, vertices, edges, trims, loops.
    for _ in 0..7 {
        at.chunk()?;
    }
    let (_, mut faces) = at.chunk()?;
    let (_, face_minor) = faces.version()?;
    let count = usize::try_from(faces.i32()?).map_err(|_| ends())?;
    if count > faces.bytes.len() {
        return Err(ends());
    }
    for _ in 0..count {
        faces.i32()?;
        faces.skip_array(4)?;
        faces.take(12)?;
    }
    let mut colours = vec![None; count];
    if face_minor >= 1 {
        faces.take(count * 16)?;
        if face_minor >= 2 && faces.bool()? {
            for colour in &mut colours {
                let [r, g, b, a] = faces.colour()?;
                if [r, g, b, a] != UNSET {
                    *colour = Some([r, g, b, 255]);
                }
            }
        }
    }
    // Its bounding box.
    at.take(48)?;
    if minor < 1 {
        return Ok(Shape::Unmeshed);
    }
    let (_, mut meshes) = at.chunk()?;
    let mut pieces = Vec::new();
    for colour in colours {
        if meshes.u8()? == 0 {
            continue;
        }
        let (class, mut data) = meshes.object()?;
        if class == MESH
            && let Some(mut piece) = mesh(&mut data, most)?
        {
            piece.face = colour;
            pieces.push(piece);
        }
    }
    Ok(if pieces.is_empty() {
        Shape::Unmeshed
    } else {
        Shape::Meshes(pieces)
    })
}

/// `ON_Extrusion`: its profile passed over, then its cached render mesh
/// (Rhino 6 on; Rhino 5 kept none in the file).
fn extrusion(at: &mut Cursor<'_>, most: usize) -> Read3dm<Shape> {
    let (_, minor, mut inside) = at.versioned(ANONYMOUS)?;
    inside.object()?;
    // Path, domain, directions, bools, boxes.
    inside.take(88 + 2 + 64 + 1)?;
    if minor < 3 {
        return Ok(Shape::Unmeshed);
    }
    inside.take(4 + 2)?;
    let (_, _, mut cache) = inside.versioned(ANONYMOUS)?;
    let mut found = None;
    while cache.u8()? != 0 {
        let (_, _, mut item) = cache.versioned(ANONYMOUS)?;
        let kind = item.uuid()?;
        let (class, mut data) = item.object()?;
        if class != MESH || (found.is_some() && kind != RENDER_MESH) {
            continue;
        }
        if let Some(piece) = mesh(&mut data, most)? {
            found = Some(piece);
            if kind == RENDER_MESH {
                break;
            }
        }
    }
    Ok(found.map_or(Shape::Unmeshed, |piece| Shape::Meshes(vec![piece])))
}
