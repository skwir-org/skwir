//! A PDF made smaller (ADR-112), object by object: its images written
//! anew where they are stored (`image`), drawn where its pages draw them
//! (`place`), and the whole packed tighter (`pack`). Its pages' drawing,
//! text, fonts, links, forms, outline, tags and metadata are left as they
//! are: never rewritten, as a content stream regenerated would lose what
//! its writer does not express (CMYK, spot colours, patterns).
//!
//! What is not made smaller, and why, said in a note: a PDF protected by
//! a password (a copy would drop its protection), a signed one (a copy
//! would break its signature), and a damaged one, whose copy would lose
//! what a reader repairs as it shows it.

mod image;
mod pack;
mod place;

use std::collections::BTreeSet;

use anyhow::{Context, Result};
use lopdf::xref::{XrefEntry, XrefRecovery};
use lopdf::{Document, LoadOptions, Object, ObjectId, Stream};
use skwir_core::shrink::{Changed, Job, Made, PdfRule};

/// The largest PDF read whole: past it, none is made smaller here.
const MOST_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// The most a stream of objects or of cross-references is inflated to as
/// the PDF is read: one holding more is not read.
const INFLATED: usize = 256 * 1024 * 1024;

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
    let not_made = |note: &str| Made {
        before: size,
        after: size,
        written: false,
        notes: vec![note.to_owned()],
        ..Made::default()
    };
    // Its bytes let go of once read: what is kept is what was read of it.
    let mut doc = {
        let bytes = std::fs::read(&job.input).context("It could not be read")?;
        Document::load_mem_with_options(&bytes, LoadOptions::with_max_decompressed_size(INFLATED))
            .context("It could not be read as a PDF")?
    };
    // A PDF protected by an owner password alone (its user password
    // empty) is read decrypted, its `/Encrypt` taken out: it was encrypted
    // all the same, and a copy would no longer forbid what it forbids.
    if doc.was_encrypted() || doc.is_encrypted() || doc.trailer.has(b"Encrypt") {
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
    if damaged(&doc) {
        return Ok(not_made(
            "it is damaged, and a smaller copy would lose what a reader repairs as it shows it",
        ));
    }
    // What every try shares, done once: the pages' thumbnails and what
    // nothing reaches let go of, what is not an image packed tighter, and
    // where the images are drawn. Nothing new is told meanwhile: the start
    // is said again, so the one waiting knows it still goes.
    let mut still = || progress(0, 1);
    pack::drop_thumbnails(&mut doc);
    doc.prune_objects();
    pack::deflate_best(&mut doc, |_, stream| !is_image_stream(stream), &mut still);
    let placed = place::placements(&doc, &mut still);
    let keep: BTreeSet<u32> = job.keep.iter().copied().collect();
    let mut rules: Vec<(u32, PdfRule)> = std::iter::once(job.quality.pdf())
        .chain(
            job.at_most
                .map(|_| job.quality.pdf_steps())
                .unwrap_or_default(),
        )
        .enumerate()
        .map(|(at, rule)| (u32::try_from(at).unwrap_or(u32::MAX), rule))
        .collect();
    // A pass after images were put back keeps the rule the first kept.
    if let Some(only) = job.rule {
        rules.retain(|(at, _)| *at == only);
        anyhow::ensure!(!rules.is_empty(), "No rule {only} is tried for this job");
    }
    let mut best: Option<(Vec<u8>, Vec<Changed>, u32)> = None;
    let attempts = rules.len() as u64;
    let last = rules.len() - 1;
    let mut doc = Some(doc);
    for (before, (at, rule)) in rules.into_iter().enumerate() {
        // The last try takes the document as read, the others a copy.
        let tried = if before == last {
            doc.take()
        } else {
            doc.clone()
        };
        let Some(tried) = tried else { break };
        let before = before as u64;
        let mut went = |done: u64, total: u64| {
            // Each try a share of the whole; its writing out the last step.
            let steps = total + 1;
            progress(before * steps + done, attempts * steps);
        };
        let (written, changed) = attempt(tried, &rule, &placed, job.gray, &keep, &mut went)?;
        let smaller = best
            .as_ref()
            .is_none_or(|(kept, _, _)| written.len() < kept.len());
        let fits = job.at_most.is_none_or(|most| written.len() as u64 <= most);
        if smaller {
            best = Some((written, changed, at));
        }
        if fits {
            break;
        }
    }
    let Some((written, changed, rule)) =
        best.filter(|(written, _, _)| (written.len() as u64) < size)
    else {
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
        rule: Some(rule),
    })
}

/// Whether `doc` was read other than as it was written: its table of
/// cross-references rebuilt by scanning the file (objects may be missing
/// or an older revision's), or an object it reaches, which the table
/// lists, not read (one a reader repairs as it shows it). A copy is
/// written from what was read: it would lose what was not.
fn damaged(doc: &Document) -> bool {
    if matches!(doc.xref_recovery, Some(XrefRecovery::Reconstructed)) {
        return true;
    }
    let listed = |id: &ObjectId| {
        matches!(
            doc.reference_table.entries.get(&id.0),
            Some(XrefEntry::Normal { .. } | XrefEntry::Compressed { .. })
        )
    };
    // What the trailer reaches, each object once.
    let mut seen: BTreeSet<ObjectId> = BTreeSet::new();
    let mut stack: Vec<&Object> = doc.trailer.iter().map(|(_, value)| value).collect();
    while let Some(object) = stack.pop() {
        match object {
            Object::Reference(id) => match doc.objects.get(id) {
                Some(object) if seen.insert(*id) => stack.push(object),
                None if listed(id) => return true,
                _ => {}
            },
            Object::Array(items) => stack.extend(items),
            Object::Dictionary(dict) => stack.extend(dict.iter().map(|(_, value)| value)),
            Object::Stream(stream) => stack.extend(stream.dict.iter().map(|(_, value)| value)),
            _ => {}
        }
    }
    false
}

/// `doc` made smaller under `rule`, its images drawn as `placed` says:
/// its bytes, and the images written anew with the pages that show them.
fn attempt(
    mut doc: Document,
    rule: &PdfRule,
    placed: &std::collections::BTreeMap<ObjectId, place::Placed>,
    gray: bool,
    keep: &BTreeSet<u32>,
    went: &mut dyn FnMut(u64, u64),
) -> Result<(Vec<u8>, Vec<Changed>)> {
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
    // What was written anew (masks resized with their images included),
    // at its best already.
    let mut anew: BTreeSet<ObjectId> = BTreeSet::new();
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
            Ok(image::Done::Written(written)) => {
                anew.extend(written);
                changed.push(Changed {
                    object: id.0,
                    pages: at
                        .map(|at| at.pages.iter().copied().collect())
                        .unwrap_or_default(),
                });
            }
            Ok(image::Done::Kept) => {}
            // An image that cannot be read here stays as it is.
            Err(error) => eprintln!("image {} left as it is: {error:#}", id.0),
        }
    }
    // Every image gone through; what is left is writing it out.
    went(count, count);
    pack::merge_duplicates(&mut doc);
    // What an image made gray named no longer (its colour profile).
    doc.prune_objects();
    pack::deflate_best(
        &mut doc,
        |id, stream| is_image_stream(stream) && !anew.contains(id),
        &mut || went(count, count),
    );
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

fn is_image_stream(stream: &Stream) -> bool {
    stream
        .dict
        .get(b"Subtype")
        .and_then(Object::as_name)
        .is_ok_and(|kind| kind == b"Image")
}

fn is_image(object: &Object) -> bool {
    matches!(object, Object::Stream(stream) if is_image_stream(stream))
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

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::{Dictionary, Stream, dictionary};
    use skwir_core::shrink::{Kind, Quality};

    /// A folder for a test's files on the home filesystem.
    fn scratch() -> tempfile::TempDir {
        let base = std::env::home_dir()
            .expect("a home")
            .join(".cache")
            .join("skwir-tests");
        std::fs::create_dir_all(&base).unwrap();
        tempfile::Builder::new()
            .prefix("shrink-")
            .tempdir_in(base)
            .unwrap()
    }

    /// A page drawing a gray gradient of 1200 × 1200 pixels over 2 inches
    /// (600 dpi), which Screen brings to 150 dpi.
    fn gradient_pdf() -> Document {
        let mut doc = Document::with_version("1.5");
        let side = 1200u32;
        let samples: Vec<u8> = (0..side * side)
            .map(|n| u8::try_from((n % side + n / side) * 255 / (2 * side)).unwrap())
            .collect();
        let image = Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Image",
                "Width" => i64::from(side),
                "Height" => i64::from(side),
                "ColorSpace" => "DeviceGray",
                "BitsPerComponent" => 8,
                "Filter" => "FlateDecode",
            },
            crate::samples::deflate(&samples),
        );
        let image_id = doc.add_object(image);
        let content = doc.add_object(Stream::new(
            Dictionary::new(),
            b"q 144 0 0 144 72 72 cm /Im0 Do Q".to_vec(),
        ));
        let tree_id = doc.new_object_id();
        let page_id = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => tree_id,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            "Contents" => content,
            "Resources" => dictionary! { "XObject" => dictionary! { "Im0" => image_id } },
        });
        doc.objects.insert(
            tree_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![page_id.into()],
                "Count" => 1,
            }),
        );
        let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => tree_id });
        doc.trailer.set("Root", catalog);
        let id = Object::String(
            b"0123456789abcdef".to_vec(),
            lopdf::StringFormat::Hexadecimal,
        );
        doc.trailer.set("ID", Object::Array(vec![id.clone(), id]));
        doc
    }

    fn job(input: &std::path::Path, output: &std::path::Path) -> Job {
        Job {
            kind: Kind::Pdf,
            input: input.to_owned(),
            output: output.to_owned(),
            quality: Quality::Screen,
            gray: false,
            strip: false,
            at_most: None,
            keep: Vec::new(),
            rule: None,
        }
    }

    /// A PDF of `objects` (each "N 0 obj" … "endobj" in order from 1) with
    /// a table of cross-references listing each where it is.
    fn written_by_hand(objects: &[&str]) -> Vec<u8> {
        let mut pdf = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::new();
        for (n, body) in objects.iter().enumerate() {
            offsets.push(pdf.len());
            pdf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", n + 1).as_bytes());
        }
        let xref = pdf.len();
        pdf.extend_from_slice(
            format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
        );
        for offset in offsets {
            pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        pdf.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                objects.len() + 1
            )
            .as_bytes(),
        );
        pdf
    }

    /// A page's drawing lopdf cannot read is lost to a copy written from
    /// what it read: the PDF is said damaged, and not made smaller.
    #[test]
    fn a_pdf_with_an_object_not_read_is_not_made_smaller() {
        let page = |contents: &str| {
            written_by_hand(&[
                "<< /Type /Catalog /Pages 2 0 R >>",
                "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R >>",
                contents,
            ])
        };
        let whole = page("<< /Length 11 >>\nstream\n0 0 m 9 9 l\nendstream");
        let broken = page("<< /Length 11 /Filter [ >>\nstream\n0 0 m 9 9 l\nendstream");
        assert!(!damaged(&Document::load_mem(&whole).unwrap()));
        let read = Document::load_mem(&broken).unwrap();
        assert!(
            !read.objects.contains_key(&(4, 0)),
            "lopdf read it after all"
        );
        assert!(damaged(&read));
        let dir = scratch();
        let input = dir.path().join("broken.pdf");
        std::fs::write(&input, &broken).unwrap();
        let copy = dir.path().join("copy.pdf");
        let made = shrink(&job(&input, &copy), &mut |_, _| {}).unwrap();
        assert!(!made.written && !copy.exists());
        assert!(made.notes.iter().any(|note| note.contains("damaged")));
    }

    /// The rule a copy was made by is said, and a job can ask that one
    /// alone: a second pass makes the copy the first did.
    #[test]
    fn a_pass_keeps_the_rule_asked() {
        let dir = scratch();
        let input = dir.path().join("gradient.pdf");
        gradient_pdf().save(&input).unwrap();
        let copy = dir.path().join("copy.pdf");
        let made = shrink(&job(&input, &copy), &mut |_, _| {}).unwrap();
        assert_eq!(made.rule, Some(0));
        let first = std::fs::read(&copy).unwrap();
        std::fs::remove_file(&copy).unwrap();
        let tight = Job {
            at_most: Some(1),
            ..job(&input, &copy)
        };
        let made = shrink(&tight, &mut |_, _| {}).unwrap();
        assert!(made.rule > Some(0), "{:?}", made.rule);
        assert_eq!(made.met, Some(false));
        std::fs::remove_file(&copy).unwrap();
        let pinned = Job {
            rule: Some(0),
            ..tight
        };
        let made = shrink(&pinned, &mut |_, _| {}).unwrap();
        assert_eq!(made.rule, Some(0));
        assert_eq!(std::fs::read(&copy).unwrap(), first);
    }

    /// A PDF protected by an owner password alone opens without one, and
    /// lopdf reads it decrypted: it is still not made smaller, as a copy
    /// would drop what it forbids. The same PDF unprotected is.
    #[test]
    fn a_pdf_with_an_owner_password_alone_is_not_made_smaller() {
        let dir = scratch();
        let plain = dir.path().join("plain.pdf");
        gradient_pdf().save(&plain).unwrap();
        let mut doc = gradient_pdf();
        let state = lopdf::EncryptionState::try_from(lopdf::EncryptionVersion::V2 {
            document: &doc,
            owner_password: "owner",
            user_password: "",
            key_length: 128,
            permissions: lopdf::Permissions::PRINTABLE,
        })
        .unwrap();
        doc.encrypt(&state).unwrap();
        let protected = dir.path().join("protected.pdf");
        doc.save(&protected).unwrap();
        let read = Document::load(&protected).unwrap();
        assert!(read.was_encrypted() && !read.trailer.has(b"Encrypt"));

        let copy = dir.path().join("copy.pdf");
        let made = shrink(&job(&plain, &copy), &mut |_, _| {}).unwrap();
        assert!(made.written, "the plain PDF is made smaller");
        std::fs::remove_file(&copy).unwrap();
        let made = shrink(&job(&protected, &copy), &mut |_, _| {}).unwrap();
        assert!(!made.written && !copy.exists());
        assert!(
            made.notes.iter().any(|note| note.contains("password")),
            "{:?}",
            made.notes
        );
    }
}
