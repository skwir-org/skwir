//! A commit's patch, as `git show` prints it: its header and message, then
//! a unified diff of each file it changed against its first parent.
use crate::rename::{self, Source};
use crate::tree::{self, Change, Entry};
use crate::{Deadline, Result};
use gix::ObjectId;
use gix::bstr::{BString, ByteSlice};
use gix::objs::tree::EntryKind;
use imara_diff::{Algorithm, Diff, Hunk, InternedInput};
use serde::Serialize;

/// Lines of context around a change, and the gap two changes are shown
/// apart past (git's: their contexts would not meet).
const CONTEXT: u32 = 3;
/// A file larger is said to differ, as binary.
const LARGEST: u64 = 8 << 20;
/// Where a NUL says a file is binary, as git looks.
const FIRST_FEW: usize = 8000;
/// A hunk's function line kept at most, as git's buffer keeps it.
const FUNCTION: usize = 80;

#[derive(Serialize)]
pub(crate) struct Wrote {
    files: usize,
    bytes: usize,
    cut: bool,
}

/// The patch, whole lines only, `bound` bytes at most.
struct Patch {
    bytes: Vec<u8>,
    bound: usize,
    cut: bool,
}

impl Patch {
    fn line(&mut self, parts: &[&[u8]]) {
        if self.cut {
            return;
        }
        let length: usize = parts.iter().map(|part| part.len()).sum::<usize>() + 1;
        if self.bytes.len() + length > self.bound {
            self.cut = true;
            return;
        }
        for part in parts {
            self.bytes.extend_from_slice(part);
        }
        self.bytes.push(b'\n');
    }
}

/// The patch of commit `id`, the files under `path` alone (all of them for
/// none), `bound` bytes at most: what is written, and what was. A file
/// `path` names that the commit has and its first parent lacks is shown
/// renamed from the file it came from, if any, as `git show -M` shows it
/// (`rename::source`).
pub(crate) fn patch(
    repo: &gix::Repository,
    id: ObjectId,
    path: &[&[u8]],
    bound: usize,
    deadline: &Deadline,
) -> Result<(Wrote, Vec<u8>)> {
    let commit = repo.find_commit(id)?;
    let parents: Vec<ObjectId> = commit.parent_ids().map(gix::Id::detach).collect();
    let mut out = Patch {
        bytes: Vec::new(),
        bound,
        cut: false,
    };
    out.line(&[b"commit ", id.to_string().as_bytes()]);
    if parents.len() > 1 {
        let short: Vec<String> = parents.iter().map(|parent| abbreviated(*parent)).collect();
        out.line(&[b"Merge: ", short.join(" ").as_bytes()]);
    }
    let author = commit.author()?;
    out.line(&[b"Author: ", author.name, b" <", author.email, b">"]);
    let date = author
        .time()?
        .format(gix::date::time::format::DEFAULT)
        .map_err(|error| format!("writing the date: {error}"))?;
    out.line(&[b"Date:   ", date.as_bytes()]);
    out.line(&[]);
    let message = commit.message_raw()?;
    let mut lines: Vec<&[u8]> = message.lines().collect();
    while lines.last().is_some_and(|line| line.trim().is_empty()) {
        lines.pop();
    }
    for line in lines {
        out.line(&[b"    ", line]);
    }
    out.line(&[]);
    // Against the first parent; one past a shallow clone's boundary is none,
    // as git takes it, and the commit shows all it holds.
    let first = parents
        .first()
        .copied()
        .filter(|parent| repo.has_object(parent));
    let new_root = commit.tree_id()?.detach();
    let old_root = match first {
        Some(parent) => Some(repo.find_commit(parent)?.tree_id()?.detach()),
        None => None,
    };
    let old = match old_root {
        Some(root) => tree::entry_at(repo, root, path)?,
        None => None,
    };
    let new = tree::entry_at(repo, new_root, path)?;
    let mut changes = Vec::new();
    let prefix: BString = path.join(&b'/').into();
    let renamed = match (old_root, old, new) {
        (Some(old_root), None, Some(new)) if !path.is_empty() && new.is_file() => {
            rename::source(repo, old_root, new_root, &prefix, new, deadline)?
        }
        _ => None,
    };
    match &renamed {
        Some(source) => changes.push(Change {
            path: prefix,
            old: Some(source.entry),
            new,
        }),
        None => tree::changed(repo, &prefix, old, new, &mut changes)?,
    }
    let mut late = false;
    for change in &changes {
        if out.cut {
            break;
        }
        if deadline.passed() {
            late = true;
            break;
        }
        file(repo, change, renamed.as_ref(), &mut out)?;
    }
    let cut = out.cut || late;
    if out.cut {
        out.bytes
            .extend_from_slice(format!("[The patch goes on past {bound} bytes]\n").as_bytes());
    } else if late {
        out.bytes
            .extend_from_slice(b"[The patch goes on: it was not read whole in time]\n");
    }
    Ok((
        Wrote {
            files: changes.len(),
            bytes: out.bytes.len(),
            cut,
        },
        out.bytes,
    ))
}

/// `id`'s first seven hexadecimal digits, as git abbreviates in a patch.
fn abbreviated(id: ObjectId) -> String {
    id.to_hex_with_len(7).to_string()
}

/// What an entry holds, as a patch shows it: a file's bytes, a link's
/// target, a submodule's commit; `None` for one said to differ as binary.
fn content(repo: &gix::Repository, entry: Entry) -> Result<Option<Vec<u8>>> {
    if entry.mode.kind() == EntryKind::Commit {
        return Ok(Some(
            format!("Subproject commit {}\n", entry.id).into_bytes(),
        ));
    }
    if repo.find_header(entry.id)?.size() > LARGEST {
        return Ok(None);
    }
    let blob = repo.find_blob(entry.id)?;
    let data = blob.data.clone();
    if data[..data.len().min(FIRST_FEW)].contains(&0) {
        return Ok(None);
    }
    Ok(Some(data))
}

/// The mode as a patch writes it.
fn mode(entry: Entry) -> &'static [u8] {
    entry.mode.kind().as_octal_str().as_bytes()
}

/// One file's part of the patch; `renamed` says where it came from when
/// it was renamed, as git writes a rename: its similarity, its two names,
/// then what changed in it, if anything did.
fn file(
    repo: &gix::Repository,
    change: &Change,
    renamed: Option<&Source>,
    out: &mut Patch,
) -> Result<()> {
    let path = change.path.as_slice();
    let was = renamed.map_or(path, |source| source.path.as_slice());
    out.line(&[b"diff --git a/", was, b" b/", path]);
    let zero = b"0000000".as_slice();
    let old_id = change.old.map(|entry| abbreviated(entry.id));
    let new_id = change.new.map(|entry| abbreviated(entry.id));
    let index = |out: &mut Patch, mode: Option<&[u8]>| {
        let old = old_id.as_deref().map_or(zero, str::as_bytes);
        let new = new_id.as_deref().map_or(zero, str::as_bytes);
        match mode {
            Some(mode) => out.line(&[b"index ", old, b"..", new, b" ", mode]),
            None => out.line(&[b"index ", old, b"..", new]),
        }
    };
    match (change.old, change.new) {
        (None, Some(new)) => {
            out.line(&[b"new file mode ", mode(new)]);
            index(out, None);
        }
        (Some(old), None) => {
            out.line(&[b"deleted file mode ", mode(old)]);
            index(out, None);
        }
        (Some(old), Some(new)) => {
            let same_mode = mode(old) == mode(new);
            if !same_mode {
                out.line(&[b"old mode ", mode(old)]);
                out.line(&[b"new mode ", mode(new)]);
            }
            if let Some(source) = renamed {
                out.line(&[
                    b"similarity index ",
                    source.percent().to_string().as_bytes(),
                    b"%",
                ]);
                out.line(&[b"rename from ", was]);
                out.line(&[b"rename to ", path]);
            }
            if old.id == new.id {
                return Ok(());
            }
            index(out, same_mode.then(|| mode(new)));
        }
        (None, None) => return Ok(()),
    }
    let before = match change.old {
        Some(entry) => content(repo, entry)?,
        None => Some(Vec::new()),
    };
    let after = match change.new {
        Some(entry) => content(repo, entry)?,
        None => Some(Vec::new()),
    };
    let old_name: Vec<u8> = match change.old {
        Some(_) => [b"a/".as_slice(), was].concat(),
        None => b"/dev/null".to_vec(),
    };
    let new_name: Vec<u8> = match change.new {
        Some(_) => [b"b/".as_slice(), path].concat(),
        None => b"/dev/null".to_vec(),
    };
    let (Some(before), Some(after)) = (before, after) else {
        out.line(&[b"Binary files ", &old_name, b" and ", &new_name, b" differ"]);
        return Ok(());
    };
    let hunks = unified(&before, &after);
    if hunks.is_empty() {
        return Ok(());
    }
    out.line(&[b"--- ", &old_name]);
    out.line(&[b"+++ ", &new_name]);
    for line in hunks {
        out.line(&[&line]);
        if out.cut {
            break;
        }
    }
    Ok(())
}

/// A line's bytes without its end, and whether it had one.
fn ended(line: &[u8]) -> (&[u8], bool) {
    match line.strip_suffix(b"\n") {
        Some(text) => (text, true),
        None => (line, false),
    }
}

/// The function line git heads a hunk with: the nearest line above it
/// (from `from` up) that starts with a letter, `_` or `$`, its trailing
/// spaces cut, 80 bytes at most.
fn function(lines: &[&[u8]], from: usize) -> Option<Vec<u8>> {
    lines[..from].iter().rev().find_map(|line| {
        let first = *line.first()?;
        if !(first.is_ascii_alphabetic() || first == b'_' || first == b'$') {
            return None;
        }
        let mut text = &line[..line.len().min(FUNCTION)];
        while let Some((last, rest)) = text.split_last() {
            if !last.is_ascii_whitespace() {
                break;
            }
            text = rest;
        }
        Some(text.to_vec())
    })
}

/// A range in a hunk's header: its first line (the one before it when it
/// is empty), and its length when not one.
fn range(start: u32, length: u32) -> String {
    let first = if length == 0 { start } else { start + 1 };
    if length == 1 {
        format!("{first}")
    } else {
        format!("{first},{length}")
    }
}

/// The unified diff of `before` and `after`, line by line, `CONTEXT` lines
/// around each change, changes whose contexts meet in one hunk: each line
/// of it without its end.
pub(crate) fn unified(before: &[u8], after: &[u8]) -> Vec<Vec<u8>> {
    let input = InternedInput::new(before, after);
    let mut diff = Diff::compute(Algorithm::Histogram, &input);
    diff.postprocess_lines(&input);
    let hunks: Vec<Hunk> = diff.hunks().collect();
    let old: Vec<&[u8]> = input
        .before
        .iter()
        .map(|token| input.interner[*token])
        .collect();
    let new: Vec<&[u8]> = input
        .after
        .iter()
        .map(|token| input.interner[*token])
        .collect();
    let old_len = old.len() as u32;
    let mut out = Vec::new();
    let mut at = 0;
    while at < hunks.len() {
        // The changes shown together: the next one's context meets this one's.
        let mut last = at;
        while last + 1 < hunks.len()
            && hunks[last + 1].before.start - hunks[last].before.end <= 2 * CONTEXT
        {
            last += 1;
        }
        let (first, end) = (&hunks[at], &hunks[last]);
        let lead = CONTEXT.min(first.before.start);
        let (old_start, new_start) = (first.before.start - lead, first.after.start - lead);
        // The lines past the last change are alike on both sides up to the
        // next change, farther than the context, or to both files' ends.
        let trail = CONTEXT.min(old_len - end.before.end);
        let (old_end, new_end) = (end.before.end + trail, end.after.end + trail);
        let mut header = format!(
            "@@ -{} +{} @@",
            range(old_start, old_end - old_start),
            range(new_start, new_end - new_start)
        )
        .into_bytes();
        if let Some(function) = function(&old, old_start as usize) {
            header.push(b' ');
            header.extend_from_slice(&function);
        }
        out.push(header);
        let mut push = |sign: u8, line: &[u8]| {
            let (text, has_end) = ended(line);
            let mut written = Vec::with_capacity(text.len() + 1);
            written.push(sign);
            written.extend_from_slice(text);
            out.push(written);
            if !has_end {
                out.push(b"\\ No newline at end of file".to_vec());
            }
        };
        let mut position = old_start;
        for hunk in &hunks[at..=last] {
            for line in &old[position as usize..hunk.before.start as usize] {
                push(b' ', line);
            }
            for line in &old[hunk.before.start as usize..hunk.before.end as usize] {
                push(b'-', line);
            }
            for line in &new[hunk.after.start as usize..hunk.after.end as usize] {
                push(b'+', line);
            }
            position = hunk.before.end;
        }
        for line in &old[position as usize..old_end as usize] {
            push(b' ', line);
        }
        at = last + 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{range, unified};

    fn said(before: &str, after: &str) -> Vec<String> {
        unified(before.as_bytes(), after.as_bytes())
            .into_iter()
            .map(|line| String::from_utf8(line).unwrap())
            .collect()
    }

    /// Ranges as git writes them: a single line by its number, an empty one
    /// by the line before it.
    #[test]
    fn a_hunk_s_ranges_are_written_as_git_writes_them() {
        assert_eq!(range(0, 0), "0,0");
        assert_eq!(range(4, 1), "5");
        assert_eq!(range(4, 3), "5,3");
    }

    /// Changes whose contexts meet are one hunk; farther apart, two, each
    /// headed by the function line above it.
    #[test]
    fn changes_near_share_a_hunk_and_far_ones_have_their_own() {
        let before = (1..=20).fold(String::new(), |mut text, n| {
            use std::fmt::Write;
            let _ = writeln!(text, "line {n}");
            text
        });
        let near = before
            .replace("line 5\n", "five\n")
            .replace("line 11\n", "eleven\n");
        assert_eq!(
            said(&before, &near)
                .iter()
                .filter(|l| l.starts_with("@@"))
                .count(),
            1
        );
        let far = before
            .replace("line 2\n", "two\n")
            .replace("line 19\n", "nineteen\n");
        let hunks: Vec<String> = said(&before, &far)
            .into_iter()
            .filter(|l| l.starts_with("@@"))
            .collect();
        assert_eq!(hunks, ["@@ -1,5 +1,5 @@", "@@ -16,5 +16,5 @@ line 15"]);
    }

    /// A last line without its end says so, as git does.
    #[test]
    fn a_last_line_without_an_end_says_so() {
        assert_eq!(
            said("a\nb", "a\nc"),
            [
                "@@ -1,2 +1,2 @@",
                " a",
                "-b",
                "\\ No newline at end of file",
                "+c",
                "\\ No newline at end of file"
            ]
        );
        assert_eq!(said("", "x\n"), ["@@ -0,0 +1 @@", "+x"]);
    }
}
