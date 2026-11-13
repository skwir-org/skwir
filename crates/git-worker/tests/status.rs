//! The helper's status (ADR-134) against repositories the `git` program
//! makes, its answers compared with `git status`'s own. Skipped where git
//! is not installed.
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

struct Repo {
    _dir: tempfile::TempDir,
    root: PathBuf,
    home: PathBuf,
}

fn has_git() -> bool {
    let found = Command::new("git").arg("--version").output().is_ok();
    if !found {
        eprintln!("git is not installed: skipped");
    }
    found
}

impl Repo {
    fn new() -> Self {
        let base = PathBuf::from(std::env::var("HOME").unwrap()).join(".cache/skwir-tests");
        std::fs::create_dir_all(&base).unwrap();
        let dir = tempfile::Builder::new()
            .prefix("git-status-")
            .tempdir_in(&base)
            .unwrap();
        let root = dir.path().join("repo");
        let home = dir.path().join("home");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&home).unwrap();
        let repo = Self {
            _dir: dir,
            root,
            home,
        };
        repo.git(&["init", "-q", "-b", "main"]);
        repo
    }

    fn git(&self, args: &[&str]) -> String {
        let output = self.git_output(args);
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    }

    fn git_output(&self, args: &[&str]) -> std::process::Output {
        Command::new("git")
            .args(["-c", "user.name=A Person", "-c", "user.email=a@example.org"])
            .args(["-c", "color.ui=false", "-c", "core.fsmonitor=false"])
            .args(args)
            .current_dir(&self.root)
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", self.home.join(".config"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("LC_ALL", "C")
            .output()
            .unwrap()
    }

    fn write(&self, path: &str, content: &[u8]) {
        let path = self.root.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    fn commit(&self, message: &str) {
        self.git(&["add", "-A"]);
        self.git(&["commit", "-q", "--allow-empty", "-m", message]);
    }

    /// What `git status` says of the files under `path`, from it, as the
    /// helper says it: each path at its strongest, a folder without its
    /// `/`. Read without refreshing the index.
    fn git_status(&self, path: &str) -> BTreeMap<String, String> {
        let mut args = vec![
            "--no-optional-locks",
            "status",
            "--porcelain=v1",
            "-z",
            "--ignored=traditional",
            "-unormal",
        ];
        if !path.is_empty() {
            args.extend(["--", path]);
        }
        let out = self.git(&args);
        let mut found = BTreeMap::new();
        let mut records = out.split('\0').filter(|record| !record.is_empty());
        while let Some(record) = records.next() {
            let (xy, name) = record.split_at(3);
            let xy = &xy[..2];
            // A rename's source follows it.
            if xy.starts_with('R') || xy.starts_with('C') {
                records.next();
            }
            let state = match xy {
                "??" => "new",
                "!!" => "ignored",
                "DD" | "AU" | "UD" | "UA" | "DU" | "AA" | "UU" => "conflict",
                _ if &xy[1..] != " " => "changed",
                _ => "staged",
            };
            let name = name.trim_end_matches('/');
            let name = if path.is_empty() {
                name
            } else {
                name.strip_prefix(path)
                    .and_then(|rest| rest.strip_prefix('/'))
                    .unwrap_or(name)
            };
            // A path said twice (taken out of the index, and new) is at
            // its strongest.
            let rank = |state: &str| {
                ["conflict", "changed", "staged", "new", "ignored"]
                    .iter()
                    .position(|known| *known == state)
            };
            let known = found
                .entry(name.to_owned())
                .or_insert_with(|| state.to_owned());
            if rank(state) < rank(known) {
                state.clone_into(known);
            }
        }
        found
    }
}

fn ask(request: &Value) -> Value {
    let mut child = Command::new(env!("CARGO_BIN_EXE_skwir-git-worker"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(request.to_string().as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{request}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn status(repo: &Repo, path: &str, excludes: Option<&Path>) -> Value {
    ask(&json!({
        "ask": "status",
        "git_dir": repo.root.join(".git"),
        "work_tree": repo.root,
        "path": path,
        "excludes_file": excludes,
    }))
}

fn states(answer: &Value) -> BTreeMap<String, String> {
    answer["states"]
        .as_array()
        .unwrap()
        .iter()
        .map(|pair| {
            (
                pair[0].as_str().unwrap().to_owned(),
                pair[1].as_str().unwrap().to_owned(),
            )
        })
        .collect()
}

/// Every state a file can be in, said as git says it: changed, staged
/// (added, modified, taken out of the index), staged then changed again
/// (changed), new, a new folder said whole, ignored files and folders, a
/// folder holding only ignored files (ignored), an empty one (nothing),
/// a file gone from the disk (changed); the same of a folder below, its
/// paths named from it.
#[test]
fn a_folder_s_files_are_said_as_git_status_says_them() {
    if !has_git() {
        return;
    }
    let repo = Repo::new();
    repo.write(".gitignore", b"target/\n*.log\n");
    repo.write("a.txt", b"one\n");
    repo.write("dir/b.txt", b"two\n");
    repo.write("dir/c.txt", b"three\n");
    repo.write("dir/kept.txt", b"kept\n");
    repo.write("sub/deep/d.txt", b"four\n");
    repo.write("untouched/e.txt", b"five\n");
    repo.commit("first");
    repo.write("a.txt", b"one, changed\n");
    repo.write("dir/b.txt", b"two, staged\n");
    repo.git(&["add", "dir/b.txt"]);
    repo.write("dir/c.txt", b"three, staged\n");
    repo.git(&["add", "dir/c.txt"]);
    repo.write("dir/c.txt", b"three, staged, changed again\n");
    repo.write("dir/added.txt", b"added\n");
    repo.git(&["add", "dir/added.txt"]);
    repo.git(&["rm", "-q", "--cached", "dir/kept.txt"]);
    repo.write("new.txt", b"new\n");
    repo.write("fresh/inside/f.txt", b"new too\n");
    repo.write("target/debug/out", b"built\n");
    repo.write("dir/x.log", b"log\n");
    repo.write("logs/only.log", b"log\n");
    std::fs::create_dir_all(repo.root.join("empty")).unwrap();
    std::fs::remove_file(repo.root.join("sub/deep/d.txt")).unwrap();
    let whole = status(&repo, "", None);
    assert_eq!(whole["folder"], Value::Null);
    assert_eq!(whole["cut"], false);
    let said = states(&whole);
    assert_eq!(said, repo.git_status(""), "as git says it");
    assert_eq!(said["a.txt"], "changed");
    assert_eq!(said["dir/b.txt"], "staged");
    assert_eq!(said["dir/c.txt"], "changed", "changed again wins");
    assert_eq!(said["dir/added.txt"], "staged");
    assert_eq!(said["dir/kept.txt"], "staged", "out of the index, and new");
    assert_eq!(said["fresh"], "new", "a new folder whole");
    assert_eq!(said["target"], "ignored");
    assert_eq!(said["logs"], "ignored", "all it holds is ignored");
    assert_eq!(said["sub/deep/d.txt"], "changed", "gone from the disk");
    assert!(!said.contains_key("empty"), "an empty folder is nothing");
    assert!(!said.keys().any(|path| path.starts_with("untouched")));
    let below = states(&status(&repo, "dir", None));
    assert_eq!(below, repo.git_status("dir"), "a folder's, from it");
    assert_eq!(below["b.txt"], "staged");
    assert_eq!(below["x.log"], "ignored");
}

/// A file whose stat changed and whose bytes did not is as it was, its
/// bytes hashed as they are; one whose executable bit changed is changed.
#[test]
fn a_file_touched_is_as_it_was_and_one_made_executable_is_changed() {
    if !has_git() {
        return;
    }
    let repo = Repo::new();
    repo.write("same.txt", b"same\n");
    repo.write("run.sh", b"echo\n");
    repo.commit("first");
    let later = std::time::SystemTime::now() + std::time::Duration::from_secs(3600);
    std::fs::File::options()
        .write(true)
        .open(repo.root.join("same.txt"))
        .unwrap()
        .set_modified(later)
        .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            repo.root.join("run.sh"),
            std::fs::Permissions::from_mode(0o755),
        )
        .unwrap();
    }
    let said = states(&status(&repo, "", None));
    assert!(!said.contains_key("same.txt"), "{said:?}");
    #[cfg(unix)]
    assert_eq!(said["run.sh"], "changed");
    assert_eq!(said, repo.git_status(""));
}

/// A file both sides of a merge changed is in conflict; a folder asked
/// that the index tracks nothing in is new, or ignored, whole.
#[test]
fn a_merge_s_conflict_and_a_folder_new_or_ignored_whole() {
    if !has_git() {
        return;
    }
    let repo = Repo::new();
    repo.write(".gitignore", b"build/\n");
    repo.write("m.txt", b"base\n");
    repo.commit("base");
    repo.git(&["checkout", "-q", "-b", "side"]);
    repo.write("m.txt", b"side\n");
    repo.commit("side");
    repo.git(&["checkout", "-q", "main"]);
    repo.write("m.txt", b"main\n");
    repo.commit("main");
    assert!(!repo.git_output(&["merge", "-q", "side"]).status.success());
    repo.write("notes/n.txt", b"n\n");
    repo.write("build/out", b"o\n");
    let said = states(&status(&repo, "", None));
    assert_eq!(said["m.txt"], "conflict");
    assert_eq!(said, repo.git_status(""));
    let notes = status(&repo, "notes", None);
    assert_eq!(notes["folder"], "new");
    assert!(states(&notes).is_empty());
    assert_eq!(status(&repo, "build", None)["folder"], "ignored");
}

/// A repository with no commit yet: what is added is staged, the rest
/// new; the user's patterns ignored everywhere are read from the file
/// skwir names.
#[test]
fn a_repository_without_a_commit_and_the_user_s_ignored_patterns() {
    if !has_git() {
        return;
    }
    let repo = Repo::new();
    repo.write("added.txt", b"a\n");
    repo.git(&["add", "added.txt"]);
    repo.write("other.txt", b"o\n");
    repo.write("scratch.tmp", b"t\n");
    let said = states(&status(&repo, "", None));
    assert_eq!(said["added.txt"], "staged");
    assert_eq!(said["other.txt"], "new");
    assert_eq!(said["scratch.tmp"], "new", "no pattern ignores it");
    let ignore = repo.home.join(".config/git/ignore");
    std::fs::create_dir_all(ignore.parent().unwrap()).unwrap();
    std::fs::write(&ignore, "*.tmp\n").unwrap();
    let said = states(&status(&repo, "", Some(&ignore)));
    assert_eq!(said["scratch.tmp"], "ignored");
    assert_eq!(said, repo.git_status(""), "as git reads the same file");
}
