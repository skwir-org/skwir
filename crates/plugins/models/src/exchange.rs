//! CAD exchange files, STEP and IGES, meshed and drawn by
//! `skwir-model-worker` with ogeom (ADR-069), with what their headers say
//! (`step`, `iges`). Such a file holds no picture. Reading one costs the
//! helper some ten to fifty times the file's size, so the quick bound is a
//! file's (`Bounds::binary_input`): past it, Full preview draws it.
use crate::Found;
use skwir_core::{CancelToken, preview::FileSource};
use skwir_platform::inspection::{self, limits, limits::Bounds};
use skwir_platform::model::{Asked, MeshFormat};

/// The header read at most: a few hundred bytes in practice, more when a
/// program writes a long description.
const HEADER: usize = 64 * 1024;

/// The model `source` holds, in `format`, drawn at most `side` pixels on
/// its longer side, or its mesh when it is asked to be turned.
pub fn read<S: FileSource + ?Sized>(
    source: &S,
    format: MeshFormat,
    side: u32,
    asked: Asked,
    bounds: Bounds,
    cancel: &CancelToken,
) -> anyhow::Result<Found> {
    let most = bounds.binary_input();
    if source.length()? > most as u64 {
        return Err(bounds.refuse(format!(
            "Too large to draw (more than {})",
            limits::describe(most)
        )));
    }
    let head = inspection::read(source, 0, HEADER, cancel)?;
    let head = String::from_utf8_lossy(&head);
    let mut details = match format {
        MeshFormat::Iges => crate::iges::header(&head),
        _ => crate::step::header(&head),
    };
    let mut drawn = crate::mesh::read(source, format, side, asked, cancel)?;
    details.append(&mut drawn.details);
    drawn.details = details;
    Ok(drawn)
}
