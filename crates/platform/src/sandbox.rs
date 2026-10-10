//! Converters, thumbnailers and the document readers' helpers run on a
//! file nobody vouched for, isolated from everything else (ADR-003,
//! ADR-030, ADR-031, ADR-039, ADR-041): no network,
//! only the system runtime and the input readable, and only a disposable
//! working folder writable. On Linux this is bubblewrap; on macOS the
//! system's own sandbox, `sandbox-exec` with a profile of skwir's; on
//! Windows nothing yet (`AppContainer`, ADR-039). A sandbox that cannot
//! start is an error: a conversion is never retried without it.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

/// The program that isolates others here, for a plugin's requirements:
/// what the settings say is missing when nothing can.
pub const PROGRAM: Option<&str> = if cfg!(target_os = "linux") {
    Some("bwrap")
} else if cfg!(target_os = "macos") {
    Some("sandbox-exec")
} else {
    None
};

/// What every isolated program may read, where it exists: the system's
/// programs, libraries and fonts. `/etc/alternatives` holds Debian's links
/// to programs; `/run/current-system` is NixOS's.
#[cfg(target_os = "linux")]
const SYSTEM: &[&str] = &[
    "/nix/store",
    "/run/current-system",
    "/usr",
    "/bin",
    "/sbin",
    "/lib",
    "/lib32",
    "/lib64",
    "/etc/alternatives",
    "/etc/fonts",
    "/etc/ld.so.cache",
    "/etc/localtime",
    "/var/cache/fontconfig",
];

/// Whether a program can run isolated here: on Linux, whether bubblewrap
/// is installed and can make its namespaces, which a container or a
/// kernel that restricts them refuses; on macOS, whether `sandbox-exec`
/// runs a program under skwir's profile. Asked once, on a worker.
pub fn available() -> bool {
    static AVAILABLE: OnceLock<bool> = OnceLock::new();
    *AVAILABLE.get_or_init(isolates)
}

#[cfg(target_os = "linux")]
fn isolates() -> bool {
    let Ok(bwrap) = crate::process::required("bwrap") else {
        return false;
    };
    let isolated = crate::process::run(
        Command::new(bwrap).args([
            "--unshare-all",
            "--die-with-parent",
            "--ro-bind",
            "/",
            "/",
            "--",
            "true",
        ]),
        &skwir_core::CancelToken::new(),
        std::time::Duration::from_secs(5),
    );
    if let Err(error) = &isolated {
        tracing::debug!("bubblewrap cannot isolate here: {error:#}");
    }
    isolated.is_ok()
}

#[cfg(target_os = "macos")]
fn isolates() -> bool {
    // The profile itself, on a program and a file of the system's: one
    // this macOS no longer reads (or no sandbox-exec) isolates nothing.
    let isolated = tempfile::tempdir()
        .map_err(anyhow::Error::from)
        .and_then(|work| {
            let mut run = isolated(
                Path::new("/usr/bin/true"),
                Path::new("/etc/hosts"),
                &[],
                work.path(),
                &[],
            )?;
            crate::process::run(
                &mut run.command,
                &skwir_core::CancelToken::new(),
                std::time::Duration::from_secs(5),
            )
        });
    if let Err(error) = &isolated {
        tracing::debug!("sandbox-exec cannot isolate here: {error:#}");
    }
    isolated.is_ok()
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn isolates() -> bool {
    false
}

/// Why a program that must run isolated does not run here.
fn needed() -> anyhow::Error {
    if cfg!(target_os = "linux") {
        anyhow::anyhow!("This needs bubblewrap, to run isolated from your files")
    } else if cfg!(target_os = "macos") {
        anyhow::anyhow!("This needs the system's sandbox, to run isolated from your files")
    } else {
        skwir_core::system::not_yet("Running a converter isolated from your files").into()
    }
}

/// The name `input` has inside the sandbox: `document`, then its
/// extension kept to ASCII letters and digits, so no program reads a
/// bracket, a percent sign or a colon of it as syntax.
pub fn inside_name(input: &Path) -> String {
    let extension: String = crate::inspection::extension(input)
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .take(16)
        .collect();
    if extension.is_empty() {
        "/input/document".to_owned()
    } else {
        format!("/input/document.{extension}")
    }
}

/// What becomes of a program run on a file nobody vouched for where the
/// system cannot isolate it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Otherwise {
    /// It does not run: a converter (`LibreOffice`, a Pandoc without its
    /// own sandbox) or `ImageMagick`, which a file can make do anything
    /// they can.
    Refuse,
    /// It runs as it would without skwir: a decoder the desktop runs on
    /// the same files all the same (`ffmpeg`, Poppler's tools, skwir's
    /// own helpers, a desktop thumbnailer), or a program that keeps itself
    /// to the files it is named (Pandoc's `--sandbox`, ADR-039).
    Run,
}

/// A program set to read a file nobody vouched for, and the paths to give
/// it, which inside a sandbox may not be the real ones (bubblewrap's
/// `/input` and `/work`).
pub struct Untrusted {
    pub command: Command,
    /// Where it reads the input.
    pub input: PathBuf,
    /// Its working folder, where it may write: what it writes there is in
    /// the real `work` folder.
    pub work: PathBuf,
    /// Whether it runs isolated.
    pub isolated: bool,
}

/// `program` set to read `input`, a file nobody vouched for, and to write
/// in `work` (its current folder): isolated where the system can
/// (`command`), else as `otherwise` says (`plain`). The one way skwir runs
/// a program on a file that is not ours; the caller adds the program's
/// arguments, naming the input and the folder as returned.
pub fn untrusted(
    program: &Path,
    input: &Path,
    work: &Path,
    otherwise: Otherwise,
) -> anyhow::Result<Untrusted> {
    untrusted_reading(program, input, work, otherwise, &[])
}

/// `untrusted`, the program also reading `readable` where it runs
/// isolated: files or folders it needs beside the system's (a helper's own
/// folder, where a package carries its libraries; a library the settings
/// name), read-only at their own paths. One that does not exist is left
/// out.
pub fn untrusted_reading(
    program: &Path,
    input: &Path,
    work: &Path,
    otherwise: Otherwise,
    readable: &[&Path],
) -> anyhow::Result<Untrusted> {
    untrusted_beside(program, input, &[], work, otherwise, readable)
}

/// `untrusted_reading`, with files the input names beside it (a glTF
/// model's buffers): each `(path, relative)` readable where the program
/// reads the input, at `relative` from it, as it is from the input
/// outside. The caller checks that each `relative` stays below the
/// input's folder; one that does not exist is left out.
pub fn untrusted_beside(
    program: &Path,
    input: &Path,
    beside: &[(PathBuf, PathBuf)],
    work: &Path,
    otherwise: Otherwise,
    readable: &[&Path],
) -> anyhow::Result<Untrusted> {
    if available() {
        return isolated(program, input, beside, work, readable);
    }
    if otherwise == Otherwise::Refuse {
        return Err(needed());
    }
    Ok(plain(program, input, work))
}

/// `program` set to read `input` and write in `work`, its current folder,
/// as it would without skwir: not isolated. Only through `untrusted`'s
/// `Otherwise::Run`, and for tests of what runs so.
pub fn plain(program: &Path, input: &Path, work: &Path) -> Untrusted {
    let mut command = Command::new(program);
    command.current_dir(work);
    Untrusted {
        command,
        input: input.to_owned(),
        work: work.to_owned(),
        isolated: false,
    }
}

/// `program` set to run isolated: `input` readable at the path returned
/// (named as `inside_name` says), and its working folder, `work`, writable
/// at the path returned, also its home, cache and current folder. The
/// caller adds the program's own arguments, naming the paths returned,
/// and runs it with `process::run`, whose cancellation kills the sandbox
/// with it. An error where the system cannot isolate (`available`).
pub fn command(program: &Path, input: &Path, work: &Path) -> anyhow::Result<Untrusted> {
    if !available() {
        return Err(needed());
    }
    isolated(program, input, &[], work, &[])
}

/// Bubblewrap: the system's folders read-only, the input at `/input`, the
/// working folder at `/work`, nothing else; no network, no session. A
/// program outside the system's folders is bound at its own path, and so
/// is what it also reads (`readable`).
#[cfg(target_os = "linux")]
fn isolated(
    program: &Path,
    input: &Path,
    beside: &[(PathBuf, PathBuf)],
    work: &Path,
    readable: &[&Path],
) -> anyhow::Result<Untrusted> {
    let bwrap = crate::process::required("bwrap")?;
    let mut command = Command::new(bwrap);
    command.args(["--unshare-all", "--die-with-parent", "--new-session"]);
    for system in SYSTEM {
        if Path::new(system).exists() {
            command.args(["--ro-bind", system, system]);
        }
    }
    let inside = inside_name(input);
    command.args([
        "--proc", "/proc", "--dev", "/dev", "--tmpfs", "/tmp", "--dir", "/input",
    ]);
    // After the private /tmp, which would hide a program kept there.
    let outside =
        |path: &Path| path.is_absolute() && !SYSTEM.iter().any(|system| path.starts_with(system));
    if outside(program) {
        command.arg("--ro-bind").arg(program).arg(program);
    }
    for path in readable {
        if outside(path) && path.exists() {
            command.arg("--ro-bind").arg(path).arg(path);
        }
    }
    command.arg("--ro-bind").arg(input).arg(&inside);
    for (path, relative) in beside {
        if path.exists() {
            command
                .arg("--ro-bind")
                .arg(path)
                .arg(Path::new("/input").join(relative));
        }
    }
    command
        .arg("--bind")
        .arg(work)
        .arg("/work")
        .args([
            "--chdir",
            "/work",
            "--setenv",
            "HOME",
            "/work/home",
            "--setenv",
            "XDG_CACHE_HOME",
            "/work/cache",
            // GIO reads files itself instead of asking gvfs over the
            // session bus, which is not there.
            "--setenv",
            "GIO_USE_VFS",
            "local",
            "--unsetenv",
            "DISPLAY",
            "--unsetenv",
            "WAYLAND_DISPLAY",
            "--unsetenv",
            "DBUS_SESSION_BUS_ADDRESS",
            "--",
        ])
        .arg(program);
    Ok(Untrusted {
        command,
        input: inside.into(),
        work: "/work".into(),
        isolated: true,
    })
}

/// What a program run by `sandbox-exec` may do (Seatbelt's profile
/// language): nothing but what it says. It runs, and what it runs is held
/// the same; it reads the system's programs, libraries, frameworks and
/// fonts, what a package manager installed, itself (its `.app` with it),
/// the input and its folder; it writes only in its folder; it may learn
/// where things are, never what they hold; it asks the system for fonts,
/// the user's name and the preferences' defaults, as programs do to
/// start. No network: nothing allows it.
#[cfg(target_os = "macos")]
const PROFILE: &str = r#"(version 1)
(deny default)
(allow process-exec process-fork)
(allow signal (target same-sandbox))
(allow process-info* (target same-sandbox))
(allow sysctl-read)
(allow ipc-posix-sem ipc-posix-shm)
(allow file-read*
    (subpath "/System")
    (subpath "/usr")
    (subpath "/bin")
    (subpath "/sbin")
    (subpath "/Library/Fonts")
    (subpath "/Library/Frameworks")
    (subpath "/private/etc")
    (subpath "/private/var/db/timezone")
    (subpath "/opt/homebrew")
    (subpath "/opt/local")
    (subpath "/nix/store")
    (literal (param "PROGRAM"))
    (subpath (param "BUNDLE"))
    (literal (param "INPUT"))
    (subpath (param "WORK")))
(allow file-read-metadata)
(allow file-write* (subpath (param "WORK")))
(allow file-read* file-write-data
    (literal "/dev/null")
    (literal "/dev/zero")
    (literal "/dev/random")
    (literal "/dev/urandom")
    (literal "/dev/dtracehelper"))
(allow mach-lookup
    (global-name "com.apple.fonts")
    (global-name "com.apple.FontObjectsServer")
    (global-name "com.apple.system.opendirectoryd.libinfo")
    (global-name "com.apple.system.logger")
    (global-name "com.apple.cfprefsd.daemon")
    (global-name "com.apple.cfprefsd.agent"))
(allow user-preference-read)
"#;

/// `sandbox-exec` with `PROFILE`: paths are the real ones, which the
/// profile names (Seatbelt cannot move a file elsewhere), so the input is
/// reached through a link in the working folder under a plain name
/// (`inside_name`), and everything is named by its canonical path
/// (`/private/var`, not `/var`), which the profile compares.
#[cfg(target_os = "macos")]
fn isolated(
    program: &Path,
    input: &Path,
    beside: &[(PathBuf, PathBuf)],
    work: &Path,
    readable: &[&Path],
) -> anyhow::Result<Untrusted> {
    use std::ffi::OsString;
    let work = std::fs::canonicalize(work)?;
    let input = std::fs::canonicalize(input)?;
    let program = std::fs::canonicalize(program)?;
    // An application's bundle, read with it: its libraries and resources.
    let bundle = program
        .ancestors()
        .find(|dir| dir.extension().is_some_and(|e| e == "app"))
        .unwrap_or(&program)
        .to_owned();
    let inside = work.join(inside_name(&input).trim_start_matches('/'));
    std::fs::create_dir_all(work.join("input"))?;
    // A folder run in before keeps the link of that run.
    let _ = std::fs::remove_file(&inside);
    std::os::unix::fs::symlink(&input, &inside)?;
    // What the input names beside it, linked where it looks, and read.
    let mut also: Vec<PathBuf> = Vec::new();
    for (path, relative) in beside {
        let Ok(path) = std::fs::canonicalize(path) else {
            continue;
        };
        let link = work.join("input").join(relative);
        if let Some(parent) = link.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let _ = std::fs::remove_file(&link);
        std::os::unix::fs::symlink(&path, &link)?;
        also.push(path);
    }
    for folder in ["home", "cache", "tmp"] {
        std::fs::create_dir_all(work.join(folder))?;
    }
    // What it also reads, each a parameter of its own: a path is never
    // written into the profile's text.
    let readable: Vec<PathBuf> = readable
        .iter()
        .filter_map(|path| std::fs::canonicalize(path).ok())
        .chain(also)
        .collect();
    let mut profile = PROFILE.to_owned();
    profile.extend(
        (0..readable.len())
            .map(|n| format!("(allow file-read* (subpath (param \"READABLE{n}\")))\n")),
    );
    let mut command = Command::new("/usr/bin/sandbox-exec");
    command.arg("-p").arg(profile);
    let named = [
        ("WORK".to_owned(), &work),
        ("INPUT".to_owned(), &input),
        ("PROGRAM".to_owned(), &program),
        ("BUNDLE".to_owned(), &bundle),
    ];
    let also = readable
        .iter()
        .enumerate()
        .map(|(n, path)| (format!("READABLE{n}"), path));
    for (name, value) in named.into_iter().chain(also) {
        let mut parameter = OsString::from(format!("{name}="));
        parameter.push(value);
        command.arg("-D").arg(parameter);
    }
    command
        .arg(&program)
        .current_dir(&work)
        .env("HOME", work.join("home"))
        .env("XDG_CACHE_HOME", work.join("cache"))
        .env("TMPDIR", work.join("tmp"));
    Ok(Untrusted {
        command,
        input: inside,
        work,
        isolated: true,
    })
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn isolated(
    _program: &Path,
    _input: &Path,
    _beside: &[(PathBuf, PathBuf)],
    _work: &Path,
    _readable: &[&Path],
) -> anyhow::Result<Untrusted> {
    Err(needed())
}

/// A plugin from outside skwir that runs as a program (ADR-045), set to
/// run isolated for as long as it serves: the system's programs and
/// libraries readable, its own `folder` readable at its own path, its
/// working folder `work` writable (its home, its cache, its current
/// folder), nothing of the user's; no network unless `network`. It reads
/// files only through skwir, which hands it the bytes. An error where the
/// system cannot isolate: such a plugin never runs otherwise.
///
/// # Errors
/// No sandbox here (`available`), or the paths cannot be read.
pub fn plugin(
    program: &Path,
    folder: &Path,
    work: &Path,
    network: bool,
) -> anyhow::Result<Command> {
    if !available() {
        return Err(needed());
    }
    plugin_isolated(program, folder, work, network)
}

/// What a program that uses the network reads to reach it: names, routes
/// and the certificates that vouch for servers.
#[cfg(target_os = "linux")]
const NETWORK: &[&str] = &[
    "/etc/resolv.conf",
    "/etc/hosts",
    "/etc/nsswitch.conf",
    "/etc/ssl",
    "/etc/pki",
    "/etc/ca-certificates",
    "/etc/static/ssl",
];

#[cfg(target_os = "linux")]
fn plugin_isolated(
    program: &Path,
    folder: &Path,
    work: &Path,
    network: bool,
) -> anyhow::Result<Command> {
    let bwrap = crate::process::required("bwrap")?;
    let mut command = Command::new(bwrap);
    command.args(["--unshare-all", "--die-with-parent", "--new-session"]);
    if network {
        command.arg("--share-net");
    }
    let readable = SYSTEM
        .iter()
        .chain(if network { NETWORK } else { &[] })
        .map(Path::new);
    for path in readable {
        // A link under /etc (resolv.conf) is bound as what it leads to.
        if path.exists() {
            command.arg("--ro-bind").arg(path).arg(path);
        }
    }
    command.args(["--proc", "/proc", "--dev", "/dev", "--tmpfs", "/tmp"]);
    // After the private /tmp, which would hide a plugin kept there.
    command.arg("--ro-bind").arg(folder).arg(folder);
    // A program of the system's is there already: bound again, a link
    // there (NixOS's `/bin/sh`) is refused as a place to mount on.
    let system = SYSTEM.iter().any(|system| program.starts_with(system));
    if !program.starts_with(folder) && program.is_absolute() && !system {
        command.arg("--ro-bind").arg(program).arg(program);
    }
    command
        .arg("--bind")
        .arg(work)
        .arg("/work")
        .args([
            "--chdir",
            "/work",
            "--setenv",
            "HOME",
            "/work/home",
            "--setenv",
            "XDG_CACHE_HOME",
            "/work/cache",
            "--setenv",
            "TMPDIR",
            "/tmp",
            "--unsetenv",
            "DISPLAY",
            "--unsetenv",
            "WAYLAND_DISPLAY",
            "--unsetenv",
            "DBUS_SESSION_BUS_ADDRESS",
            "--",
        ])
        .arg(program);
    Ok(command)
}

#[cfg(target_os = "macos")]
fn plugin_isolated(
    program: &Path,
    folder: &Path,
    work: &Path,
    network: bool,
) -> anyhow::Result<Command> {
    use std::ffi::OsString;
    let work = std::fs::canonicalize(work)?;
    let folder = std::fs::canonicalize(folder)?;
    let program = std::fs::canonicalize(program)?;
    for inside in ["home", "cache", "tmp"] {
        std::fs::create_dir_all(work.join(inside))?;
    }
    // The converters' profile, the plugin's folder in place of an input,
    // and the network where it may use it.
    let mut profile = PROFILE.replace("(literal (param \"INPUT\"))", "(subpath (param \"INPUT\"))");
    if network {
        profile.push_str("(allow network*)\n");
    }
    let mut command = Command::new("/usr/bin/sandbox-exec");
    command.arg("-p").arg(profile);
    for (name, value) in [
        ("WORK", &work),
        ("INPUT", &folder),
        ("PROGRAM", &program),
        ("BUNDLE", &folder),
    ] {
        let mut parameter = OsString::from(format!("{name}="));
        parameter.push(value);
        command.arg("-D").arg(parameter);
    }
    command
        .arg(&program)
        .current_dir(&work)
        .env("HOME", work.join("home"))
        .env("XDG_CACHE_HOME", work.join("cache"))
        .env("TMPDIR", work.join("tmp"));
    Ok(command)
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn plugin_isolated(
    _program: &Path,
    _folder: &Path,
    _work: &Path,
    _network: bool,
) -> anyhow::Result<Command> {
    Err(
        skwir_core::system::not_yet("Running a plugin as a program, isolated from your files")
            .into(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_inside_name_keeps_only_a_plain_extension() {
        assert_eq!(inside_name(Path::new("/a/b.AVIF")), "/input/document.avif");
        assert_eq!(inside_name(Path::new("/a/b.c[0]")), "/input/document.c0");
        assert_eq!(inside_name(Path::new("/a/README")), "/input/document");
        assert_eq!(inside_name(Path::new("/a/x.%d:@")), "/input/document.d");
        assert_eq!(inside_name(Path::new("/a/x.%:@")), "/input/document");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_plugin_reads_its_folder_writes_its_work_and_sees_nothing_else() {
        if !available() {
            eprintln!("bubblewrap cannot isolate here");
            return;
        }
        let home = tempfile::tempdir().unwrap();
        let folder = home.path().join("plugin");
        std::fs::create_dir(&folder).unwrap();
        std::fs::write(folder.join("data"), "its own").unwrap();
        std::fs::write(home.path().join("secret"), "the user's").unwrap();
        let script = folder.join("run");
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\ncat {data} > /work/out\ncat {secret} >> /work/out 2>/dev/null || echo hidden >> /work/out\n",
                data = folder.join("data").display(),
                secret = home.path().join("secret").display(),
            ),
        )
        .unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let work = tempfile::tempdir().unwrap();
        let mut command = plugin(&script, &folder, work.path(), false).unwrap();
        crate::process::run(
            &mut command,
            &skwir_core::CancelToken::new(),
            std::time::Duration::from_secs(20),
        )
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(work.path().join("out")).unwrap(),
            "its ownhidden\n"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn an_isolated_program_sees_the_input_and_writes_only_its_folder() {
        if !available() {
            eprintln!("bubblewrap cannot isolate here");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("a photo’s [name].txt");
        std::fs::write(&input, "inside").unwrap();
        // A program kept outside the system's folders, as a test's is.
        let script = dir.path().join("copy");
        std::fs::write(
            &script,
            "#!/bin/sh\nread line < \"$1\"\necho \"$line\" > /work/out\necho escaped > \"$2\" 2>/dev/null\nexit 0\n",
        )
        .unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let work = tempfile::tempdir().unwrap();
        let mut run = command(&script, &input, work.path()).unwrap();
        assert_eq!(run.input, Path::new("/input/document.txt"));
        assert_eq!(run.work, Path::new("/work"));
        run.command.arg(&run.input).arg(dir.path().join("escaped"));
        crate::process::run(
            &mut run.command,
            &skwir_core::CancelToken::new(),
            std::time::Duration::from_secs(10),
        )
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(work.path().join("out")).unwrap(),
            "inside\n"
        );
        assert!(
            !dir.path().join("escaped").exists(),
            "nothing is written outside its folder"
        );
    }

    /// What the input names beside it (a glTF's buffer) is read where the
    /// program reads the input, at the same path from it.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_file_beside_the_input_is_read_beside_it() {
        if !available() {
            eprintln!("bubblewrap cannot isolate here");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("model.gltf");
        std::fs::write(&input, "{}").unwrap();
        std::fs::create_dir(dir.path().join("data")).unwrap();
        std::fs::write(dir.path().join("data/it's a box.bin"), "buffer").unwrap();
        let script = dir.path().join("read");
        std::fs::write(
            &script,
            "#!/bin/sh\ncat \"$(dirname \"$1\")/data/it's a box.bin\" > /work/out\n",
        )
        .unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let work = tempfile::tempdir().unwrap();
        let beside = [(
            dir.path().join("data/it's a box.bin"),
            PathBuf::from("data/it's a box.bin"),
        )];
        let mut run = untrusted_beside(
            &script,
            &input,
            &beside,
            work.path(),
            Otherwise::Refuse,
            &[],
        )
        .unwrap();
        run.command.arg(&run.input);
        crate::process::run(
            &mut run.command,
            &skwir_core::CancelToken::new(),
            std::time::Duration::from_secs(10),
        )
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(work.path().join("out")).unwrap(),
            "buffer"
        );
    }
}
