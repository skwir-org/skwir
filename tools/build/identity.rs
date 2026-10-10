// Which build of skwir a program is, `SKWIR_BUILD` to its code: skwir
// and each of its helpers include this in their build script and say it,
// a helper when asked with `--build`, so that a helper of another build
// can be named when a call of it fails (`skwir_platform::process::explain`).
//
// The version, and in a release build (what is run and shipped) the
// commit it was built from: a debug build (the tests) is not built again
// for a commit, and a release build only once per commit. Outside a git
// checkout (a source archive, Nix's build) the version alone.

/// Tell the crate being built its `SKWIR_BUILD`.
fn emit_build() {
    println!("cargo::rerun-if-changed=build.rs");
    let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_default();
    let release = std::env::var("PROFILE").is_ok_and(|profile| profile == "release");
    match release.then(commit).flatten() {
        Some(commit) => println!("cargo::rustc-env=SKWIR_BUILD={version}+{commit}"),
        None => println!("cargo::rustc-env=SKWIR_BUILD={version}"),
    }
}

/// The commit checked out, the first 12 digits of its hash, and the build
/// script run again when another is: HEAD's file (another branch, or a
/// commit on none), the folder of the branch's ref (a commit, its ref
/// written there, or made there from `packed-refs`) and `packed-refs`.
fn commit() -> Option<String> {
    let output = std::process::Command::new("git")
        .args([
            "rev-parse",
            "--absolute-git-dir",
            "--path-format=absolute",
            "--git-common-dir",
            "HEAD",
            "--symbolic-full-name",
            "HEAD",
        ])
        .output()
        .ok()
        .filter(|output| output.status.success())?;
    let text = String::from_utf8(output.stdout).ok()?;
    let mut lines = text.lines();
    let (git, common, hash) = (lines.next()?, lines.next()?, lines.next()?);
    let git = std::path::Path::new(git);
    let common = std::path::Path::new(common);
    let mut watched = vec![git.join("HEAD"), common.join("packed-refs")];
    if let Some(folder) = lines
        .next()
        .filter(|name| name.starts_with("refs/"))
        .and_then(|name| common.join(name).parent().map(std::path::Path::to_owned))
    {
        watched.push(folder);
    }
    for path in watched.iter().filter(|path| path.exists()) {
        println!("cargo::rerun-if-changed={}", path.display());
    }
    hash.get(..12).map(str::to_owned)
}
