//! The commits that reached `HEAD`, as `git log --date-order` lists them,
//! those that changed a path alone when one is asked.
use crate::tree::Lookups;
use crate::{Deadline, Result};
use gix::ObjectId;
use gix::bstr::ByteSlice;
use gix::refs::Category;
use serde::Serialize;
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};

/// A summary kept at most, in characters.
const SUMMARY: usize = 300;
/// An author's name or address kept at most, in characters.
const NAME: usize = 200;

#[derive(Serialize)]
pub(crate) struct Log {
    /// The branch `HEAD` is on; `None` when it is detached.
    head: Option<String>,
    commits: Vec<Commit>,
    more: bool,
    stopped: bool,
}

#[derive(Serialize)]
struct Commit {
    id: String,
    /// Its id shortened as git shortens it (`--abbrev-commit`): to the
    /// length `core.abbrev` says, or the repository's size gives, longer
    /// where that would name two objects.
    short: String,
    parents: Vec<String>,
    author: String,
    email: String,
    /// When it was written, in seconds since 1970 (UTC), and the offset of
    /// its writer's zone, in seconds east of UTC.
    time: i64,
    offset: i32,
    summary: String,
    refs: Vec<String>,
}

/// A commit as the walk read it.
struct Node {
    id: ObjectId,
    time: i64,
    /// Its parents among those read, by place in `nodes`.
    parents: Vec<u32>,
}

/// The log of `repo` from `HEAD`: `limit` commits at most, those that
/// changed `path` alone when it is not empty.
///
/// The order is git's `--date-order`, which needs every commit's children
/// known before it is placed: the history is read whole first, by gix's
/// walk newest first (which a shallow clone's boundary ends), then placed
/// by Kahn's algorithm, a commit becoming ready once all its children are
/// placed, the newest ready placed first (a binary heap): n log n in the
/// commits, each read once. Read past the deadline, what was read is placed
/// and answered.
pub(crate) fn log(
    repo: &gix::Repository,
    path: &[&[u8]],
    limit: usize,
    deadline: &Deadline,
) -> Result<Log> {
    let head = repo.head()?;
    let branch = head
        .referent_name()
        .map(|name| name.shorten().to_str_lossy().into_owned());
    let Some(tip) = head.id().map(gix::Id::detach) else {
        return Ok(Log {
            head: branch,
            commits: Vec::new(),
            more: false,
            stopped: false,
        });
    };
    let mut stopped = false;
    // Read the history whole.
    let mut nodes: Vec<Node> = Vec::new();
    let mut parents_read: Vec<Vec<ObjectId>> = Vec::new();
    let mut place: HashMap<ObjectId, u32> = HashMap::new();
    let walk = repo
        .rev_walk([tip])
        .sorting(gix::revision::walk::Sorting::ByCommitTime(
            gix::traverse::commit::simple::CommitTimeOrder::NewestFirst,
        ))
        .all()?;
    for (read, info) in walk.enumerate() {
        if read % 1024 == 0 && deadline.passed() {
            stopped = true;
            break;
        }
        let info = info?;
        let at = u32::try_from(nodes.len()).map_err(|_| "more commits than can be counted")?;
        place.insert(info.id, at);
        parents_read.push(info.parent_ids.iter().copied().collect());
        nodes.push(Node {
            id: info.id,
            time: info.commit_time.unwrap_or_default(),
            parents: Vec::new(),
        });
    }
    // Each parent known by its place: one not read (past a shallow
    // clone's boundary, or the deadline) is none.
    let mut children = vec![0u32; nodes.len()];
    for (node, parents) in nodes.iter_mut().zip(parents_read) {
        node.parents = parents
            .iter()
            .filter_map(|parent| place.get(parent).copied())
            .collect();
        for parent in &node.parents {
            children[*parent as usize] += 1;
        }
    }
    let refs = refs(repo)?;
    let mut lookups = Lookups::new(path);
    // The entry at `path` in each commit's tree, looked up once.
    let mut entries: Vec<Option<Option<ObjectId>>> = vec![None; nodes.len()];
    let mut entry = |at: u32, lookups: &mut Lookups<'_>| -> Result<Option<ObjectId>> {
        if let Some(known) = entries[at as usize] {
            return Ok(known);
        }
        let tree = repo.find_commit(nodes[at as usize].id)?.tree_id()?.detach();
        let found = lookups.at(repo, tree)?.map(|entry| entry.id);
        entries[at as usize] = Some(found);
        Ok(found)
    };
    // Ready: every child placed. The newest first, and of two as new the
    // one the walk read first.
    let mut ready: BinaryHeap<(i64, Reverse<u32>)> = nodes
        .iter()
        .enumerate()
        .filter(|(at, _)| children[*at] == 0)
        .map(|(at, node)| (node.time, Reverse(at as u32)))
        .collect();
    let mut commits = Vec::new();
    let mut more = false;
    while let Some((_, Reverse(at))) = ready.pop() {
        if commits.len() >= limit {
            more = true;
            break;
        }
        if commits.len() % 64 == 0 && deadline.passed() {
            stopped = true;
            break;
        }
        let node = &nodes[at as usize];
        for parent in &node.parents {
            let left = &mut children[*parent as usize];
            *left -= 1;
            if *left == 0 {
                ready.push((nodes[*parent as usize].time, Reverse(*parent)));
            }
        }
        let kept = path.is_empty() || {
            let own = entry(at, &mut lookups)?;
            if node.parents.is_empty() {
                own.is_some()
            } else {
                let mut differs = true;
                for parent in &node.parents {
                    if entry(*parent, &mut lookups)? == own {
                        differs = false;
                        break;
                    }
                }
                differs
            }
        };
        if !kept {
            continue;
        }
        let commit = repo.find_commit(node.id)?;
        let author = commit.author()?;
        let time = author.time()?;
        let message = commit.message()?;
        commits.push(Commit {
            id: node.id.to_string(),
            short: gix::prelude::ObjectIdExt::attach(node.id, repo)
                .shorten_or_id()
                .to_string(),
            parents: node
                .parents
                .iter()
                .map(|parent| nodes[*parent as usize].id.to_string())
                .collect(),
            author: cut(&author.name.to_str_lossy(), NAME),
            email: cut(&author.email.to_str_lossy(), NAME),
            time: time.seconds,
            offset: time.offset,
            summary: cut(message.summary().to_str_lossy().trim(), SUMMARY),
            refs: refs.get(&node.id).cloned().unwrap_or_default(),
        });
    }
    Ok(Log {
        head: branch,
        commits,
        more,
        stopped,
    })
}

/// `text`, `most` characters at most.
fn cut(text: &str, most: usize) -> String {
    match text.char_indices().nth(most) {
        Some((end, _)) => text[..end].to_owned(),
        None => text.to_owned(),
    }
}

/// The branches and tags at each commit, by the commit they lead to (a tag
/// peeled): local branches (`main`), then remote-tracking ones (`remote:
/// origin/main`), then tags (`tag: v1`), each by name: a local branch's
/// name may hold a `/` too.
fn refs(repo: &gix::Repository) -> Result<HashMap<ObjectId, Vec<String>>> {
    let mut found: HashMap<ObjectId, Vec<(u8, String)>> = HashMap::new();
    let platform = repo.references()?;
    for reference in platform.all()? {
        let Ok(mut reference) = reference else {
            continue;
        };
        let Some((category, short)) = reference.name().category_and_short_name() else {
            continue;
        };
        let short = short.to_str_lossy().into_owned();
        let (rank, label) = match category {
            Category::LocalBranch => (0, short),
            // `origin/HEAD` names the branch the remote is on, not one of its own.
            Category::RemoteBranch if !short.ends_with("/HEAD") => (1, format!("remote: {short}")),
            Category::Tag => (2, format!("tag: {short}")),
            _ => continue,
        };
        let Ok(id) = reference.peel_to_id() else {
            continue;
        };
        found.entry(id.detach()).or_default().push((rank, label));
    }
    Ok(found
        .into_iter()
        .map(|(id, mut labels)| {
            labels.sort();
            (id, labels.into_iter().map(|(_, label)| label).collect())
        })
        .collect())
}
