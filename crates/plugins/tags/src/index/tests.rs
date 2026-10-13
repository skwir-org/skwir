use super::*;

/// The index's folders as the cache keeps them, for `any_input`.
pub(crate) fn encoded_folders(folders: &BTreeMap<PathBuf, usize>) -> Vec<u8> {
    encode_folders(folders)
}

pub(crate) fn decoded_folders(bytes: &[u8]) -> BTreeMap<PathBuf, usize> {
    decode_folders(bytes)
}

/// How many folders a piece read held.
pub(crate) fn decoded_piece(bytes: &[u8], parents: &[Option<u32>]) -> Option<usize> {
    decode_piece(bytes, parents).map(|folders| folders.len())
}

fn home_tempdir() -> tempfile::TempDir {
    let base = dirs::home_dir().unwrap().join(".cache").join("skwir-tests");
    std::fs::create_dir_all(&base).unwrap();
    tempfile::Builder::new()
        .prefix("tags-index-")
        .tempdir_in(base)
        .unwrap()
}

/// An index of `megabytes`, stored in `slot`, of no app: nothing written
/// unless asked.
fn index(slot: Option<CacheSlot>, megabytes: u64) -> Arc<TagIndex> {
    Arc::new_cyclic(|me| TagIndex {
        state: Mutex::default(),
        slot,
        lifetime: CancelToken::new(),
        pending: AtomicBool::new(false),
        written: Mutex::new(None),
        on_change: Mutex::default(),
        limit: AtomicU64::new(megabytes << 20),
        working: AtomicBool::new(false),
        nudge: crossbeam_channel::bounded(1),
        me: me.clone(),
    })
}

fn tags(t: &[&str]) -> Vec<String> {
    t.iter().map(|s| (*s).to_owned()).collect()
}

#[test]
fn the_most_used_tags_come_first_spelled_as_most_files_spell_them() {
    let index = index(None, 64);
    let file = |i: usize| PathBuf::from(format!("/r/{i}"));
    index.learn(vec![
        (file(0), tags(&["Work", "home"])),
        (file(1), tags(&["work", "Été"])),
        (file(2), tags(&["Work"])),
        (file(3), tags(&["été", "zeta"])),
        (file(4), tags(&["alpha"])),
    ]);
    index.learn(vec![(PathBuf::from("/elsewhere/5"), tags(&["zeta"]))]);
    assert_eq!(
        index.top(4, Path::new("/r")),
        [
            ("Work".to_owned(), 3),
            // Two files, one spelling each: the one sorting first.
            ("Été".to_owned(), 2),
            ("alpha".to_owned(), 1),
            ("home".to_owned(), 1),
        ]
    );
    assert!(index.top(0, Path::new("/r")).is_empty());
    assert_eq!(
        index.top(9, Path::new("/elsewhere")),
        [("zeta".to_owned(), 1)]
    );
}

#[test]
fn the_index_learns_tags_and_their_removal_and_forgets_the_gone() {
    let tmp = home_tempdir();
    let (a, b) = (tmp.path().join("it's a"), tmp.path().join("b"));
    std::fs::write(&a, b"x").unwrap();
    std::fs::write(&b, b"x").unwrap();
    let index = index(None, 64);
    index.learn(vec![
        (a.clone(), tags(&["Work", "home"])),
        (b.clone(), tags(&["work"])),
    ]);
    let mut found = index.files_with("WORK", tmp.path());
    found.sort();
    assert_eq!(found, [b.clone(), a.clone()]);
    assert_eq!(
        index.files_with("home", tmp.path()),
        std::slice::from_ref(&a)
    );
    assert!(
        index
            .files_with("home", &tmp.path().join("elsewhere"))
            .is_empty()
    );
    // Read again without tags: forgotten.
    index.learn(vec![(a.clone(), Vec::new())]);
    assert_eq!(
        index.files_with("work", tmp.path()),
        std::slice::from_ref(&b)
    );
    // Gone from the disk: forgotten when asked.
    std::fs::remove_file(&b).unwrap();
    assert!(index.files_with("work", tmp.path()).is_empty());
    assert!(index.state().folders.is_empty());
    assert_eq!(index.state().total(), 0, "the room is given back");
}

#[test]
fn past_its_room_it_learns_no_new_file_but_keeps_those_known_current() {
    let index = index(None, 1);
    let read: Vec<(PathBuf, Vec<String>)> = (0..100_000)
        .map(|i| (PathBuf::from(format!("/r/{}/{i}", i % 50)), tags(&["t"])))
        .collect();
    index.learn(read);
    let held = index.tally(Path::new("/r")).1;
    assert!(held > 5_000 && held < 100_000, "{held}");
    assert!(index.is_full());
    assert!(index.state().total() <= 1 << 20);
    // Those known stay current.
    index.learn(vec![(PathBuf::from("/r/0/0"), tags(&["u"]))]);
    assert_eq!(
        index.files_with("u", Path::new("/r")).len(),
        0,
        "gone from the disk"
    );
    index.learn(vec![(PathBuf::from("/r/0/0"), tags(&["u"]))]);
    assert_eq!(index.top(1, Path::new("/r/0/0")).len(), 0);
    // More room: it learns again.
    index.set_limit(64);
    assert!(!index.is_full());
    index.learn(vec![(PathBuf::from("/r/new/x"), tags(&["v"]))]);
    assert_eq!(index.tally(Path::new("/r/new")).1, 1);
}

#[test]
fn it_is_kept_in_pieces_read_when_needed_and_moves_carry_tags_along() {
    let tmp = home_tempdir();
    let slot = || Some(skwir_core::cache::Cache::new(tmp.path()).namespace("tags"));
    let first = index(slot(), 64);
    first.learn(vec![
        (PathBuf::from("/r/a"), tags(&["x"])),
        (PathBuf::from("/r/.hidden/b"), tags(&["x"])),
        (PathBuf::from("/r/d/c"), tags(&["X", "y"])),
        (PathBuf::from("/r/d/.c"), tags(&["z"])),
    ]);
    // Hidden folders and files are not counted, as a search leaves them
    // out; a tie between spellings goes to the first in order.
    assert_eq!(
        first.tally(Path::new("/r")),
        (vec![("X".into(), 2), ("y".into(), 1)], 2)
    );
    let since = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    first.cover(Path::new("/r"), since, &[7, 9]);
    first.write();
    let room = first.state().total();

    // Read again, as the next run reads it: nothing but what is known
    // between pieces until a question needs one.
    let again = index(slot(), 64);
    let covered = again.covering(Path::new("/r/d")).unwrap();
    assert_eq!(
        (covered.root.as_path(), covered.since()),
        (Path::new("/r"), since)
    );
    assert_eq!(again.dirs(Path::new("/r")), [7, 9].into());
    assert!(again.covering(Path::new("/s")).is_none());
    assert_eq!(again.state().total(), room);
    assert!(again.state().pieces.iter().all(|p| !p.loaded));
    assert_eq!(again.tally(Path::new("/r/d")).1, 1);
    assert_eq!(
        again.state().pieces.iter().filter(|p| p.loaded).count(),
        1,
        "only /r/d's piece"
    );
    // Let go of when unused, read again when asked.
    {
        let mut state = again.state();
        for piece in &mut state.pieces {
            piece.used = Instant::now().checked_sub(IDLE);
        }
    }
    assert!(!again.unload_idle());
    assert_eq!(again.tally(Path::new("/r")).1, 2);

    // A folder moved carries its files' tags; one gone, its files.
    again.moved(&[
        (PathBuf::from("/r/d"), Some(PathBuf::from("/r/e"))),
        (PathBuf::from("/r/a"), None),
    ]);
    assert_eq!(
        again
            .by_folder(Path::new("/r"))
            .get(Path::new("/r/e"))
            .map(Vec::len),
        Some(2)
    );
    assert_eq!(
        again.tally(Path::new("/r")),
        (vec![("X".into(), 1), ("y".into(), 1)], 1)
    );
    // A covered folder moved away is no longer covered.
    again.moved(&[(PathBuf::from("/r"), Some(PathBuf::from("/q")))]);
    assert!(again.covering(Path::new("/r")).is_none());
    assert_eq!(again.tally(Path::new("/q")).1, 1);
}

/// What the index takes, against `FILE_BYTES` and `FOLDER_BYTES`:
/// `cargo nextest run --release -p skwir-plugin-tags scale_probe
/// --run-ignored only --no-capture`.
#[test]
#[ignore = "measures memory"]
fn scale_probe() {
    fn rss_kb() -> u64 {
        std::fs::read_to_string("/proc/self/status")
            .unwrap()
            .lines()
            .find(|l| l.starts_with("VmRSS:"))
            .unwrap()
            .split_whitespace()
            .nth(1)
            .unwrap()
            .parse()
            .unwrap()
    }
    let words = [
        "vacation", "alps", "work", "family", "2024", "todo", "receipts", "kids",
    ];
    let n = 500_000usize;
    let index = index(None, 4096);
    let before = rss_kb();
    // In batches, so what is read weighs nothing beside the index.
    for batch in (0..n).collect::<Vec<_>>().chunks(5_000) {
        let read: Vec<(PathBuf, Vec<String>)> = batch
            .iter()
            .map(|&i| {
                let path = PathBuf::from(format!(
                    "/home/someone/Pictures/{}/{}/IMG_{i:06}.jpg",
                    2000 + i % 25,
                    i % 400
                ));
                (
                    path,
                    (0..=(i % 3))
                        .map(|k| words[(i + k * 3) % 8].to_owned())
                        .collect(),
                )
            })
            .collect();
        index.learn(read);
    }
    let after = rss_kb();
    let t = Instant::now();
    let (_, files) = index.tally(Path::new("/home/someone"));
    let tally = t.elapsed();
    let t = Instant::now();
    let pieces: usize = index
        .state()
        .pieces
        .iter()
        .map(|p| encode_piece(p).len())
        .sum();
    let encode = t.elapsed();
    eprintln!(
        "n={n} files={files}: memory +{} MB, counted as {} MB, {} MB stored, tally {tally:?}, encode all {encode:?}",
        after.saturating_sub(before) / 1024,
        index.state().total() >> 20,
        pieces >> 20,
    );
}

#[test]
fn a_category_counts_each_file_within_it_once() {
    let tmp = home_tempdir();
    let file = |name: &str| {
        let path = tmp.path().join(name);
        std::fs::write(&path, b"x").unwrap();
        path
    };
    let (a, b, c) = (file("a"), file("b"), file("c"));
    let index = index(None, 64);
    index.learn(vec![
        (a.clone(), tags(&["Places/France/Paris", "Places/Japan"])),
        (b.clone(), tags(&["Places/France", "work"])),
        (c.clone(), tags(&["work"])),
    ]);
    let (counts, files) = index.tally(tmp.path());
    assert_eq!(files, 3);
    assert_eq!(
        counts,
        [
            ("Places".to_owned(), 2),
            ("Places/France".to_owned(), 2),
            ("Places/France/Paris".to_owned(), 1),
            ("Places/Japan".to_owned(), 1),
            ("work".to_owned(), 2),
        ],
        "a with two tags in Places counts once there"
    );
    let mut found = index.files_with("places", tmp.path());
    found.sort();
    assert_eq!(found, [a.clone(), b.clone()]);
    assert_eq!(
        index.files_with("Places/Japan", tmp.path()),
        std::slice::from_ref(&a)
    );
    // Taken out of the category, the file leaves its count.
    index.learn(vec![(a.clone(), tags(&["Places/Japan"]))]);
    let (counts, _) = index.tally(tmp.path());
    assert!(
        counts.contains(&("Places/France".to_owned(), 1)),
        "{counts:?}"
    );
    assert!(!counts.iter().any(|(t, _)| t == "Places/France/Paris"));
    assert!(counts.contains(&("Places".to_owned(), 2)));
}
