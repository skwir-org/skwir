//! 3MF packages: the build's items, each an object placed by its
//! transform; an object is a mesh, or components (other objects, maybe in
//! another model file of the package, as Bambu Studio and Orca write them)
//! placed by theirs. Lengths are in the unit the model states. Colours are
//! those of the core specification's base materials and the materials
//! extension's colour groups, by triangle or by corner.
use crate::mesh::{Colour, IDENTITY, Mesh, Point, Transform, compose, grey};
use quick_xml::events::Event;
use std::collections::HashMap;
use std::io::Read;

/// Components nested deeper than this are not followed.
const DEPTH: usize = 16;
/// Model files read from one package.
const PARTS: usize = 256;

type Archive = zip::ZipArchive<std::io::Cursor<Vec<u8>>>;

#[derive(Default)]
struct Object {
    points: Vec<Point>,
    triangles: Vec<[u32; 3]>,
    /// Its own property (`pid`, `pindex`): its triangles' unless they say.
    property: Option<(u32, u32)>,
    /// Each triangle's colour resource (a `Part::resources` index) and its
    /// corners' colours in it (`p1`, `p2`, `p3`), once any triangle has one.
    properties: Vec<Option<(u32, [u32; 3])>>,
    /// Other objects: the part holding them (`None`: this one), their id,
    /// and where they are placed.
    components: Vec<(Option<String>, String, Transform)>,
}

#[derive(Default)]
struct Part {
    objects: HashMap<String, Object>,
    /// The ids a property names, each once.
    resources: Vec<String>,
    /// Their colours, in order: a base material's display colour, a colour
    /// group's colours.
    colours: HashMap<String, Vec<Colour>>,
    /// The build: objects placed by their transforms.
    items: Vec<(Option<String>, String, Transform)>,
    unit: Option<String>,
}

/// A transform as 3MF writes it: twelve numbers, the images of X, Y and Z,
/// then the translation.
fn transform(text: Option<String>) -> Transform {
    let Some(text) = text else { return IDENTITY };
    let n: Vec<f32> = text
        .split_whitespace()
        .filter_map(|w| w.parse().ok())
        .collect();
    if n.len() != 12 {
        return IDENTITY;
    }
    [
        [n[0], n[1], n[2]],
        [n[3], n[4], n[5]],
        [n[6], n[7], n[8]],
        [n[9], n[10], n[11]],
    ]
}

fn attribute(element: &quick_xml::events::BytesStart<'_>, name: &str) -> Option<String> {
    element
        .attributes()
        .flatten()
        .find(|a| a.key.local_name().as_ref() == name)
        .and_then(|a| {
            a.normalized_value(quick_xml::XmlVersion::Implicit1_0)
                .ok()
                .map(std::borrow::Cow::into_owned)
        })
}

fn number(element: &quick_xml::events::BytesStart<'_>, name: &str) -> f32 {
    attribute(element, name)
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.)
}

/// A colour as 3MF writes it: `#RRGGBB` or `#RRGGBBAA`.
fn colour(text: &str) -> Option<Colour> {
    let hex = text.strip_prefix('#')?;
    let byte = |at: usize| u8::from_str_radix(hex.get(at..at + 2)?, 16).ok();
    match hex.len() {
        6 => Some([byte(0)?, byte(2)?, byte(4)?, 255]),
        8 => Some([byte(0)?, byte(2)?, byte(4)?, byte(6)?]),
        _ => None,
    }
}

impl Part {
    /// The index `id` is kept by, among the ids properties name.
    fn resource(&mut self, id: String) -> u32 {
        if let Some(at) = self.resources.iter().position(|known| *known == id) {
            return at as u32;
        }
        self.resources.push(id);
        (self.resources.len() - 1) as u32
    }

    /// A triangle corner's colour: number `index` of resource `resource`,
    /// grey where it names none.
    fn colour(&self, resource: u32, index: u32) -> Colour {
        self.resources
            .get(resource as usize)
            .and_then(|id| self.colours.get(id))
            .and_then(|colours| colours.get(index as usize))
            .copied()
            .unwrap_or_else(grey)
    }
}

/// A part's path as the zip names it.
fn member_name(path: &str) -> String {
    path.trim_start_matches('/').to_owned()
}

fn parse(xml: &[u8], most: usize) -> Result<Part, String> {
    let mut reader = quick_xml::Reader::from_reader(xml);
    let mut buf = Vec::new();
    let mut part = Part::default();
    let mut current: Option<(String, Object)> = None;
    // The colour resource being read: its id and colours.
    let mut group: Option<(String, Vec<Colour>)> = None;
    let mut triangles = 0usize;
    loop {
        let event = reader
            .read_event_into(&mut buf)
            .map_err(|e| format!("the 3MF model is not XML: {e}"))?;
        match event {
            Event::Start(e) | Event::Empty(e) => match e.local_name().as_ref() {
                "model" => part.unit = attribute(&e, "unit"),
                "object" => {
                    let property = attribute(&e, "pid").map(|pid| {
                        let index = attribute(&e, "pindex").and_then(|v| v.parse().ok());
                        (part.resource(pid), index.unwrap_or(0))
                    });
                    current = attribute(&e, "id").map(|id| {
                        (
                            id,
                            Object {
                                property,
                                ..Object::default()
                            },
                        )
                    });
                }
                "basematerials" | "colorgroup" => {
                    group = attribute(&e, "id").map(|id| (id, Vec::new()));
                }
                "base" | "color" => {
                    let name = if e.local_name().as_ref() == "base" {
                        "displaycolor"
                    } else {
                        "color"
                    };
                    if let Some((_, colours)) = &mut group {
                        colours.push(
                            attribute(&e, name)
                                .and_then(|c| colour(&c))
                                .unwrap_or_else(grey),
                        );
                    }
                }
                "vertex" => {
                    if let Some((_, object)) = &mut current {
                        if object.points.len() > most.saturating_mul(3) {
                            return Err(format!("the model has more than {most} triangles"));
                        }
                        object
                            .points
                            .push([number(&e, "x"), number(&e, "y"), number(&e, "z")]);
                    }
                }
                "triangle" => {
                    if let Some((_, object)) = &mut current {
                        triangles += 1;
                        if triangles > most {
                            return Err(format!("the model has more than {most} triangles"));
                        }
                        let index = |name| {
                            attribute(&e, name)
                                .and_then(|v| v.parse().ok())
                                .unwrap_or(u32::MAX)
                        };
                        object
                            .triangles
                            .push([index("v1"), index("v2"), index("v3")]);
                        // Its colour: its own property, else its object's.
                        let own = attribute(&e, "pid").map(|pid| part.resource(pid));
                        let corner = |name: &str| attribute(&e, name).and_then(|v| v.parse().ok());
                        let property = match (own.or(object.property.map(|p| p.0)), corner("p1")) {
                            (Some(resource), first) => {
                                let first = first.or(object.property.map(|p| p.1)).unwrap_or(0);
                                Some((
                                    resource,
                                    [
                                        first,
                                        corner("p2").unwrap_or(first),
                                        corner("p3").unwrap_or(first),
                                    ],
                                ))
                            }
                            (None, _) => None,
                        };
                        // Kept from the first triangle with one on, those
                        // before it without.
                        if property.is_some() || !object.properties.is_empty() {
                            object.properties.resize(object.triangles.len() - 1, None);
                            object.properties.push(property);
                        }
                    }
                }
                "component" => {
                    if let (Some((_, object)), Some(id)) = (&mut current, attribute(&e, "objectid"))
                    {
                        object.components.push((
                            attribute(&e, "path"),
                            id,
                            transform(attribute(&e, "transform")),
                        ));
                    }
                }
                "item" => {
                    if let Some(id) = attribute(&e, "objectid") {
                        part.items.push((
                            attribute(&e, "path"),
                            id,
                            transform(attribute(&e, "transform")),
                        ));
                    }
                }
                _ => {}
            },
            Event::End(e) if e.local_name().as_ref() == "object" => {
                if let Some((id, object)) = current.take() {
                    part.objects.insert(id, object);
                }
            }
            Event::End(e) if matches!(e.local_name().as_ref(), "basematerials" | "colorgroup") => {
                if let Some((id, colours)) = group.take() {
                    part.colours.insert(id, colours);
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    Ok(part)
}

struct Package {
    archive: Archive,
    parts: HashMap<String, Part>,
    most_bytes: u64,
    /// Bytes of model files unpacked so far.
    unpacked: u64,
    most: usize,
}

impl Package {
    /// The part at `name`, read once.
    fn part(&mut self, name: &str) -> Result<&Part, String> {
        if !self.parts.contains_key(name) {
            if self.parts.len() >= PARTS {
                return Err("the package holds too many model files".into());
            }
            let file = self
                .archive
                .by_name(name)
                .map_err(|_| format!("the package has no {name}"))?;
            // Every model file together, as they unpack, within the bound.
            let room = self.most_bytes.saturating_sub(self.unpacked);
            if file.size() > room {
                return Err(format!(
                    "the package's models unpack to more than {} bytes",
                    self.most_bytes
                ));
            }
            let mut xml = Vec::new();
            file.take(room + 1)
                .read_to_end(&mut xml)
                .map_err(|e| format!("reading {name}: {e}"))?;
            if xml.len() as u64 > room {
                return Err(format!(
                    "the package's models unpack to more than {} bytes",
                    self.most_bytes
                ));
            }
            self.unpacked += xml.len() as u64;
            let part = parse(&xml, self.most)?;
            self.parts.insert(name.to_owned(), part);
        }
        Ok(&self.parts[name])
    }

    /// Object `id` of part `name`, placed by `placed`, into `mesh`.
    fn place(
        &mut self,
        name: &str,
        id: &str,
        placed: &Transform,
        mesh: &mut Mesh,
        depth: usize,
    ) -> Result<(), String> {
        if depth > DEPTH {
            return Ok(());
        }
        let part = self.part(name)?;
        let Some(object) = part.objects.get(id) else {
            return Ok(());
        };
        if !object.triangles.is_empty() && object.properties.is_empty() {
            mesh.add(&object.points, object.triangles.iter().copied(), placed)?;
        } else if !object.triangles.is_empty() {
            // Coloured: a point once for each colour its triangles give it.
            let mut made: HashMap<(u32, Colour), u32> = HashMap::new();
            let (mut points, mut colours) = (Vec::new(), Vec::new());
            let mut triangles = Vec::with_capacity(object.triangles.len());
            for (t, property) in object.triangles.iter().zip(&object.properties) {
                if t.iter().any(|&i| i as usize >= object.points.len()) {
                    continue;
                }
                triangles.push([0, 1, 2].map(|corner| {
                    let colour = property.map_or_else(grey, |(resource, indices)| {
                        part.colour(resource, indices[corner])
                    });
                    *made.entry((t[corner], colour)).or_insert_with(|| {
                        points.push(object.points[t[corner] as usize]);
                        colours.push(colour);
                        (points.len() - 1) as u32
                    })
                }));
            }
            mesh.add_coloured(&points, Some(&colours), triangles, placed)?;
        }
        let components = object.components.clone();
        for (path, child, local) in components {
            let child_part = path.as_deref().map_or_else(|| name.to_owned(), member_name);
            self.place(
                &child_part,
                &child,
                &compose(&local, placed),
                mesh,
                depth + 1,
            )?;
        }
        Ok(())
    }
}

/// The root model's name, from the package's relationships, else where it
/// usually is.
fn root(archive: &mut Archive) -> String {
    let mut rels = String::new();
    if let Ok(file) = archive.by_name("_rels/.rels") {
        let _ = file.take(64 * 1024).read_to_string(&mut rels);
    }
    let mut reader = quick_xml::Reader::from_reader(rels.as_bytes());
    let mut buf = Vec::new();
    while let Ok(event) = reader.read_event_into(&mut buf) {
        match event {
            Event::Empty(e) | Event::Start(e) if e.local_name().as_ref() == "Relationship" => {
                if attribute(&e, "Type").is_some_and(|t| t.ends_with("/3dmodel"))
                    && let Some(target) = attribute(&e, "Target")
                {
                    return member_name(&target);
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    "3D/3dmodel.model".into()
}

/// The unit's usual abbreviation.
fn unit(name: &str) -> Option<&'static str> {
    Some(match name {
        "micron" => "µm",
        "millimeter" => "mm",
        "centimeter" => "cm",
        "inch" => "in",
        "foot" => "ft",
        "meter" => "m",
        _ => return None,
    })
}

pub fn read(bytes: Vec<u8>, most_bytes: u64, mesh: &mut Mesh) -> Result<(), String> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .map_err(|e| format!("not a 3MF package: {e}"))?;
    let root = root(&mut archive);
    let mut package = Package {
        archive,
        parts: HashMap::new(),
        most_bytes,
        unpacked: 0,
        most: mesh.most(),
    };
    let (items, stated) = {
        let part = package.part(&root)?;
        (part.items.clone(), part.unit.clone())
    };
    for (path, id, placed) in &items {
        let name = path.as_deref().map_or_else(|| root.clone(), member_name);
        package.place(&name, id, placed, mesh, 0)?;
    }
    mesh.objects = items.len();
    // Millimetres unless the model says otherwise.
    mesh.unit = unit(stated.as_deref().unwrap_or("millimeter"));
    Ok(())
}
