//! Meshes that carry no picture of their own (STL, OBJ, PLY, OFF, glTF, a
//! 3MF its slicer did not picture), drawn and measured by
//! `skwir-model-worker` (`skwir_platform::model`). Properties says what a
//! print needs: its size, whether it is watertight and what it holds.
use crate::Found;
use skwir_core::CancelToken;
use skwir_platform::model::{self, Asked, Measures, MeshFormat};
use std::path::Path;

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
    }
}

/// The mesh at `path` drawn at most `side` pixels on its longer side.
pub fn read(
    path: &Path,
    format: MeshFormat,
    side: u32,
    asked: Asked,
    cancel: &CancelToken,
) -> anyhow::Result<Found> {
    let drawn = model::draw(path, format, side, asked, cancel)?;
    Ok(Found {
        title: title(format).into(),
        picture: Some(drawn.picture),
        details: details(format, &drawn.measures),
        notice: None,
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
    }
}
