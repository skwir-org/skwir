//! Meshes that carry no picture of their own (STL, OBJ, PLY, OFF, glTF, a
//! 3MF its slicer did not picture, a STEP or IGES model's faces, the meshes a Rhino
//! model stores), drawn and measured by
//! `skwir-model-worker` (`skwir_platform::model`), or read whole for a
//! window that draws and turns them (ADR-067). Properties says what a print
//! needs: its size, whether it is watertight and what it holds.
use crate::Found;
use skwir_core::{CancelToken, preview::FileSource, words};
use skwir_platform::model::{self, Asked, Measures, MeshFormat, Shown};
use std::sync::Arc;

/// The unit's volume as it is usually said, from a length unit cubed.
fn volume(value: f64, unit: &str) -> Option<String> {
    let short = crate::gcode::short;
    let said = match unit {
        "µm" => words!(
            models_volume_mm3 = "{ $volume } mm³",
            volume = short(value / 1e9),
            note = "A model's volume in cubic millimetres, in the preview's Properties. volume: the number."
        ),
        "mm" => words!(
            models_volume_cm3 = "{ $volume } cm³",
            volume = short(value / 1e3),
            note = "A model's volume in cubic centimetres, in the preview's Properties. volume: the number."
        ),
        "cm" => words!(
            models_volume_cm3 = "{ $volume } cm³",
            volume = short(value),
            note = "A model's volume in cubic centimetres, in the preview's Properties. volume: the number."
        ),
        "m" => words!(
            models_volume_m3 = "{ $volume } m³",
            volume = short(value),
            note = "A model's volume in cubic metres, in the preview's Properties. volume: the number."
        ),
        "in" => words!(
            models_volume_in3 = "{ $volume } in³",
            volume = short(value),
            note = "A model's volume in cubic inches, in the preview's Properties. volume: the number."
        ),
        "ft" => words!(
            models_volume_ft3 = "{ $volume } ft³",
            volume = short(value),
            note =
                "A model's volume in cubic feet, in the preview's Properties. volume: the number."
        ),
        _ => return None,
    };
    Some(said.say())
}

/// What the measures say, in the order Properties shows them.
pub fn details(format: MeshFormat, measures: &Measures) -> Vec<(String, String)> {
    // An STL states no unit; every slicer reads it in millimetres.
    let unit = measures
        .unit
        .clone()
        .or_else(|| (format == MeshFormat::Stl).then(|| "mm".to_owned()));
    let size = crate::size_3d(measures.size.map(crate::gcode::short), unit.as_deref());
    let mut out = vec![(crate::size_label(), size)];
    if let (Some(value), Some(unit)) = (measures.volume, &unit)
        && let Some(said) = volume(value, unit)
    {
        out.push((
            words!(
                models_detail_volume = "Volume",
                note = "A label in the preview's Properties: how much space the 3D model encloses."
            )
            .say(),
            said,
        ));
    }
    let closed = if measures.closed {
        words!(
            models_watertight_yes = "Yes",
            note = "The Watertight row's value in the preview's Properties: the model's mesh is closed."
        )
    } else {
        words!(
            models_watertight_no = "No",
            note = "The Watertight row's value in the preview's Properties: the model's mesh has holes."
        )
    };
    out.push((
        words!(
            models_detail_watertight = "Watertight",
            note = "A label in the preview's Properties: whether the model's mesh is closed, with no holes, as a 3D print needs."
        )
        .say(),
        closed.say(),
    ));
    if measures.faces > 0 {
        out.push((
            words!(
                models_detail_faces = "Faces",
                note = "A label in the preview's Properties: how many surfaces a CAD model (STEP, IGES) is made of."
            )
            .say(),
            measures.faces.to_string(),
        ));
    }
    out.push((
        words!(
            models_detail_triangles = "Triangles",
            note = "A label in the preview's Properties: how many triangles the model's mesh has."
        )
        .say(),
        measures.triangles.to_string(),
    ));
    if measures.objects > 1 {
        out.push((
            words!(
                models_detail_objects = "Objects",
                note = "A label in the preview's Properties: how many separate objects the model holds."
            )
            .say(),
            measures.objects.to_string(),
        ));
    }
    out
}

/// What the model is, by its format's name, for the toolbar.
fn title(format: MeshFormat) -> String {
    // The formats' names, the same in every language.
    let name = match format {
        MeshFormat::Stl => "STL",
        MeshFormat::Obj => "OBJ",
        MeshFormat::Ply => "PLY",
        MeshFormat::Off => "OFF",
        MeshFormat::Gltf => "glTF",
        MeshFormat::ThreeMf => return "3MF".to_owned(),
        MeshFormat::Step => "STEP",
        MeshFormat::Iges => "IGES",
        MeshFormat::Rhino => "Rhino",
    };
    crate::model_title(name)
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
        Some(
            words!(
                models_faces_missing = "{ $missing } of { $count ->
                   *[other] { $count } faces
                } could not be drawn",
                missing = missing,
                count = drawn.measures.faces,
                note = "Said under a CAD model drawn with holes: some of its surfaces could not be turned into triangles. missing: how many; count: how many surfaces the model has."
            )
            .say(),
        )
    } else if unmeshed > 0 {
        Some(
            words!(
                models_objects_unmeshed = "{ $count ->
                    [one] { $count } object stores no mesh and is not drawn
                   *[other] { $count } objects store no mesh and are not drawn
                }",
                count = unmeshed,
                note = "Said under a Rhino model drawn from the meshes it keeps: objects that keep none are left out."
            )
            .say(),
        )
    } else {
        None
    };
    Ok(match drawn.shown {
        Shown::Mesh(mesh) => Found {
            title: title(format),
            mesh: Some(Arc::new(mesh)),
            details,
            mesh_available: true,
            notice: holes,
            ..Found::default()
        },
        Shown::Picture(picture) => {
            // Asked to be turned, and too large to be unasked.
            let large = matches!(asked, Asked::Turned { whole: false }).then(|| {
                words!(
                    models_shown_as_picture = "Shown as a picture: it has { $millions } million triangles. 3D turns it all the same",
                    millions = format!("{:.1}", drawn.measures.triangles as f64 / 1e6),
                    note = "Said under a model too large to turn at once: a still picture is shown, and the 3D button turns it anyway. millions: the number of triangles in millions (2.4)."
                )
                .say()
            });
            let notice: Vec<String> = holes.into_iter().chain(large).collect();
            Found {
                title: title(format),
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
