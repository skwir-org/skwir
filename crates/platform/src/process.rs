//! Bounded, cancellable tool execution. Call only from scheduler workers.
//! Each Unix child owns a process group so cancellation also stops descendants.

use std::io::{Read, Seek};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::Context;
use bibli_core::CancelToken;

pub fn find(program: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths).find_map(|dir| {
            let path = dir.join(program);
            let metadata = path.metadata().ok()?;
            if !metadata.is_file() {
                return None;
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if metadata.permissions().mode() & 0o111 == 0 {
                    return None;
                }
            }
            std::fs::canonicalize(path).ok()
        })
    })
}

pub fn required(program: &str) -> anyhow::Result<PathBuf> {
    find(program).with_context(|| format!("Install {program} to preview this file"))
}

struct ChildGuard(Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        #[cfg(unix)]
        if let Ok(raw) = i32::try_from(self.0.id())
            && let Some(pid) = rustix::process::Pid::from_raw(raw)
        {
            let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::KILL);
        }
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Capture small diagnostic/metadata output without pipes that can deadlock.
/// Files live in a private temporary directory and are capped while running.
/// Rendered artifacts must separately be checked by the caller before decoding.
pub fn run(
    command: &mut Command,
    cancel: &CancelToken,
    timeout: Duration,
) -> anyhow::Result<Vec<u8>> {
    const OUTPUT_LIMIT: u64 = 1024 * 1024;
    anyhow::ensure!(!cancel.is_cancelled(), "preview cancelled");
    let mut stdout = tempfile::tempfile()?;
    let mut stderr = tempfile::tempfile()?;
    command
        .stdin(Stdio::null())
        .stdout(stdout.try_clone()?)
        .stderr(stderr.try_clone()?);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let program = Path::new(command.get_program())
        .file_name()
        .unwrap_or(command.get_program())
        .to_string_lossy()
        .into_owned();
    let mut child = ChildGuard(
        command
            .spawn()
            .with_context(|| format!("start {program}"))?,
    );
    let start = Instant::now();
    let status = loop {
        anyhow::ensure!(!cancel.is_cancelled(), "preview cancelled");
        anyhow::ensure!(start.elapsed() < timeout, "{program}: preview timed out");
        anyhow::ensure!(
            stdout.metadata()?.len() <= OUTPUT_LIMIT && stderr.metadata()?.len() <= OUTPUT_LIMIT,
            "{program}: excessive output"
        );
        if let Some(status) = child.0.try_wait()? {
            break status;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    if !status.success() {
        stderr.rewind()?;
        let mut diagnostic = String::new();
        stderr.take(4096).read_to_string(&mut diagnostic).ok();
        anyhow::bail!("{program} failed ({status}): {}", diagnostic.trim());
    }
    stdout.rewind()?;
    let mut bytes = Vec::new();
    stdout.take(OUTPUT_LIMIT).read_to_end(&mut bytes)?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancellation_prevents_spawn_and_timeout_stops_a_child() {
        let tmp = tempfile::tempdir().unwrap();
        let marker = tmp.path().join("started");
        let cancelled = CancelToken::new();
        cancelled.cancel();
        let mut cmd = Command::new("sh");
        cmd.args(["-c", "touch \"$1\"", "test"]).arg(&marker);
        assert!(run(&mut cmd, &cancelled, Duration::from_secs(1)).is_err());
        assert!(!marker.exists());
        let start = Instant::now();
        let error = run(
            Command::new("sh").args(["-c", "sleep 30 & wait"]),
            &CancelToken::new(),
            Duration::from_millis(50),
        )
        .unwrap_err();
        assert!(error.to_string().contains("timed out"));
        assert!(start.elapsed() < Duration::from_secs(2));
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn running_cancellation_kills_descendants() {
        let tmp = tempfile::tempdir().unwrap();
        let marker = tmp.path().join("child-pid");
        let token = CancelToken::new();
        let cancel = token.clone();
        let output = marker.clone();
        let worker = std::thread::spawn(move || {
            run(
                Command::new("sh")
                    .args(["-c", "sleep 30 & printf '%s' \"$!\" > \"$1\"; wait", "test"])
                    .arg(output),
                &cancel,
                Duration::from_secs(5),
            )
        });
        let deadline = Instant::now() + Duration::from_secs(2);
        let pid = loop {
            if let Ok(contents) = std::fs::read_to_string(&marker)
                && let Ok(pid) = contents.parse::<u32>()
            {
                break pid;
            }
            assert!(Instant::now() < deadline, "child never started");
            std::thread::sleep(Duration::from_millis(5));
        };
        token.cancel();
        assert!(
            worker
                .join()
                .unwrap()
                .unwrap_err()
                .to_string()
                .contains("cancelled")
        );
        loop {
            let stat = std::fs::read_to_string(format!("/proc/{pid}/stat"));
            if stat.is_err() || stat.unwrap().split_whitespace().nth(2) == Some("Z") {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "descendant survived cancellation"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn captures_output_and_reports_failed_exit() {
        let token = CancelToken::new();
        assert_eq!(
            run(
                Command::new("sh").args(["-c", "printf hello"]),
                &token,
                Duration::from_secs(2)
            )
            .unwrap(),
            b"hello"
        );
        let error = run(
            Command::new("sh").args(["-c", "echo broken >&2; exit 3"]),
            &token,
            Duration::from_secs(2),
        )
        .unwrap_err();
        assert!(error.to_string().contains("broken"));
    }
}
