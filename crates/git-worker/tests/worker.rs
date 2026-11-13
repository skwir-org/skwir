//! The helper against repositories the `git` program makes, its answers
//! compared with git's own (`rev-list`, `show`). Skipped where git is not
//! installed.
use serde_json::{Value, json};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// A repository made for a test, in a folder of its own, its commits
/// dated a minute apart from a fixed time unless told otherwise.
struct Repo {
    _dir: tempfile::TempDir,
    root: PathBuf,
    home: PathBuf,
    minute: std::cell::Cell<i64>,
}

/// When the first commit is made: 2026-11-03 09:05:01 +0100.
const START: i64 = 1_793_693_101;

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
            .prefix("git-worker-")
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
            minute: std::cell::Cell::new(0),
        };
        repo.git(&["init", "-q", "-b", "main"]);
        repo
    }

    /// Run git here, its output.
    fn git(&self, args: &[&str]) -> String {
        self.git_at(args, START + self.minute.get() * 60)
    }

    fn git_at(&self, args: &[&str], time: i64) -> String {
        let date = format!("{time} +0100");
        let output = Command::new("git")
            .args(["-c", "user.name=A Person", "-c", "user.email=a@example.org"])
            .args(["-c", "core.abbrev=7", "-c", "color.ui=false"])
            .args(args)
            .current_dir(&self.root)
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("HOME", &self.home)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("LC_ALL", "C")
            .env("TZ", "UTC")
            .env("GIT_AUTHOR_DATE", &date)
            .env("GIT_COMMITTER_DATE", &date)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    }

    fn write(&self, path: &str, content: &[u8]) {
        let path = self.root.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    /// Commit all that is in the folder, a minute after the last: its id.
    fn commit(&self, message: &str) -> String {
        self.minute.set(self.minute.get() + 1);
        self.git(&["add", "-A"]);
        self.git(&["commit", "-q", "--allow-empty", "-m", message]);
        self.head()
    }

    fn head(&self) -> String {
        self.git(&["rev-parse", "HEAD"]).trim().to_owned()
    }

    fn git_dir(&self) -> PathBuf {
        self.root.join(".git")
    }
}

/// The helper's answer to `request`: its status, standard output and error.
fn ask_raw(input: &[u8]) -> (bool, Vec<u8>, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_skwir-git-worker"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // A helper that refuses a long request stops reading it.
    let _ = child.stdin.take().unwrap().write_all(input);
    let output = child.wait_with_output().unwrap();
    (
        output.status.success(),
        output.stdout,
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

fn ask(request: &Value) -> Value {
    let (ok, stdout, stderr) = ask_raw(request.to_string().as_bytes());
    assert!(ok, "{request}: {stderr}");
    serde_json::from_slice(&stdout).unwrap()
}

fn log(repo: &Repo, path: &str, limit: usize) -> Value {
    ask(&json!({"ask": "log", "git_dir": repo.git_dir(), "path": path, "limit": limit}))
}

fn ids(answer: &Value) -> Vec<String> {
    answer["commits"]
        .as_array()
        .unwrap()
        .iter()
        .map(|commit| commit["id"].as_str().unwrap().to_owned())
        .collect()
}

/// `count` lines, each saying its number.
fn numbered(count: usize) -> String {
    (1..=count).fold(String::new(), |mut text, n| {
        use std::fmt::Write as _;
        let _ = writeln!(text, "line {n}");
        text
    })
}

fn lines(text: &str) -> Vec<String> {
    text.lines().map(str::to_owned).collect()
}

/// The patch the helper writes for `commit` under `path`.
fn patch(repo: &Repo, commit: &str, path: &str, bytes: u64) -> (Value, String) {
    let out = repo.root.parent().unwrap().join("out.patch");
    let answer = ask(&json!({
        "ask": "patch", "git_dir": repo.git_dir(), "commit": commit,
        "path": path, "out": out, "bytes": bytes,
    }));
    (answer, std::fs::read_to_string(&out).unwrap())
}

/// A history in a line: newest first, each commit's parent, summary,
/// author and date, and the branches and tags at it.
#[test]
fn a_linear_history_is_listed_newest_first_with_what_points_at_it() {
    if !has_git() {
        return;
    }
    let repo = Repo::new();
    repo.write("a.txt", b"one\n");
    let first = repo.commit("First\n\nWith a body");
    repo.git(&["branch", "feature"]);
    repo.write("a.txt", b"two\n");
    let second = repo.commit("  Second, said with spaces  ");
    repo.git(&["tag", "-a", "-m", "the first release", "v1"]);
    repo.write("a.txt", b"three\n");
    let third = repo.commit("Third");
    repo.git(&["update-ref", "refs/remotes/origin/main", &first]);
    let answer = log(&repo, "", 500);
    assert_eq!(answer["head"], "main");
    assert_eq!(ids(&answer), [third.clone(), second.clone(), first.clone()]);
    assert_eq!(
        lines(&repo.git(&["rev-list", "HEAD"])),
        [third.clone(), second.clone(), first.clone()]
    );
    let commits = answer["commits"].as_array().unwrap();
    assert_eq!(commits[0]["parents"], json!([second]));
    assert_eq!(commits[2]["parents"], json!([]));
    assert_eq!(commits[0]["summary"], "Third");
    assert_eq!(commits[1]["summary"], "Second, said with spaces");
    assert_eq!(commits[2]["summary"], "First");
    assert_eq!(commits[0]["author"], "A Person");
    assert_eq!(commits[0]["email"], "a@example.org");
    assert_eq!(commits[0]["time"], START + 180);
    assert_eq!(commits[0]["offset"], 3600);
    assert_eq!(commits[0]["refs"], json!(["main"]));
    assert_eq!(commits[1]["refs"], json!(["tag: v1"]));
    assert_eq!(
        commits[2]["refs"],
        json!(["feature", "remote: origin/main"])
    );
    // Shortened as git shortens it.
    assert_eq!(
        lines(&repo.git(&["rev-list", "--abbrev-commit", "HEAD"])),
        commits
            .iter()
            .map(|commit| commit["short"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>()
    );
    assert_eq!(answer["more"], false);
    assert_eq!(answer["stopped"], false);
}

/// A parent never comes before its children, whatever their dates say: a
/// side branch dated before its own parent is listed above it, as git's
/// `--date-order` lists it.
#[test]
fn a_parent_comes_after_all_its_children_as_git_s_date_order() {
    if !has_git() {
        return;
    }
    let repo = Repo::new();
    repo.write("a", b"a\n");
    repo.minute.set(10);
    let base = repo.commit("Base");
    repo.git(&["checkout", "-q", "-b", "side"]);
    repo.write("s", b"s\n");
    repo.minute.set(-10);
    let side = repo.commit("Side, its clock behind");
    repo.git(&["checkout", "-q", "main"]);
    repo.write("m", b"m\n");
    repo.minute.set(11);
    let main = repo.commit("Main");
    repo.minute.set(12);
    repo.git(&["merge", "-q", "--no-ff", "-m", "Merge side", "side"]);
    let merge = repo.head();
    let answer = log(&repo, "", 500);
    assert_eq!(
        ids(&answer),
        [merge.clone(), main.clone(), side.clone(), base.clone()]
    );
    assert_eq!(
        lines(&repo.git(&["rev-list", "--date-order", "HEAD"])),
        ids(&answer)
    );
    assert_eq!(answer["commits"][0]["parents"], json!([main, side]));
}

/// A folder's or a file's history is the commits that changed it, as
/// `git rev-list -- path` lists them, a side branch that changed it
/// included and the merge that brought it not.
#[test]
fn a_path_s_history_is_the_commits_that_changed_it() {
    if !has_git() {
        return;
    }
    let repo = Repo::new();
    repo.write("dir/a", b"a\n");
    repo.write("other/x", b"x\n");
    repo.commit("Both");
    repo.write("other/x", b"x2\n");
    repo.commit("Other");
    repo.git(&["checkout", "-q", "-b", "side"]);
    repo.write("dir/a", b"a on the side\n");
    repo.commit("Dir on the side");
    repo.git(&["checkout", "-q", "main"]);
    repo.write("other/x", b"x3\n");
    repo.commit("Other again");
    repo.minute.set(repo.minute.get() + 1);
    repo.git(&["merge", "-q", "--no-ff", "-m", "Merge side", "side"]);
    repo.write("dir/b", b"b\n");
    repo.commit("A file in dir");
    repo.write("dir/a", b"a at last\n");
    repo.commit("Dir/a at last");
    for path in ["dir", "dir/", "dir/a", "other", "other/x", "missing"] {
        let answer = log(&repo, path, 500);
        let listed = repo.git(&["rev-list", "HEAD", "--", path.trim_end_matches('/')]);
        assert_eq!(ids(&answer), lines(&listed), "{path}");
    }
    assert_eq!(
        log(&repo, "dir", 500)["commits"].as_array().unwrap().len(),
        4
    );
}

/// The limit counts what is listed; `more` says commits were left.
#[test]
fn the_limit_counts_what_is_listed_and_says_more_were_left() {
    if !has_git() {
        return;
    }
    let repo = Repo::new();
    for n in 0..3 {
        repo.write("f", format!("{n}\n").as_bytes());
        repo.commit(&format!("Commit {n}"));
    }
    let two = log(&repo, "", 2);
    assert_eq!(two["commits"].as_array().unwrap().len(), 2);
    assert_eq!(two["more"], true);
    let three = log(&repo, "", 3);
    assert_eq!(three["commits"].as_array().unwrap().len(), 3);
    assert_eq!(three["more"], false);
}

/// A repository with no commit yet has no history, on its branch.
#[test]
fn an_empty_repository_has_no_history() {
    if !has_git() {
        return;
    }
    let repo = Repo::new();
    let answer = log(&repo, "", 500);
    assert_eq!(answer["commits"], json!([]));
    assert_eq!(answer["head"], "main");
    assert_eq!(answer["more"], false);
}

/// A commit's patch is what `git show` prints: its header and message, a
/// file added, one changed, one deleted, a mode changed, a binary file, a
/// link; and the first commit's, against nothing.
#[test]
fn a_commit_s_patch_is_what_git_show_prints() {
    if !has_git() {
        return;
    }
    let repo = Repo::new();
    let text = numbered(30);
    repo.write("text.txt", text.as_bytes());
    repo.write("gone.txt", b"going\n");
    repo.write("script.sh", b"#!/bin/sh\necho hi\n");
    repo.write("fn.rs", b"fn main() {\n    let a = 1;\n    let b = 2;\n    let c = 3;\n    let d = 4;\n    let e = 5;\n}\n");
    let root = repo.commit("First\n\nA body, two lines\nlong.");
    let changed = text
        .replace("line 3\n", "line three\n")
        .replace("line 25\n", "");
    repo.write("text.txt", changed.as_bytes());
    std::fs::remove_file(repo.root.join("gone.txt")).unwrap();
    repo.write("new.txt", b"new\nno end");
    repo.write("empty", b"");
    repo.write("binary.bin", b"\x00\x01\x02 binary");
    repo.write("fn.rs", b"fn main() {\n    let a = 1;\n    let b = 2;\n    let c = 3;\n    let d = 4;\n    let e = 6;\n}\n");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let script = repo.root.join("script.sh");
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::os::unix::fs::symlink("text.txt", repo.root.join("link")).unwrap();
    }
    let second = repo.commit("Second");
    let shown = |commit: &str, path: &[&str]| {
        let mut args = vec![
            "show",
            "--no-renames",
            "--histogram",
            "--no-ext-diff",
            "--no-textconv",
            commit,
        ];
        if !path.is_empty() {
            args.push("--");
            args.extend_from_slice(path);
        }
        repo.git(&args)
    };
    let (answer, written) = patch(&repo, &second, "", 1 << 20);
    assert_eq!(written, shown(&second, &[]));
    assert_eq!(answer["cut"], false);
    assert_eq!(answer["bytes"], written.len());
    #[cfg(unix)]
    assert_eq!(answer["files"], 8);
    let (_, written) = patch(&repo, &root, "", 1 << 20);
    assert_eq!(written, shown(&root, &[]));
    let (answer, written) = patch(&repo, &second, "text.txt", 1 << 20);
    assert_eq!(written, shown(&second, &["text.txt"]));
    assert_eq!(answer["files"], 1);
}

/// A folder's part of a patch is the files under it.
#[test]
fn a_folder_s_patch_is_the_files_under_it() {
    if !has_git() {
        return;
    }
    let repo = Repo::new();
    repo.write("dir/sub/a", b"a\n");
    repo.write("dir/b", b"b\n");
    repo.write("elsewhere", b"e\n");
    repo.commit("First");
    repo.write("dir/sub/a", b"a2\n");
    repo.write("dir/c", b"c\n");
    repo.write("elsewhere", b"e2\n");
    std::fs::remove_file(repo.root.join("dir/b")).unwrap();
    let commit = repo.commit("Second");
    let (answer, written) = patch(&repo, &commit, "dir", 1 << 20);
    assert_eq!(
        written,
        repo.git(&["show", "--no-renames", "--histogram", &commit, "--", "dir"])
    );
    assert_eq!(answer["files"], 3);
    assert!(!written.contains("elsewhere"));
}

/// A merge's patch is against its first parent, its header naming both.
#[test]
fn a_merge_s_patch_is_against_its_first_parent() {
    if !has_git() {
        return;
    }
    let repo = Repo::new();
    repo.write("a", b"a\n");
    repo.commit("Base");
    repo.git(&["checkout", "-q", "-b", "side"]);
    repo.write("s", b"side\n");
    repo.commit("Side");
    repo.git(&["checkout", "-q", "main"]);
    repo.write("m", b"main\n");
    repo.commit("Main");
    repo.minute.set(repo.minute.get() + 1);
    repo.git(&["merge", "-q", "--no-ff", "-m", "Merge side", "side"]);
    let merge = repo.head();
    let (_, written) = patch(&repo, &merge, "", 1 << 20);
    let parents = repo.git(&["log", "-1", "--format=%p", &merge]);
    let header: Vec<String> = lines(&written).into_iter().take(2).collect();
    assert_eq!(
        header,
        [
            format!("commit {merge}"),
            format!("Merge: {}", parents.trim())
        ]
    );
    let diff = repo.git(&["diff", "--histogram", &format!("{merge}^1"), &merge]);
    assert!(written.ends_with(&diff), "{written}");
}

/// A patch past its bound stops at a line's end and says it goes on.
#[test]
fn a_patch_past_its_bound_stops_and_says_so() {
    if !has_git() {
        return;
    }
    let repo = Repo::new();
    let text = numbered(500);
    repo.write("long.txt", text.as_bytes());
    let commit = repo.commit("Long");
    let (answer, written) = patch(&repo, &commit, "", 400);
    assert_eq!(answer["cut"], true);
    let (kept, last) = written.trim_end().rsplit_once('\n').unwrap();
    assert_eq!(last, "[The patch goes on past 400 bytes]");
    assert!(kept.len() < 400);
    assert!(written.starts_with(&format!("commit {commit}\n")));
}

/// The helper says it runs, and which build it is.
#[test]
fn the_helper_answers_its_probe_and_its_build() {
    let probe = Command::new(env!("CARGO_BIN_EXE_skwir-git-worker"))
        .arg("--probe")
        .output()
        .unwrap();
    assert!(probe.status.success());
    assert!(probe.stdout.is_empty());
    let build = Command::new(env!("CARGO_BIN_EXE_skwir-git-worker"))
        .arg("--build")
        .output()
        .unwrap();
    assert!(build.status.success());
    assert!(!String::from_utf8_lossy(&build.stdout).trim().is_empty());
}

/// What is past its bounds, or not a repository, is refused with a reason.
#[test]
fn what_is_past_its_bounds_is_refused_with_a_reason() {
    let nowhere = Path::new("/nonexistent/skwir-git-worker/.git");
    let refused = |input: &[u8]| {
        let (ok, stdout, stderr) = ask_raw(input);
        assert!(!ok, "{}", String::from_utf8_lossy(input));
        assert!(stdout.is_empty());
        assert!(!stderr.trim().is_empty());
        assert_eq!(stderr.trim().lines().count(), 1, "{stderr}");
    };
    refused(&vec![b' '; 64 * 1024 + 1]);
    refused(b"not json");
    refused(
        json!({"ask": "log", "git_dir": nowhere, "limit": 200_001})
            .to_string()
            .as_bytes(),
    );
    refused(
        json!({"ask": "log", "git_dir": nowhere, "deadline_ms": 60_001})
            .to_string()
            .as_bytes(),
    );
    refused(
        json!({"ask": "log", "git_dir": nowhere})
            .to_string()
            .as_bytes(),
    );
    refused(
        json!({"ask": "patch", "git_dir": nowhere, "commit": "0", "out": "/dev/null", "bytes": (64u64 << 20) + 1})
            .to_string()
            .as_bytes(),
    );
    if has_git() {
        let repo = Repo::new();
        repo.write("a", b"a\n");
        repo.commit("One");
        let request = |commit: &str| {
            json!({"ask": "patch", "git_dir": repo.git_dir(), "commit": commit,
                   "out": repo.root.join("x.patch"), "bytes": 1024})
            .to_string()
        };
        refused(request("not hex").as_bytes());
        refused(request(&"0".repeat(40)).as_bytes());
    }
}
