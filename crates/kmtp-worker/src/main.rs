//! A phone KDE's kmtpd holds, served as a folder through FUSE (ADR-097).
//! Over USB one program at a time uses a phone, and kmtpd (a module of
//! KDE's `kiod6`) keeps every one it sees; Skwir reaches such a phone
//! through kmtpd's own interface on the session bus, which this helper
//! mounts:
//!
//! ```text
//! skwir-kmtp-worker mount <udi> <mount point> <cache folder>
//! ```
//!
//! mounts the device kmtpd knows by its Solid `udi` at the mount point,
//! each of its storages a folder, and exits 0 once mounted, leaving a copy
//! of itself serving the mount until it is let go of, the device leaves
//! kmtpd or `kiod6` the bus. What it could not mount it says on standard
//! error, exiting 1. What is read or written is kept in unnamed files of
//! the cache folder, gone with their last close. Linux only.

#[cfg(target_os = "linux")]
mod fs;
#[cfg(target_os = "linux")]
mod kmtpd;

use std::ffi::OsString;

fn main() {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let code = match args.first().and_then(|a| a.to_str()) {
        // `--build`, skwir asking which build of it this helper is, when a
        // call of it failed (`platform::process::explain`).
        Some("--build") => {
            println!("{}", env!("SKWIR_BUILD"));
            0
        }
        // `--probe`, skwir's settings asking whether this helper works
        // here (`platform::process::probe`).
        Some("--probe") => probe(),
        Some("mount") => mount(&args[1..]),
        Some("serve") => serve(&args[1..]),
        _ => {
            eprintln!("usage: skwir-kmtp-worker mount <udi> <mount point> <cache folder>");
            2
        }
    };
    std::process::exit(code);
}

/// Whether it can work here: FUSE's device, and the system's program that
/// mounts it for a user (`fusermount3`).
#[cfg(target_os = "linux")]
fn probe() -> i32 {
    if !std::path::Path::new("/dev/fuse").exists() {
        eprintln!("this system has no FUSE (/dev/fuse)");
        return 1;
    }
    if fs::fusermount().is_none() {
        eprintln!("fusermount3 is not installed");
        return 1;
    }
    0
}

#[cfg(not(target_os = "linux"))]
fn probe() -> i32 {
    eprintln!("KDE's kmtpd is reached on Linux only");
    1
}

/// Start the copy of this program that serves the mount (`serve`), and
/// wait until it says it mounted: exit 0, leaving it running, or 1 with
/// what it said. The caller waits for this exit, then finds the mount in
/// the table (`mounter::mount`). Until mounted, `serve` is in this
/// helper's process group, which the caller stops whole when it gives up
/// (`process::run_leaving`: cancelled, or kmtpd not answering); mounted,
/// it leaves it, outliving what started it (ADR-077).
fn mount(args: &[OsString]) -> i32 {
    use std::io::BufRead;
    let Ok(program) = std::env::current_exe() else {
        eprintln!("this helper cannot find itself");
        return 1;
    };
    let mut command = std::process::Command::new(program);
    command
        .arg("serve")
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            eprintln!("this helper could not start: {error}");
            return 1;
        }
    };
    let mut said = String::new();
    if let Some(out) = child.stdout.take() {
        let _ = std::io::BufReader::new(out).read_line(&mut said);
    }
    match said.trim_end() {
        "mounted" => 0,
        said => {
            let _ = child.wait();
            let why = said.strip_prefix("failed: ").unwrap_or(said);
            eprintln!(
                "{}",
                if why.is_empty() {
                    "it ended before it mounted"
                } else {
                    why
                }
            );
            1
        }
    }
}

/// Mount and serve, saying `mounted` or `failed: <why>` on standard
/// output first (`mount` reads it), then nothing more.
#[cfg(target_os = "linux")]
fn serve(args: &[OsString]) -> i32 {
    use std::io::Write;
    let [udi, point, cache] = args else {
        println!("failed: it was not told what to mount, where and into what cache");
        return 2;
    };
    let udi = udi.to_string_lossy().into_owned();
    let session = match fs::Phone::mount(
        &udi,
        std::path::Path::new(point),
        std::path::Path::new(cache),
    ) {
        Ok(session) => session,
        Err(error) => {
            println!("failed: {error}");
            return 1;
        }
    };
    // Out of the helper's process group, and its session: no longer
    // stopped with what started it.
    let _ = rustix::process::setsid();
    println!("mounted");
    let _ = std::io::stdout().flush();
    match fs::serve(session, udi, std::path::PathBuf::from(point)) {
        Ok(()) => 0,
        Err(_) => 1,
    }
}

#[cfg(not(target_os = "linux"))]
fn serve(_args: &[OsString]) -> i32 {
    println!("failed: KDE's kmtpd is reached on Linux only");
    1
}
