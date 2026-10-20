// Which build of skwir this program is (`SKWIR_BUILD`).
include!("../../tools/build/identity.rs");

fn main() {
    emit_build();
    link_libarchive();
}

/// libarchive: the system's, or where `SKWIR_LIBARCHIVE_STATIC` names a
/// folder `tools/package/libarchive.sh` made, linked statically from it
/// with what it says the libraries need (the system's own: Windows'
/// cryptography, macOS's zlib), as Windows' and macOS's packages carry
/// it (ADR-041).
fn link_libarchive() {
    println!("cargo::rerun-if-env-changed=SKWIR_LIBARCHIVE_STATIC");
    let Some(folder) = std::env::var_os("SKWIR_LIBARCHIVE_STATIC") else {
        println!("cargo::rustc-link-lib=archive");
        return;
    };
    let folder = std::path::PathBuf::from(folder);
    let read = |name: &str| {
        let path = folder.join(name);
        println!("cargo::rerun-if-changed={}", path.display());
        std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
    };
    let target = std::env::var("TARGET").expect("cargo names the target");
    let built = read("target");
    assert!(
        built.trim() == target,
        "SKWIR_LIBARCHIVE_STATIC is libarchive built for {}, not {target}",
        built.trim()
    );
    println!(
        "cargo::rustc-link-search=native={}",
        folder.join("lib").display()
    );
    for library in read("link").lines().filter(|line| !line.is_empty()) {
        println!("cargo::rustc-link-lib={library}");
    }
}
