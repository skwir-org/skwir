//! A folder's listing printed (P4): what a pane shows, in its order, as a
//! table of names, kinds, sizes and times modified, the folder its title.
//! The entries are copied under the workspace's lock and written out after
//! it, so a folder of 100,000 entries holds the lock only to copy them;
//! those whose metadata the pane has not read yet (entries arrive before
//! it) are read once, a folder at a time in its lane (`fs::read_paths`).
use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::Context as _;
use skwir_core::App;
use skwir_core::cancel::CancelToken;
use skwir_core::entry::{Entry, EntryId, keys};
use skwir_core::print::Part;
use skwir_core::workspace::PaneId;

/// The listing `pane` shows, and what it is a listing of: its folder's
/// path, else its location's.
pub(crate) fn listing(
    app: &App,
    pane: PaneId,
    cancel: &CancelToken,
) -> anyhow::Result<(String, Part)> {
    let title = app
        .pane_dir(pane)
        .map_or_else(|_| "Listing".to_owned(), |dir| dir.display().to_string());
    // Copied under the lock: an entry, how many opened folders deep it is
    // shown, and its path.
    let mut shown: Vec<(Entry, usize, Option<PathBuf>)> = {
        let ws = app.workspace.read();
        let pane = ws.pane(pane).context("No such pane")?;
        pane.order()
            .iter()
            .filter_map(|&id| Some((pane.entry(id)?.clone(), pane.depth(id), pane.path_of(id))))
            .collect()
    };
    let unread: Vec<(EntryId, PathBuf)> = shown
        .iter()
        .filter(|(entry, ..)| !entry.metadata.is_hydrated() && !entry.is_place())
        .filter_map(|(entry, _, path)| Some((entry.id, path.clone()?)))
        .collect();
    if !unread.is_empty() {
        let mut read: HashMap<EntryId, Entry> = HashMap::new();
        let finished = skwir_core::fs::read_paths(&unread, &app.metadata, cancel, |found, _| {
            read.extend(found.into_iter().map(|entry| (entry.id, entry)));
        });
        anyhow::ensure!(finished, "Printing was cancelled");
        for (entry, ..) in &mut shown {
            // Its identity and name are the pane's: the rest is read.
            if let Some(found) = read.remove(&entry.id) {
                entry.kind = found.kind;
                entry.target = found.target;
                entry.metadata = found.metadata;
            }
        }
    }
    let zone = jiff::tz::TimeZone::system();
    let rows = shown
        .iter()
        .map(|(entry, depth, _)| row(entry, *depth, &zone))
        .collect();
    Ok((
        title,
        Part::Table {
            columns: ["Name", "Kind", "Size", "Modified"]
                .map(str::to_owned)
                .to_vec(),
            rows,
        },
    ))
}

/// An entry's row: its name (indented by how deep it is shown, a folder's
/// ending with a slash), its kind, its size and when it was modified, as
/// the list view says them; what is not known yet is left blank.
fn row(entry: &Entry, depth: usize, zone: &jiff::tz::TimeZone) -> Vec<String> {
    let mut name = "    ".repeat(depth);
    name.push_str(&entry.name.to_string_lossy());
    if entry.is_dir() {
        name.push('/');
    }
    let kind = if entry.is_dir() {
        "Folder".to_owned()
    } else {
        skwir_platform::mime::of_entry(entry)
            .and_then(|mime| mime.description())
            .unwrap_or_default()
    };
    let size = entry
        .metadata
        .value(keys::SIZE)
        .and_then(skwir_core::entry::Value::as_u64)
        .filter(|_| !entry.is_dir())
        .map(skwir_platform::inspection::size)
        .unwrap_or_default();
    let modified = entry
        .metadata
        .value(keys::MODIFIED)
        .and_then(skwir_core::entry::Value::as_time)
        .and_then(|time| jiff::Timestamp::try_from(time).ok())
        .map(|at| {
            at.to_zoned(zone.clone())
                .strftime("%Y-%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_default();
    vec![name, kind, size, modified]
}

#[cfg(test)]
mod tests {
    use skwir_core::app::{App, AppConfig};
    use skwir_core::command::Invocation;
    use skwir_core::print::Part;

    /// A folder's listing is its entries as the pane shows them, each
    /// with its kind, size and time, nothing read again for it.
    #[test]
    fn a_folder_s_listing_is_what_its_pane_shows() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("letters 'n' notes");
        std::fs::create_dir_all(dir.join("drafts")).unwrap();
        std::fs::write(dir.join("b note.txt"), "hello").unwrap();
        std::fs::write(dir.join("a ünïcode.md"), "# Title\n").unwrap();
        let app = App::new(AppConfig::ephemeral(tmp.path().join("state"))).unwrap();
        let navigate = Invocation::with_args(
            "navigate",
            &serde_json::json!({ "location": dir.to_string_lossy() }),
        );
        app.dispatch(&navigate).unwrap();
        let pane = app.workspace.read().active_id();
        assert!(app.pump_until_complete(pane, std::time::Duration::from_secs(10)));
        let order: Vec<String> = {
            let ws = app.workspace.read();
            let shown = ws.pane(pane).unwrap();
            shown
                .order()
                .iter()
                .map(|&id| shown.entry(id).unwrap().name.to_string_lossy().into_owned())
                .collect()
        };
        let cancel = skwir_core::cancel::CancelToken::new();
        let (title, part) = super::listing(&app, pane, &cancel).unwrap();
        assert_eq!(title, dir.display().to_string());
        let Part::Table { columns, rows } = part else {
            panic!("a table");
        };
        assert_eq!(columns, ["Name", "Kind", "Size", "Modified"]);
        // In the pane's order, a folder marked as one.
        let names: Vec<&str> = rows
            .iter()
            .map(|row| row[0].trim_end_matches('/'))
            .collect();
        assert_eq!(names, order);
        let row = |name: &str| rows.iter().find(|row| row[0] == name).unwrap();
        assert_eq!(row("drafts/")[1], "Folder");
        assert_eq!(row("drafts/")[2], "", "a folder's size is not said");
        assert_eq!(row("b note.txt")[2], "5 B");
        assert!(!row("b note.txt")[1].is_empty(), "its kind");
        assert_eq!(row("a ünïcode.md")[3].len(), "2026-10-31 12:00".len());
    }
}
