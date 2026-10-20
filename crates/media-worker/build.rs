// Which build of skwir this program is (`SKWIR_BUILD`).
include!("../../tools/build/identity.rs");

fn main() {
    emit_build();
    // On a Mac, GStreamer's libraries are named by `@rpath`: the helper
    // finds them in the framework in the bundle's Frameworks (macOS's
    // package, ADR-076), else where GStreamer's own installer puts it.
    if std::env::var("CARGO_CFG_TARGET_OS").is_ok_and(|os| os == "macos") {
        for path in [
            "@executable_path/../Frameworks/GStreamer.framework/Versions/1.0/lib",
            "/Library/Frameworks/GStreamer.framework/Versions/1.0/lib",
        ] {
            println!("cargo::rustc-link-arg-bins=-Wl,-rpath,{path}");
        }
    }
}
