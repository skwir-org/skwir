//! PLY, as text or binary of either byte order: a header naming each
//! element's properties, then the elements in that order. Only the points'
//! `x`, `y`, `z` and colours (`red`, `green`, `blue`, `alpha`, or
//! `diffuse_` before them) and the faces' index lists are kept; everything
//! else (normals, other elements) is read past.
use crate::mesh::{Colour, IDENTITY, Mesh, Point};

#[derive(Clone, Copy)]
enum Kind {
    I8,
    U8,
    I16,
    U16,
    I32,
    U32,
    F32,
    F64,
}

impl Kind {
    fn parse(name: &str) -> Result<Kind, String> {
        Ok(match name {
            "char" | "int8" => Kind::I8,
            "uchar" | "uint8" => Kind::U8,
            "short" | "int16" => Kind::I16,
            "ushort" | "uint16" => Kind::U16,
            "int" | "int32" => Kind::I32,
            "uint" | "uint32" => Kind::U32,
            "float" | "float32" => Kind::F32,
            "double" | "float64" => Kind::F64,
            other => return Err(format!("the PLY type {other} is unknown")),
        })
    }

    fn size(self) -> usize {
        match self {
            Kind::I8 | Kind::U8 => 1,
            Kind::I16 | Kind::U16 => 2,
            Kind::I32 | Kind::U32 | Kind::F32 => 4,
            Kind::F64 => 8,
        }
    }
}

enum Property {
    One(String, Kind),
    List(String, Kind, Kind),
}

struct Element {
    name: String,
    count: usize,
    properties: Vec<Property>,
}

/// Where the values come from: words of text, or bytes of either order.
enum Values<'a> {
    Text(std::str::SplitAsciiWhitespace<'a>),
    Binary {
        bytes: &'a [u8],
        at: usize,
        big: bool,
    },
}

impl Values<'_> {
    fn next(&mut self, kind: Kind) -> Result<f64, String> {
        match self {
            Values::Text(words) => words
                .next()
                .and_then(|w| w.parse().ok())
                .ok_or_else(|| "the PLY model ends early".into()),
            Values::Binary { bytes, at, big } => {
                let size = kind.size();
                let raw = bytes
                    .get(*at..*at + size)
                    .ok_or("the PLY model ends early")?;
                *at += size;
                let mut buf = [0u8; 8];
                buf[..size].copy_from_slice(raw);
                if *big {
                    buf[..size].reverse();
                }
                Ok(match kind {
                    Kind::I8 => f64::from(buf[0].cast_signed()),
                    Kind::U8 => f64::from(buf[0]),
                    Kind::I16 => f64::from(i16::from_le_bytes([buf[0], buf[1]])),
                    Kind::U16 => f64::from(u16::from_le_bytes([buf[0], buf[1]])),
                    Kind::I32 => f64::from(i32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]])),
                    Kind::U32 => f64::from(u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]])),
                    Kind::F32 => f64::from(f32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]])),
                    Kind::F64 => f64::from_le_bytes(buf),
                })
            }
        }
    }
}

/// The corners a face may list.
const CORNERS: usize = 1024;

/// The channel of a point's colour a property names.
fn channel(name: &str) -> Option<usize> {
    let name = name.strip_prefix("diffuse_").unwrap_or(name);
    ["red", "green", "blue", "alpha"]
        .iter()
        .position(|c| *c == name)
}

pub fn read(bytes: &[u8], mesh: &mut Mesh) -> Result<(), String> {
    const END: &[u8] = b"end_header";
    let end = bytes
        .windows(END.len())
        .position(|w| w == END)
        .ok_or("not a PLY model")?;
    let body = bytes[end..]
        .iter()
        .position(|b| *b == b'\n')
        .map(|n| end + n + 1)
        .ok_or("not a PLY model")?;
    let header = std::str::from_utf8(&bytes[..end]).map_err(|_| "not a PLY model")?;
    let mut lines = header.lines().map(str::trim);
    if lines.next() != Some("ply") {
        return Err("not a PLY model".into());
    }
    let mut format = None;
    let mut elements: Vec<Element> = Vec::new();
    for line in lines {
        let words: Vec<&str> = line.split_whitespace().collect();
        match words.as_slice() {
            ["format", kind, ..] => format = Some((*kind).to_owned()),
            ["element", name, count] => elements.push(Element {
                name: (*name).to_owned(),
                count: count
                    .parse()
                    .map_err(|_| "a PLY element count is not a number")?,
                properties: Vec::new(),
            }),
            ["property", "list", count, item, name] => elements
                .last_mut()
                .ok_or("a PLY property comes before its element")?
                .properties
                .push(Property::List(
                    (*name).to_owned(),
                    Kind::parse(count)?,
                    Kind::parse(item)?,
                )),
            ["property", kind, name] => elements
                .last_mut()
                .ok_or("a PLY property comes before its element")?
                .properties
                .push(Property::One((*name).to_owned(), Kind::parse(kind)?)),
            _ => {}
        }
    }
    let rest = &bytes[body..];
    let mut values = match format.as_deref() {
        Some("ascii") => Values::Text(
            std::str::from_utf8(rest)
                .map_err(|_| "the PLY text is not UTF-8")?
                .split_ascii_whitespace(),
        ),
        Some("binary_little_endian") => Values::Binary {
            bytes: rest,
            at: 0,
            big: false,
        },
        Some("binary_big_endian") => Values::Binary {
            bytes: rest,
            at: 0,
            big: true,
        },
        _ => return Err("the PLY format is unknown".into()),
    };
    let mut points: Vec<Point> = Vec::new();
    let mut colours: Vec<Colour> = Vec::new();
    let mut triangles: Vec<[u32; 3]> = Vec::new();
    for element in &elements {
        let is_vertex = element.name == "vertex";
        // Coloured when its points say red, green and blue.
        let coloured = is_vertex
            && [0, 1, 2].iter().all(|c| {
                element
                    .properties
                    .iter()
                    .any(|p| matches!(p, Property::One(name, _) if channel(name) == Some(*c)))
            });
        let is_face = element.name == "face";
        if (is_vertex && element.count > mesh.most().saturating_mul(3))
            || (is_face && element.count > mesh.most())
        {
            return Err(format!("the model has more than {} triangles", mesh.most()));
        }
        for _ in 0..element.count {
            let mut point = [0f32; 3];
            let mut colour: Colour = [0, 0, 0, 255];
            for property in &element.properties {
                match property {
                    Property::One(name, kind) => {
                        let value = values.next(*kind)?;
                        if is_vertex
                            && let Some(axis) = ["x", "y", "z"].iter().position(|a| a == name)
                        {
                            point[axis] = value as f32;
                        } else if coloured && let Some(c) = channel(name) {
                            // Bytes, or fractions of one.
                            let scale = if matches!(kind, Kind::F32 | Kind::F64) {
                                255.
                            } else {
                                1.
                            };
                            colour[c] = (value * scale).clamp(0., 255.).round() as u8;
                        }
                    }
                    Property::List(name, count, item) => {
                        let n = values.next(*count)? as usize;
                        if n > CORNERS {
                            return Err("a PLY face has too many corners".into());
                        }
                        let corners: Vec<u32> = (0..n)
                            .map(|_| values.next(*item).map(|i| i as u32))
                            .collect::<Result<_, _>>()?;
                        if is_face && matches!(name.as_str(), "vertex_indices" | "vertex_index") {
                            for i in 1..n.saturating_sub(1) {
                                triangles.push([corners[0], corners[i], corners[i + 1]]);
                            }
                            if triangles.len() > mesh.most() {
                                return Err(format!(
                                    "the model has more than {} triangles",
                                    mesh.most()
                                ));
                            }
                        }
                    }
                }
            }
            if is_vertex {
                points.push(point);
                if coloured {
                    colours.push(colour);
                }
            }
        }
    }
    mesh.objects = 1;
    let colours = (!colours.is_empty()).then_some(colours.as_slice());
    mesh.add_coloured(&points, colours, triangles, &IDENTITY)
}
