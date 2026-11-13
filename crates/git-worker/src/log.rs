//! The commits that reached `HEAD`, as `git log --date-order` lists them,
//! those that changed a path alone when one is asked.
use crate::tree::{Entry, Lookups};
use crate::{Deadline, Result, rename};
use gix::ObjectId;
use gix::bstr::{BString, ByteSlice};
use gix::refs::Category;
use serde::Serialize;
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};

/// A summary kept at most, in characters.
const SUMMARY: usize = 300;
/// An author's name or address kept at most, in characters.
const NAME: usize = 200;
/// A commit no child has given the file's name in it yet.
const UNNAMED: u32 = u32::MAX;

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
    /// In a file's history, the file's name in this commit, where it is
    /// not the one asked (before a rename).
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<String>,
}

/// A commit as the walk read it.
struct Node {
    id: ObjectId,
    time: i64,
    /// Its parents among those read, by place in `nodes`.
    parents: Vec<u32>,
}

/// The names a file had along its history, each looked up in the trees
/// once a tree and a depth (`Lookups`): the one asked first.
struct Followed {
    names: Vec<BString>,
    lookups: Vec<Lookups>,
    known: HashMap<BString, u32>,
}

impl Followed {
    fn new(path: &[&[u8]]) -> Self {
        let name: BString = path.join(&b'/').into();
        Self {
            names: vec![name.clone()],
            lookups: vec![Lookups::new(path)],
            known: HashMap::from([(name, 0)]),
        }
    }

    /// The place of `name`, given one the first time.
    fn of(&mut self, name: BString) -> u32 {
        if let Some(at) = self.known.get(&name) {
            return *at;
        }
        let at = self.names.len() as u32;
        let components: Vec<&[u8]> = name.split(|byte| *byte == b'/').collect();
        self.lookups.push(Lookups::new(&components));
        self.names.push(name.clone());
        self.known.insert(name, at);
        at
    }

    /// What tree `tree` holds under name `at`.
    fn entry(&mut self, repo: &gix::Repository, at: u32, tree: ObjectId) -> Result<Option<Entry>> {
        self.lookups[at as usize].at(repo, tree)
    }
}

/// The log of `repo` from `HEAD`: `limit` commits at most, those that
/// changed `path` alone when it is not empty, a file followed across its
/// renames.
///
/// The order is git's `--date-order`, which needs every commit's children
/// known before it is placed: the history is read whole first, by gix's
/// walk newest first (which a shallow clone's boundary ends), then placed
/// by Kahn's algorithm, a commit becoming ready once all its children are
/// placed, the newest ready placed first (a binary heap): n log n in the
/// commits, each read once. Read past the deadline, what was read is placed
/// and answered.
///
/// A file (not a folder) is followed as `git log --follow` follows it, but
/// along each line of history apart: each commit knows the file's name in
/// it, `HEAD`'s the one asked, a parent's given by its first child placed
/// (its newest). Where a child has the file and a parent has nothing under
/// its name, the parent's name is the file's the child renamed it from, if
/// any (`rename::source`, a tree diff of the two): asked once a rename, and
/// where the file begins.
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
    let mut names = Followed::new(path);
    // Each commit's tree, read once.
    let mut trees: Vec<Option<ObjectId>> = vec![None; nodes.len()];
    let mut tree = |at: u32, nodes: &[Node]| -> Result<ObjectId> {
        if let Some(known) = trees[at as usize] {
            return Ok(known);
        }
        let id = repo.find_commit(nodes[at as usize].id)?.tree_id()?.detach();
        trees[at as usize] = Some(id);
        Ok(id)
    };
    // A file is followed; a folder, or what `HEAD` lacks, is not.
    let follow = !path.is_empty() && {
        let top = place.get(&tip).copied().unwrap_or_default();
        nodes.get(top as usize).is_some()
            && names
                .entry(repo, 0, tree(top, &nodes)?)?
                .is_some_and(Entry::is_file)
    };
    // The file's name in each commit, by place in `names`; `UNNAMED` until
    // a child names it (the tip is named the one asked).
    let mut name_of = vec![UNNAMED; nodes.len()];
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
        if (follow || commits.len() % 64 == 0) && deadline.passed() {
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
        let name = match name_of[at as usize] {
            UNNAMED => 0,
            name => name,
        };
        let kept = path.is_empty() || {
            let own_tree = tree(at, &nodes)?;
            let own = names.entry(repo, name, own_tree)?;
            // Unchanged against a parent that has it alike under one name.
            let mut differs = true;
            for parent in &node.parents {
                let theirs_tree = tree(*parent, &nodes)?;
                let mut theirs_name = name;
                let mut theirs = names.entry(repo, name, theirs_tree)?;
                if follow
                    && theirs.is_none()
                    && let Some(file) = own.filter(|entry| entry.is_file())
                    && let Some(source) = rename::source(
                        repo,
                        theirs_tree,
                        own_tree,
                        &names.names[name as usize],
                        file,
                        deadline,
                    )?
                {
                    theirs_name = names.of(source.path);
                    theirs = Some(source.entry);
                }
                if name_of[*parent as usize] == UNNAMED {
                    name_of[*parent as usize] = theirs_name;
                }
                if theirs_name == name && theirs == own {
                    differs = false;
                }
            }
            if node.parents.is_empty() {
                own.is_some()
            } else {
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
            path: (name != 0).then(|| names.names[name as usize].to_str_lossy().into_owned()),
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
