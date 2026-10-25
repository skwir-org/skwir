// Which build of skwir a program is, `SKWIR_BUILD` to its code: skwir
// and each of its helpers include this in their build script and say it,
// a helper when asked with `--build`, so that a helper of another build
// can be named when a call of it fails (`skwir_platform::process::explain`).
//
// The version, and in a release build (what is run and shipped) the
// commit it was built from, unless the version's own tag (`v0.2.0`) names
// it: a release says its version alone, any other commit says which it is
// (About, `skwir --version`). A debug build (the tests) is not built again
// for a commit, and a release build only once per commit. Outside a git
// checkout of skwir (a source archive, Nix's build, the release's archive
// unpacked inside the AUR's own checkout) the version alone.

/// Tell the crate being built its `SKWIR_BUILD`.
fn emit_build() {
    println!("cargo::rerun-if-changed=build.rs");
    let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_default();
    let release = std::env::var("PROFILE").is_ok_and(|profile| profile == "release");
    match release.then(|| commit(&version)).flatten() {
        Some(commit) => println!("cargo::rustc-env=SKWIR_BUILD={version}+{commit}"),
        None => println!("cargo::rustc-env=SKWIR_BUILD={version}"),
    }
}

/// The commit checked out, the first 12 digits of its hash, and the build
/// script run again when another is: HEAD's file (another branch, or a
/// commit on none), the folder of the branch's ref (a commit, its ref
/// written there, or made there from `packed-refs`), the tags' folder (a
/// tag made) and `packed-refs`. `None` for the commit `version`'s tag
/// names, and unless the checkout is skwir's own, whose top holds this
/// file: git answers for whatever repository the folder is in.
fn commit(version: &str) -> Option<String> {
    let output = std::process::Command::new("git")
        .args([
            "rev-parse",
            "--show-toplevel",
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
    let top = std::path::Path::new(lines.next()?);
    if !top.join("tools/build/identity.rs").is_file() {
        return None;
    }
    let (git, common, hash) = (lines.next()?, lines.next()?, lines.next()?);
    let git = std::path::Path::new(git);
    let common = std::path::Path::new(common);
    let mut watched = vec![
        git.join("HEAD"),
        common.join("packed-refs"),
        common.join("refs/tags"),
    ];
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
    let tag = format!("refs/tags/v{version}^{{commit}}");
    let tagged = std::process::Command::new("git")
        .args(["rev-parse", "--verify", "--quiet", &tag])
        .output()
        .is_ok_and(|output| output.stdout.trim_ascii() == hash.as_bytes());
    if tagged {
        return None;
    }
    hash.get(..12).map(str::to_owned)
}
