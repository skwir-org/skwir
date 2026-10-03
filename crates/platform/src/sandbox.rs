//! Converters and thumbnailers run on a file nobody vouched for, isolated
//! from everything else (ADR-003, ADR-030, ADR-031). On Linux this is
//! bubblewrap: no network, only the system runtime and the input readable,
//! and only a disposable working folder writable. A sandbox that cannot
//! start is an error: a conversion is never retried without it.

use std::path::Path;
use std::process::Command;
use std::sync::OnceLock;

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
/// kernel that restricts them refuses. Asked once, on a worker.
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
        &bibli_core::CancelToken::new(),
        std::time::Duration::from_secs(5),
    );
    if let Err(error) = &isolated {
        tracing::debug!("bubblewrap cannot isolate here: {error:#}");
    }
    isolated.is_ok()
}

#[cfg(not(target_os = "linux"))]
fn isolates() -> bool {
    false
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

/// `program` set to run isolated: `input` readable at the path returned
/// (`inside_name`), and `work` writable at `/work`, also its home, cache
/// and current folder. A program outside the system's folders is bound
/// at its own path. The caller adds the program's own arguments and runs
/// it with `process::run`, whose cancellation kills the sandbox with it.
pub fn command(program: &Path, input: &Path, work: &Path) -> anyhow::Result<(Command, String)> {
    #[cfg(target_os = "linux")]
    {
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
        if program.is_absolute() && !SYSTEM.iter().any(|system| program.starts_with(system)) {
            command.arg("--ro-bind").arg(program).arg(program);
        }
        command
            .arg("--ro-bind")
            .arg(input)
            .arg(&inside)
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
        Ok((command, inside))
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (program, input, work);
        anyhow::bail!("isolated conversion is not implemented on this platform")
    }
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
        let (mut command, inside) = command(&script, &input, work.path()).unwrap();
        assert_eq!(inside, "/input/document.txt");
        command.arg(&inside).arg(dir.path().join("escaped"));
        crate::process::run(
            &mut command,
            &bibli_core::CancelToken::new(),
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
}
