//! Converters run on a file nobody vouched for, isolated from everything
//! else (ADR-003, ADR-030). On Linux this is bubblewrap: no network, only
//! the system runtime and the input readable, and only a disposable
//! working folder writable. A sandbox that cannot start is an error: a
//! conversion is never retried without it.

use std::path::Path;
use std::process::Command;

/// Whether a converter can run isolated here: on Linux, whether
/// bubblewrap is installed.
pub fn available() -> bool {
    cfg!(target_os = "linux") && crate::process::required("bwrap").is_ok()
}

/// `program` set to run isolated: `input` readable at the path returned
/// (named `document.<its extension>`, whatever the file is called), and
/// `work` writable at `/work`, also its home, cache and current folder.
/// The caller adds the program's own arguments and runs it with
/// `process::run`, whose cancellation kills the sandbox with it.
pub fn command(program: &Path, input: &Path, work: &Path) -> anyhow::Result<(Command, String)> {
    #[cfg(target_os = "linux")]
    {
        let bwrap = crate::process::required("bwrap")?;
        let mut command = Command::new(bwrap);
        command.args(["--unshare-all", "--die-with-parent", "--new-session"]);
        for system in [
            "/nix/store",
            "/usr",
            "/bin",
            "/lib",
            "/lib64",
            "/etc/fonts",
            "/etc/ld.so.cache",
            "/etc/localtime",
        ] {
            if Path::new(system).exists() {
                command.args(["--ro-bind", system, system]);
            }
        }
        let inside = format!("/input/document.{}", crate::inspection::extension(input));
        command
            .args([
                "--proc",
                "/proc",
                "--dev",
                "/dev",
                "--tmpfs",
                "/tmp",
                "--dir",
                "/input",
                "--ro-bind",
            ])
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
