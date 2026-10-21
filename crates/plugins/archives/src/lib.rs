//! Archive directory tree backed by the optional libarchive process,
//! archives extracted (`extract`, ADR-048), and archives made (`compress`,
//! ADR-059).
use skwir_core::{
    App, CancelToken,
    inspection::{InspectRequest, Inspection, Row},
    plugin::Plugin,
    preview::{PreviewProvider, PreviewRequest},
};
use skwir_platform::{
    archive,
    inspection::{self, limits, limits::Bounds},
    mime,
};
use std::{collections::BTreeMap, sync::Arc};

mod compress;
mod extract;
pub use compress::Compress;
pub use extract::Extract;

pub struct PreviewPlugin;
impl Plugin for PreviewPlugin {
    fn id(&self) -> &'static str {
        "archives"
    }
    fn title(&self) -> &'static str {
        "Archives"
    }
    fn description(&self) -> &'static str {
        "List the contents of ZIP, tar, 7z and other archives, preview their members without extracting, extract them (Extract Here, in an archive's menu and a drop's), and make them (Compress, in the menu of what you select)"
    }
    fn category(&self) -> skwir_core::plugin::Category {
        skwir_core::plugin::Category::Previews
    }
    fn requirements(&self) -> Vec<skwir_core::plugin::Requirement> {
        vec![
            archive::WORKER
                .requirement("Listing, reading, extracting and making archives")
                .required(),
        ]
    }
    fn capabilities(&self) -> skwir_core::permissions::Capabilities {
        use skwir_core::permissions::Capability;
        // It writes what it extracts, and the archives it makes.
        skwir_core::permissions::Capabilities::of(&[
            Capability::Read,
            Capability::Write,
            Capability::Programs,
        ])
    }
    fn register(&self, app: &App) {
        app.previews.register(self.id(), Arc::new(Provider));
        extract::register(app, self.id());
        compress::register(app, self.id());
    }
}
/// Disc images, which the database does not call archives.
static DISC_IMAGES: mime::Types = mime::Types::new(&["application/x-cd-image"]);

struct Provider;
impl PreviewProvider for Provider {
    fn id(&self) -> &'static str {
        "archives"
    }
    /// What the MIME database calls an archive (`mime::Kind::Archive`:
    /// a compressed tar, a package, a Java archive), and a disc image,
    /// which libarchive reads too.
    fn accepts(&self, req: &PreviewRequest) -> bool {
        mime::of_request(&req.entry.path, req.mime.as_deref()).is_some_and(|mime| {
            mime::is_kind(&mime, mime::Kind::Archive) || DISC_IMAGES.contains(mime.as_str())
        })
    }
    fn extract(
        &self,
        req: &PreviewRequest,
        view: &InspectRequest,
        target: &str,
        into: &std::path::Path,
        cancel: &CancelToken,
    ) -> anyhow::Result<Option<skwir_core::preview::Member>> {
        let path = req.entry.resolve()?;
        // Found as the listing showed it; written out as the member is to be
        // looked at.
        let member = archive::member(&path, target.parse::<u64>()?, cancel)?;
        anyhow::ensure!(
            member.regular && !member.encrypted,
            "This member cannot be previewed (directory, link or encrypted file)"
        );
        Ok(Some(skwir_core::preview::Member {
            own_file: false,
            name: member.file_name().to_owned(),
            path: archive::extract(&path, &member, into, Bounds::of(view), cancel)?,
        }))
    }
    /// The listing. A member is never inspected here: the panel extracts it
    /// and previews it as the file it is.
    fn inspect(
        &self,
        req: &PreviewRequest,
        view: &InspectRequest,
        cancel: &CancelToken,
    ) -> anyhow::Result<Option<Inspection>> {
        let path = req.entry.resolve()?;
        let bounds = Bounds::of(view);
        let listing = archive::list_within(&path, bounds, cancel)?;
        let members = &listing.members;
        let rows = member_rows(members, cancel)?;
        let details = facts(members, &rows);
        // What was left out is the one thing the strip has to say.
        let notice = if listing.complete {
            String::new()
        } else {
            format!(
                "First {} members · open it in an archive application for the rest",
                members.len()
            )
        };
        Ok(Some(Inspection {
            details,
            kind: skwir_core::inspection::InspectionKind::Archive,
            title: format!("Archive · {} members", members.len()),
            notice,
            rows,
            columns: vec!["Name".into(), "Size".into(), "Status".into()],
            whole_available: !listing.complete && bounds.cut(),
            ..Inspection::default()
        }))
    }
}

/// What the listing says about the archive, for the panel's Properties.
fn facts(members: &[archive::Member], rows: &[Row]) -> Vec<skwir_core::inspection::Detail> {
    let folders = rows.iter().filter(|r| r.branch).count();
    // Declared sizes are the archive's word: a hostile one must not
    // overflow the total.
    let unpacked = members
        .iter()
        .filter(|member| !member.directory)
        .filter_map(|member| u64::try_from(member.size).ok())
        .fold(0u64, u64::saturating_add);
    let encrypted = members.iter().filter(|member| member.encrypted).count();
    inspection::details([
        ("Files", inspection::count(rows.len() - folders)),
        ("Folders", inspection::count(folders)),
        (
            "Unpacked",
            (unpacked > 0).then(|| inspection::size(unpacked)),
        ),
        (
            "Encrypted",
            inspection::count(encrypted).map(|count| format!("{count} members")),
        ),
    ])
}

fn member_rows(members: &[archive::Member], cancel: &CancelToken) -> anyhow::Result<Vec<Row>> {
    let mut rows = BTreeMap::new();
    for member in members {
        inspection::check(cancel)?;
        let parts: Vec<_> = member.name.split('/').filter(|p| !p.is_empty()).collect();
        anyhow::ensure!(
            parts.len() <= limits::NESTING,
            "Archive path exceeds {} components",
            limits::NESTING
        );
        for end in 1..=parts.len() {
            let name: Vec<String> = parts[..end].iter().map(|s| (*s).to_owned()).collect();
            let directory = end < parts.len() || member.directory;
            let bytes = (!directory)
                .then(|| u64::try_from(member.size).ok())
                .flatten();
            rows.entry(name).or_insert_with(|| Row {
                cells: vec![
                    parts[end - 1].into(),
                    bytes.map(inspection::size).unwrap_or_default(),
                    if directory || (member.regular && !member.encrypted) {
                        String::new()
                    } else if member.encrypted {
                        "Encrypted".into()
                    } else {
                        "Link / special".into()
                    },
                ],
                bytes,
                depth: end - 1,
                branch: directory,
                target: (!directory && member.regular && !member.encrypted)
                    .then(|| member.index.to_string()),
                section: None,
            });
        }
    }
    // Sort sibling folders first while keeping every subtree contiguous for
    // the viewer's depth-based collapse operation. Never sort flattened rows.
    let mut ordered: Vec<_> = rows
        .iter()
        .map(|(path, row)| {
            let key: Vec<_> = path
                .iter()
                .enumerate()
                .map(|(i, part)| {
                    let branch = rows.get(&path[..=i]).is_some_and(|r| r.branch);
                    (!branch, part.to_lowercase(), part.clone())
                })
                .collect();
            (key, row.clone())
        })
        .collect();
    ordered.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(ordered.into_iter().map(|(_, row)| row).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Types from the free desktops' database (step E, ADR-035).
    #[cfg(target_os = "linux")]
    #[test]
    fn archives_are_what_the_database_calls_archives() {
        let accepts = |name: &str| {
            Provider.accepts(&PreviewRequest {
                entry: skwir_core::op::EntryRef::new(skwir_core::FileId::new(1, 1), name),
                mime: None,
                page: 1,
                seconds: None,
                max_side: 64,
                text_size: None,
                fit: None,
            })
        };
        for name in ["a.zip", "a.tar.gz", "a.7z", "a.deb", "a.jar", "a.iso"] {
            assert!(accepts(name), "{name} is an archive");
        }
        // Packages of documents are their readers'.
        for name in ["a.docx", "a.epub", "a.cbz", "a.txt"] {
            assert!(!accepts(name), "{name} is no archive");
        }
    }

    #[test]
    fn folder_first_order_keeps_subtrees_and_member_identity() {
        let members: Vec<_> = [
            "a.txt",
            "Zoo's/child.bin",
            "Zoo's/nested/last.txt",
            "Zoo's.ext",
            "alpha/empty",
            "secret.zip",
            "shortcut",
        ]
        .iter()
        .enumerate()
        .map(|(i, name)| archive::Member {
            index: i as u64,
            name: (*name).into(),
            size: if i == 6 { -1 } else { 1536 },
            directory: false,
            regular: i != 6,
            encrypted: i == 5,
            ..Default::default()
        })
        .collect();
        let rows = member_rows(&members, &CancelToken::new()).unwrap();
        assert_eq!(
            rows.iter()
                .map(|r| (r.cells[0].as_str(), r.depth))
                .collect::<Vec<_>>(),
            vec![
                ("alpha", 0),
                ("empty", 1),
                ("Zoo's", 0),
                ("nested", 1),
                ("last.txt", 2),
                ("child.bin", 1),
                ("a.txt", 0),
                ("secret.zip", 0),
                ("shortcut", 0),
                ("Zoo's.ext", 0),
            ]
        );
        assert!(rows[2].branch);
        assert_eq!(rows[2].bytes, None);
        assert_eq!(rows[5].bytes, Some(1536));
        assert_eq!(rows[5].target.as_deref(), Some("1"));
        assert_eq!(rows[7].cells[2], "Encrypted");
        assert_eq!(rows[7].target, None);
        assert_eq!(rows[8].bytes, None);
        assert_eq!(rows[8].target, None);
        assert_eq!(rows[5].cells[1], "1.5 KB");
    }

    #[test]
    fn hostile_declared_sizes_saturate_the_total() {
        let members: Vec<_> = (0..3)
            .map(|i| archive::Member {
                index: i,
                name: format!("huge{i}"),
                size: i64::MAX,
                directory: false,
                regular: true,
                encrypted: false,
                ..Default::default()
            })
            .collect();
        let rows = member_rows(&members, &CancelToken::new()).unwrap();
        let facts = facts(&members, &rows);
        let unpacked = facts.iter().find(|f| f.label == "Unpacked").unwrap();
        assert_eq!(unpacked.value, inspection::size(u64::MAX));
        assert_eq!(facts[0], skwir_core::inspection::Detail::new("Files", "3"));
        assert!(facts.iter().all(|f| f.label != "Folders"));
    }
}
