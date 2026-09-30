//! A `tag:` search asks the indexes first (ADR-026): the tags plugin's own,
//! learned as it reads files, and any other hint provider. What they name
//! is checked like any file and never shown twice; what the walk would not
//! reach (a hidden folder, a folder through a link) stays out.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use bibli_core::app::{App, AppConfig};
use bibli_core::cancel::CancelToken;
use bibli_core::entry::keys;
use bibli_core::hints::HintProvider;
use bibli_core::location::ListingEvent;

/// A folder where extended attributes work: not `/tmp`, often a tmpfs.
fn home_tempdir() -> tempfile::TempDir {
    let base = dirs::home_dir().unwrap().join(".cache").join("bibli-tests");
    std::fs::create_dir_all(&base).unwrap();
    tempfile::Builder::new()
        .prefix("search-hints-")
        .tempdir_in(base)
        .unwrap()
}

/// Each file a search found, with how many times it was sent.
fn search(app: &App, expr: &str, root: &Path) -> BTreeMap<String, usize> {
    let location = bibli_plugin_query::location_for(expr, Some(root));
    let open = app.open_location(&location).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut found = BTreeMap::new();
    loop {
        match open
            .events()
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .expect("the search completes")
        {
            ListingEvent::Added(batch) => {
                for e in batch {
                    let path = open.path_of(e.id).unwrap();
                    let relative = path.strip_prefix(root).unwrap().display().to_string();
                    *found.entry(relative).or_default() += 1;
                }
            }
            ListingEvent::Complete => break,
            ListingEvent::Error(e) => panic!("{e}"),
            _ => {}
        }
    }
    open.close();
    found
}

/// Names files a walk would never show, or that are not tagged.
struct Planted(Vec<PathBuf>);

impl HintProvider for Planted {
    fn id(&self) -> &'static str {
        "planted"
    }
    fn candidates(&self, _: &str, _: &str, _: &Path, _: &CancelToken) -> Option<Vec<PathBuf>> {
        Some(self.0.clone())
    }
}

#[test]
fn a_tag_search_asks_the_index_first_and_trusts_only_the_files() {
    let tmp = home_tempdir();
    let root = tmp.path().join("it's tagged");
    for dir in ["", ".hidden", "real", "deep/er"] {
        std::fs::create_dir_all(root.join(dir)).unwrap();
    }
    std::os::unix::fs::symlink(root.join("real"), root.join("link")).unwrap();
    let tagged = [
        "a.txt",
        ".hidden/b.txt",
        "real/c.txt",
        "deep/er/d.txt",
        "stale.txt",
    ];
    for name in tagged.iter().chain(&["plain.txt"]) {
        std::fs::write(root.join(name), name).unwrap();
    }
    for name in tagged {
        if xattr::set(root.join(name), "user.xdg.tags", b"Work,other").is_err() {
            eprintln!("skipped: no user extended attributes here");
            return;
        }
    }
    let root = std::fs::canonicalize(&root).unwrap();
    let app = App::new(AppConfig::ephemeral(tmp.path().join("state"))).unwrap();
    app.plugins
        .add(Arc::new(bibli_plugin_query::QueryPlugin::default()), &app);
    app.plugins
        .add(Arc::new(bibli_plugin_tags::TagsPlugin::default()), &app);
    let once = |names: &[&str]| -> BTreeMap<String, usize> {
        names.iter().map(|n| ((*n).to_owned(), 1)).collect()
    };
    let expected = once(&["a.txt", "deep/er/d.txt", "real/c.txt", "stale.txt"]);

    // The first search walks; the index learns what the provider read.
    assert_eq!(search(&app, "tag:work", &root), expected);
    let known = |app: &App| {
        let mut known: Vec<String> = app
            .hints
            .candidates(keys::TAGS, "WORK", &root, &CancelToken::new())
            .unwrap()
            .iter()
            .map(|p| p.strip_prefix(&root).unwrap().display().to_string())
            .collect();
        known.sort();
        known
    };
    assert_eq!(
        known(&app),
        ["a.txt", "deep/er/d.txt", "real/c.txt", "stale.txt"]
    );
    // A search with hidden files reads the hidden one too.
    assert!(search(&app, "tag:work hidden:true", &root).contains_key(".hidden/b.txt"));
    assert!(known(&app).contains(&".hidden/b.txt".to_owned()));

    // Changed behind its back: the index still names it, the file says no.
    xattr::remove(root.join("stale.txt"), "user.xdg.tags").unwrap();
    // Named by another index: through a link, not tagged, gone, elsewhere.
    app.hints.register(
        "test",
        Arc::new(Planted(vec![
            root.join("link/c.txt"),
            root.join("plain.txt"),
            root.join("gone.txt"),
            tmp.path().join("state"),
            root.join("a.txt"),
        ])),
    );
    let expected = once(&["a.txt", "deep/er/d.txt", "real/c.txt"]);
    assert_eq!(search(&app, "tag:work", &root), expected, "each once");
    assert!(
        !known(&app).contains(&"stale.txt".to_owned()),
        "read again, it is forgotten"
    );
    // Only a tag every match must have is asked: an OR walks as before.
    assert_eq!(
        search(&app, "tag:work OR name:plain", &root),
        once(&["a.txt", "deep/er/d.txt", "plain.txt", "real/c.txt"])
    );
}
