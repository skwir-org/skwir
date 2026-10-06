//! macOS's own types, Uniform Type Identifiers (ADR-037): the type the
//! system declares for an extension, its MIME name, what people call it,
//! its extensions, and what it conforms to, read as bibli's kinds.
//! Compiled, never run (ADR-035: 5b runs it).

use objc2::rc::{Retained, autoreleasepool};
use objc2_foundation::NSString;
use objc2_uniform_type_identifiers::UTType;

use super::{Kind, Mime};

pub const ASKED: bool = true;

/// What a type conforming to each of these is, of bibli's kinds: the
/// system's own groups (`UTCoreTypes`). What it leaves out (word
/// processor files, Markdown as a document) the database's kinds add.
const KINDS: [(&str, Kind); 13] = [
    ("public.image", Kind::Image),
    ("public.movie", Kind::Video),
    ("public.audio", Kind::Audio),
    ("public.spreadsheet", Kind::Spreadsheet),
    ("public.presentation", Kind::Presentation),
    ("public.archive", Kind::Archive),
    ("public.font", Kind::Font),
    ("public.executable", Kind::Program),
    ("com.apple.application", Kind::Program),
    ("public.text", Kind::Text),
    ("com.adobe.pdf", Kind::Document),
    ("org.idpf.epub-container", Kind::Document),
    ("public.rtf", Kind::Document),
];

/// The type macOS declares for files named `*.extension`, by its MIME
/// name: `None` for one it only makes up (a dynamic type, `dyn.…`) or
/// that has no MIME name.
pub fn by_extension(extension: &str) -> Option<Mime> {
    // Every call on a worker drains what the system autoreleased there.
    autoreleasepool(|_| {
        let declared = UTType::typeWithFilenameExtension(&NSString::from_str(extension))
            .filter(|t| !t.isDynamic())?;
        mime_of(&declared)
    })
}

fn mime_of(declared: &UTType) -> Option<Mime> {
    let mime = declared.preferredMIMEType()?.to_string();
    let mime = mime.trim();
    mime.contains('/').then(|| Mime(mime.to_ascii_lowercase()))
}

/// The type declared with the MIME name `m`, by this name or another.
fn declared(m: &Mime) -> Option<Retained<UTType>> {
    UTType::typeWithMIMEType(&NSString::from_str(m.as_str())).filter(|t| !t.isDynamic())
}

/// What the system calls files of type `m`, in the user's language.
pub fn description(m: &Mime) -> Option<String> {
    autoreleasepool(|_| Some(declared(m)?.localizedDescription()?.to_string()))
}

/// The name the system prefers for the type `m` is declared with.
pub fn canonical(m: &Mime) -> Option<Mime> {
    autoreleasepool(|_| -> Option<Mime> {
        let declared = declared(m)?;
        mime_of(&declared)
    })
}

/// The extensions of the type `m`, the preferred one first.
pub fn extensions(m: &Mime) -> Vec<String> {
    autoreleasepool(|_| {
        let Some(declared) = declared(m) else {
            return Vec::new();
        };
        let mut all: Vec<String> = declared
            .preferredFilenameExtension()
            .map(|e| e.to_string().to_lowercase())
            .into_iter()
            .collect();
        let tags = declared.tags();
        if let Some(listed) = tags.objectForKey(&NSString::from_str("public.filename-extension")) {
            for extension in &listed {
                let extension = extension.to_string().to_lowercase();
                if !extension.is_empty() && !all.contains(&extension) {
                    all.push(extension);
                }
            }
        }
        all
    })
}

/// The kinds of `m`: those the database gives it and those its declared
/// type conforms to.
pub fn kinds(m: &Mime, database: u16) -> u16 {
    database
        | autoreleasepool(|_| {
            let Some(declared) = declared(m) else {
                return 0;
            };
            KINDS
                .iter()
                .filter(|(id, _)| {
                    UTType::typeWithIdentifier(&NSString::from_str(id))
                        .is_some_and(|group| declared.conformsToType(&group))
                })
                .fold(0, |bits, (_, kind)| bits | kind.bit())
        })
}
