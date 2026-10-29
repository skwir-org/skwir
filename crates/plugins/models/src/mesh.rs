//! Meshes that carry no picture of their own (STL, OBJ, PLY, OFF, glTF, a
//! 3MF its slicer did not picture, a STEP or IGES model's faces, the meshes a Rhino
//! model stores), drawn and measured by
//! `skwir-model-worker` (`skwir_platform::model`), or read whole for a
//! window that draws and turns them (ADR-067). Properties says what a print
//! needs: its size, whether it is watertight and what it holds.
use crate::Found;
use skwir_core::{CancelToken, preview::FileSource};
use skwir_platform::model::{self, Asked, Measures, MeshFormat, Shown};
use std::sync::Arc;

/// The unit's volume as it is usually said, from a length unit cubed.
fn volume(value: f64, unit: &str) -> Option<String> {
    let (value, unit) = match unit {
        "µm" => (value / 1e9, "mm³"),
        "mm" => (value / 1e3, "cm³"),
        "cm" => (value, "cm³"),
        "m" => (value, "m³"),
        "in" => (value, "in³"),
        "ft" => (value, "ft³"),
        _ => return None,
    };
    Some(format!("{} {unit}", crate::gcode::short(value)))
}

/// What the measures say, in the order Properties shows them.
pub fn details(format: MeshFormat, measures: &Measures) -> Vec<(String, String)> {
    // An STL states no unit; every slicer reads it in millimetres.
    let unit = measures
        .unit
        .clone()
        .or_else(|| (format == MeshFormat::Stl).then(|| "mm".to_owned()));
    let [x, y, z] = measures.size.map(crate::gcode::short);
    let size = match &unit {
        Some(unit) => format!("{x} × {y} × {z} {unit}"),
        None => format!("{x} × {y} × {z}"),
    };
    let mut out = vec![("Size".to_owned(), size)];
    if let (Some(value), Some(unit)) = (measures.volume, &unit)
        && let Some(said) = volume(value, unit)
    {
        out.push(("Volume".into(), said));
    }
    out.push((
        "Watertight".into(),
        if measures.closed { "Yes" } else { "No" }.into(),
    ));
    if measures.faces > 0 {
        out.push(("Faces".into(), measures.faces.to_string()));
    }
    out.push(("Triangles".into(), measures.triangles.to_string()));
    if measures.objects > 1 {
        out.push(("Objects".into(), measures.objects.to_string()));
    }
    out
}

/// The format's name, for the toolbar.
fn title(format: MeshFormat) -> &'static str {
    match format {
        MeshFormat::Stl => "STL model",
        MeshFormat::Obj => "OBJ model",
        MeshFormat::Ply => "PLY model",
        MeshFormat::Off => "OFF model",
        MeshFormat::Gltf => "glTF model",
        MeshFormat::ThreeMf => "3MF",
        MeshFormat::Step => "STEP model",
        MeshFormat::Iges => "IGES model",
        MeshFormat::Rhino => "Rhino model",
    }
}

/// The mesh `source` holds drawn at most `side` pixels on its longer
/// side, by the helper, which reads it by path
/// (`FileSource::path_for_program`).
pub fn read<S: FileSource + ?Sized>(
    source: &S,
    format: MeshFormat,
    side: u32,
    asked: Asked,
    cancel: &CancelToken,
) -> anyhow::Result<Found> {
    let drawn = model::draw(
        &source.path_for_program(cancel)?,
        format,
        side,
        asked,
        cancel,
    )?;
    let details = details(format, &drawn.measures);
    // Faces that would not mesh are holes in what is shown, not in the
    // model: said, so a hole is never taken for the shape.
    let missing = drawn.measures.faces_missing;
    let unmeshed = drawn.measures.objects_missing;
    let holes = if missing > 0 {
        Some(format!(
            "{missing} of {} faces could not be drawn",
            drawn.measures.faces
        ))
    } else if unmeshed == 1 {
        Some("1 object stores no mesh and is not drawn".to_owned())
    } else if unmeshed > 1 {
        Some(format!(
            "{unmeshed} objects store no mesh and are not drawn"
        ))
    } else {
        None
    };
    Ok(match drawn.shown {
        Shown::Mesh(mesh) => Found {
            title: title(format).into(),
            mesh: Some(Arc::new(mesh)),
            details,
            mesh_available: true,
            notice: holes,
            ..Found::default()
        },
        Shown::Picture(picture) => {
            // Asked to be turned, and too large to be unasked.
            let large = matches!(asked, Asked::Turned { whole: false }).then(|| {
                format!(
                    "Shown as a picture: it has {:.1} million triangles. 3D turns it all the same",
                    drawn.measures.triangles as f64 / 1e6
                )
            });
            let notice: Vec<String> = holes.into_iter().chain(large).collect();
            Found {
                title: title(format).into(),
                picture: Some(picture),
                details,
                mesh_available: true,
                notice: (!notice.is_empty()).then(|| notice.join(" · ")),
                ..Found::default()
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measures_as_a_print_needs_them() {
        let mut measures = Measures {
            triangles: 225_706,
            objects: 1,
            size: [60.001, 31.004, 48.],
            unit: None,
            closed: true,
            volume: Some(15_550.53),
            faces: 0,
            faces_missing: 0,
            objects_missing: 0,
        };
        let said = |d: Vec<(String, String)>| {
            d.into_iter()
                .map(|(l, v)| format!("{l}: {v}"))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            said(details(MeshFormat::Stl, &measures)),
            [
                "Size: 60 × 31 × 48 mm",
                "Volume: 15.55 cm³",
                "Watertight: Yes",
                "Triangles: 225706"
            ]
        );
        // An OBJ says no unit, and none is supposed: no volume either.
        measures.objects = 3;
        assert_eq!(
            said(details(MeshFormat::Obj, &measures)),
            [
                "Size: 60 × 31 × 48",
                "Watertight: Yes",
                "Triangles: 225706",
                "Objects: 3"
            ]
        );
        measures.unit = Some("m".into());
        measures.closed = false;
        measures.volume = None;
        assert_eq!(
            said(details(MeshFormat::Gltf, &measures))[..2],
            ["Size: 60 × 31 × 48 m", "Watertight: No"]
        );
        // A model of surfaces counts its faces.
        measures.faces = 637;
        assert_eq!(
            said(details(MeshFormat::Step, &measures))[2..4],
            ["Faces: 637", "Triangles: 225706"]
        );
    }
}
