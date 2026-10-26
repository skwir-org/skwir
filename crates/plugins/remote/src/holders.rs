//! Which programs hold a device open (ADR-096): over USB, one program at a
//! time uses a phone or a camera, and GVfs cannot open one another holds
//! (KDE's `kiod6` keeps every phone it sees). Found in `/proc`, each of the
//! user's processes' open files; another user's are not readable, and are
//! not this user's to quit. Linux only.

use std::path::Path;

/// The programs of this user other than Skwir holding `node` open, by the
/// names they were started as (`kiod6`), each once. Reads every process's
/// open files: on a worker, once opening the device failed.
#[cfg(target_os = "linux")]
pub fn of(node: &Path) -> Vec<String> {
    let own = std::process::id();
    let mut names: Vec<String> = Vec::new();
    let Ok(processes) = std::fs::read_dir("/proc") else {
        return names;
    };
    for process in processes.flatten() {
        let Some(pid) = process
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<u32>().ok())
        else {
            continue;
        };
        if pid == own {
            continue;
        }
        let Ok(files) = std::fs::read_dir(process.path().join("fd")) else {
            continue;
        };
        let holds = files
            .flatten()
            .any(|file| std::fs::read_link(file.path()).is_ok_and(|target| target == node));
        if holds
            && let Some(name) = started_as(&process.path())
            && !names.contains(&name)
        {
            names.push(name);
        }
    }
    names
}

#[cfg(not(target_os = "linux"))]
pub fn of(_node: &Path) -> Vec<String> {
    Vec::new()
}

/// What the process at `process` (its folder of `/proc`) was started as:
/// its command's file name, not the name a wrapper leaves in `comm`
/// (`.kiod6-wrapped`).
#[cfg(target_os = "linux")]
fn started_as(process: &Path) -> Option<String> {
    let line = std::fs::read(process.join("cmdline")).ok()?;
    let command = line.split(|b| *b == 0).next().filter(|c| !c.is_empty())?;
    let command = skwir_core::os_path::from_bytes_ref(command);
    let name = Path::new(&command).file_name()?.to_string_lossy();
    (!name.is_empty()).then(|| name.into_owned())
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    /// A program holding a file open is named by what it was started as.
    #[test]
    fn a_program_holding_a_file_is_named() {
        let dir = tempfile::tempdir().unwrap();
        let node = dir.path().join("it's held");
        std::fs::write(&node, b"").unwrap();
        assert!(of(&node).is_empty(), "nothing holds it yet");
        let mut held = std::process::Command::new("sleep")
            .arg("30")
            .stdin(std::fs::File::open(&node).unwrap())
            .spawn()
            .unwrap();
        // Its files are listed once it runs: waited for, never slept on.
        let since = std::time::Instant::now();
        let mut names = of(&node);
        while names.is_empty() && since.elapsed() < std::time::Duration::from_secs(10) {
            std::thread::yield_now();
            names = of(&node);
        }
        let _ = held.kill();
        let _ = held.wait();
        assert_eq!(names, ["sleep"]);
        assert!(of(&dir.path().join("another")).is_empty());
    }
}
