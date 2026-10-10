//! The glob index of the vendored xdg-mime answers every name as trying
//! each pattern of the database does: the same types, in the same order,
//! for names made from every pattern the desktop's database holds, in
//! every case, and for names no pattern was written for. Linux's alone:
//! the database is read from the free desktops' folders in tests
//! (ADR-035).
#![cfg(target_os = "linux")]

use std::collections::BTreeSet;

/// Names that exercise each pattern of `globs2`: a plain one, its case
/// changed, with more before it; a literal as it is and in capitals.
fn names_from(glob: &str) -> Vec<String> {
    let mut names = Vec::new();
    if let Some(suffix) = glob.strip_prefix('*')
        && !suffix.contains(['*', '?', '[', '\\'])
    {
        names.push(format!("file{suffix}"));
        names.push(format!("FILE{}", suffix.to_uppercase()));
        names.push(format!("a.b{suffix}"));
        names.push(format!("Éte{suffix}"));
        return names;
    }
    if !glob.contains(['*', '?', '[', '\\']) {
        names.push(glob.to_owned());
        names.push(glob.to_uppercase());
        names.push(format!("x{glob}"));
        return names;
    }
    // A full glob: what it would match with its wildcards filled in.
    names.push(glob.replace('*', "abc").replace('?', "x"));
    names
}

#[test]
fn the_index_answers_as_every_pattern_tried_does() {
    let dirs = skwir_platform::xdg::data_search();
    let db = xdg_mime::SharedMimeInfo::new_for_directories(&dirs);
    let mut names: BTreeSet<String> = [
        "",
        "README",
        "Makefile.am",
        "notes.TXT",
        "x.tar.gz",
        "café.JPG",
        ".bashrc",
        "core",
        "CMakeLists.txt",
        "ẞ.pdf",
        "İ.png",
        "no-extension.",
        "*",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    let mut patterns = 0;
    for dir in &dirs {
        let Ok(text) = std::fs::read_to_string(dir.join("mime/globs2")) else {
            continue;
        };
        for line in text.lines().filter(|l| !l.starts_with('#')) {
            if let Some(glob) = line.split(':').nth(2) {
                patterns += 1;
                names.extend(names_from(glob));
            }
        }
    }
    if patterns == 0 {
        eprintln!("no type database here: nothing to compare");
        return;
    }
    for name in &names {
        assert_eq!(
            db.get_mime_types_from_file_name(name),
            db.get_mime_types_from_file_name_linear(name),
            "{name:?}"
        );
    }
}
