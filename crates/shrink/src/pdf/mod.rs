//! A PDF made smaller (ADR-112), object by object: its images written
//! anew where they are stored (`image`), drawn where its pages draw them
//! (`place`), and the whole packed tighter (`pack`). Its pages' drawing,
//! text, fonts, links, forms, outline, tags and metadata are left as they
//! are: never rewritten, as a content stream regenerated would lose what
//! its writer does not express (CMYK, spot colours, patterns).
//!
//! What is not made smaller, and why, said in a note: a PDF protected by
//! a password (a copy would drop its protection) and a signed one (a copy
//! would break its signature).

mod image;
mod pack;
mod place;

use std::collections::BTreeSet;

use anyhow::{Context, Result};
use lopdf::{Document, Object, ObjectId};
use skwir_core::shrink::{Changed, Job, Made, PDF_STEPS, PdfRule};

/// The largest PDF read whole: past it, none is made smaller here.
const MOST_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// A smaller copy of the PDF `job.input` at `job.output`, as `job` asks;
/// none when it would not be smaller, or the PDF is one not made smaller
/// (said in the notes). `progress` is told the images gone through, of
/// all those each try goes through.
pub fn shrink(job: &Job, progress: &mut dyn FnMut(u64, u64)) -> Result<Made> {
    let size = std::fs::metadata(&job.input)
        .with_context(|| format!("{} could not be read", job.input.display()))?
        .len();
    anyhow::ensure!(
        size <= MOST_BYTES,
        "It is larger than a PDF made smaller may be"
    );
    let bytes = std::fs::read(&job.input).context("It could not be read")?;
    let not_made = |note: &str| Made {
        before: size,
        after: size,
        written: false,
        notes: vec![note.to_owned()],
        ..Made::default()
    };
    let doc = Document::load_mem(&bytes).context("It could not be read as a PDF")?;
    if doc.is_encrypted() || doc.trailer.has(b"Encrypt") {
        return Ok(not_made(
            "it is protected by a password, which a smaller copy would not keep",
        ));
    }
    match signed(&doc) {
        Some(Signed::Signature) => {
            return Ok(not_made(
                "it is signed, and a smaller copy would break its signature",
            ));
        }
        Some(Signed::Rights) => {
            return Ok(not_made(
                "it carries rights its maker signed (to fill it in and save it in Adobe's Reader), which a smaller copy would break",
            ));
        }
        None => {}
    }
    let keep: BTreeSet<u32> = job.keep.iter().copied().collect();
    let mut rules = vec![job.quality.pdf()];
    if job.at_most.is_some() && job.quality.pdf().dpi.is_some() {
        rules.extend(PDF_STEPS);
    }
    let mut best: Option<(Vec<u8>, Vec<Changed>)> = None;
    let attempts = rules.len() as u64;
    for (before, rule) in rules.into_iter().enumerate() {
        let before = before as u64;
        let mut went = |done: u64, total: u64| {
            // Each try a share of the whole; its writing out the last step.
            let steps = total + 1;
            progress(before * steps + done, attempts * steps);
        };
        let (written, changed) = attempt(doc.clone(), &rule, job.gray, &keep, &mut went)?;
        let smaller = best
            .as_ref()
            .is_none_or(|(kept, _)| written.len() < kept.len());
        let fits = job.at_most.is_none_or(|most| written.len() as u64 <= most);
        if smaller {
            best = Some((written, changed));
        }
        if fits {
            break;
        }
    }
    let Some((written, changed)) = best.filter(|(written, _)| (written.len() as u64) < size) else {
        return Ok(Made {
            before: size,
            after: size,
            written: false,
            ..Made::default()
        });
    };
    std::fs::write(&job.output, &written).context("The copy could not be written")?;
    let after = written.len() as u64;
    Ok(Made {
        before: size,
        after,
        written: true,
        met: job.at_most.map(|most| after <= most),
        changed,
        notes: Vec::new(),
    })
}

/// `doc` made smaller under `rule`: its bytes, and the images written
/// anew with the pages that show them.
fn attempt(
    mut doc: Document,
    rule: &PdfRule,
    gray: bool,
    keep: &BTreeSet<u32>,
    went: &mut dyn FnMut(u64, u64),
) -> Result<(Vec<u8>, Vec<Changed>)> {
    let placed = place::placements(&doc);
    let images: Vec<ObjectId> = doc
        .objects
        .iter()
        .filter(|(_, object)| is_image(object))
        .map(|(&id, _)| id)
        .collect();
    // Soft masks are images too, written with the image they mask.
    let masks: BTreeSet<ObjectId> = images
        .iter()
        .filter_map(|id| match doc.get_object(*id) {
            Ok(Object::Stream(stream)) => stream
                .dict
                .get(b"SMask")
                .and_then(Object::as_reference)
                .ok(),
            _ => None,
        })
        .collect();
    let mut changed = Vec::new();
    let count = images.len() as u64;
    for (looked, id) in images.into_iter().enumerate() {
        went(looked as u64, count);
        if masks.contains(&id) || keep.contains(&id.0) {
            continue;
        }
        let at = placed.get(&id);
        let dpi = at.and_then(|at| {
            let Ok(Object::Stream(stream)) = doc.get_object(id) else {
                return None;
            };
            let side = |key: &[u8]| {
                stream
                    .dict
                    .get(key)
                    .ok()
                    .and_then(|value| value.as_i64().ok())
                    .and_then(|value| u32::try_from(value).ok())
            };
            at.dpi(side(b"Width")?, side(b"Height")?)
        });
        match image::rewrite(&mut doc, id, rule, dpi, gray) {
            Ok(image::Done::Written) => changed.push(Changed {
                object: id.0,
                pages: at
                    .map(|at| at.pages.iter().copied().collect())
                    .unwrap_or_default(),
            }),
            Ok(image::Done::Kept) => {}
            // An image that cannot be read here stays as it is.
            Err(error) => eprintln!("image {} left as it is: {error:#}", id.0),
        }
    }
    // Every image gone through; what is left is writing it out.
    went(count, count);
    pack::drop_thumbnails(&mut doc);
    pack::merge_duplicates(&mut doc);
    doc.prune_objects();
    pack::deflate_best(&mut doc);
    doc.renumber_objects();
    // Object streams need PDF 1.5.
    if doc.version.as_str() < "1.5" {
        "1.5".clone_into(&mut doc.version);
    }
    let mut written = Vec::new();
    doc.save_with_options(
        &mut written,
        lopdf::SaveOptions::builder()
            .use_object_streams(true)
            .use_xref_streams(true)
            .compression_level(9)
            .build(),
    )
    .context("The copy could not be written")?;
    Ok((written, changed))
}

fn is_image(object: &Object) -> bool {
    matches!(object, Object::Stream(stream)
        if stream.dict.get(b"Subtype").and_then(Object::as_name).is_ok_and(|kind| kind == b"Image"))
}

/// What signs a PDF.
enum Signed {
    /// A signature of its form's, or one certifying it (`DocMDP`).
    Signature,
    /// Rights its maker signed alone (`UR`, `UR3`: Adobe's Reader
    /// extensions, which let its Reader fill the form in and save it).
    Rights,
}

/// What signs `doc`, if anything: a field of its form that is signed, a
/// certification, or usage rights its catalogue asks permissions of.
fn signed(doc: &Document) -> Option<Signed> {
    let catalog = doc.catalog().ok()?;
    let perms = catalog
        .get(b"Perms")
        .ok()
        .and_then(|perms| doc.dereference(perms).ok())
        .and_then(|(_, perms)| perms.as_dict().ok());
    let form = catalog
        .get(b"AcroForm")
        .ok()
        .and_then(|form| doc.dereference(form).ok())
        .and_then(|(_, form)| form.as_dict().ok());
    // SigFlags' first bit: the document holds at least one signature.
    let sig_flag = form.is_some_and(|form| {
        form.get(b"SigFlags")
            .and_then(Object::as_i64)
            .is_ok_and(|flags| flags & 1 == 1)
    });
    let sig_field = doc.objects.values().any(|object| {
        object.as_dict().is_ok_and(|dict| {
            dict.get(b"FT")
                .and_then(Object::as_name)
                .is_ok_and(|kind| kind == b"Sig")
                && dict.has(b"V")
        })
    });
    if sig_flag || sig_field || perms.is_some_and(|perms| perms.has(b"DocMDP")) {
        return Some(Signed::Signature);
    }
    perms.is_some().then_some(Signed::Rights)
}
