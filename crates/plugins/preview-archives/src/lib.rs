//! Archive directory tree backed by the optional libarchive process.
use bibli_core::{
    App, CancelToken,
    inspection::{InspectRequest, Inspection, Row},
    plugin::Plugin,
    preview::{PreviewProvider, PreviewRequest},
};
use bibli_platform::{archive, inspection};
use std::{collections::BTreeMap, sync::Arc};
pub struct PreviewPlugin;
impl Plugin for PreviewPlugin {
    fn id(&self) -> &'static str {
        "preview-archives"
    }
    fn title(&self) -> &'static str {
        "Preview: archives"
    }
    fn register(&self, app: &App) {
        app.previews.register(self.id(), Arc::new(Provider));
    }
}
struct Provider;
impl PreviewProvider for Provider {
    fn id(&self) -> &'static str {
        "preview-archives"
    }
    fn accepts(&self, req: &PreviewRequest) -> bool {
        matches!(
            inspection::extension(&req.entry.path).as_str(),
            "zip"
                | "7z"
                | "rar"
                | "tar"
                | "gz"
                | "bz2"
                | "xz"
                | "zst"
                | "tgz"
                | "tbz2"
                | "txz"
                | "cpio"
                | "iso"
                | "cab"
                | "ar"
                | "deb"
                | "rpm"
        )
    }
    fn inspect(
        &self,
        req: &PreviewRequest,
        view: &InspectRequest,
        cancel: &CancelToken,
    ) -> anyhow::Result<Option<Inspection>> {
        let path = req.entry.resolve()?;
        let members = archive::list(&path, cancel)?;
        if let Some(target) = &view.target {
            let index = target.parse::<u64>()?;
            let member = members
                .iter()
                .find(|m| m.index == index)
                .ok_or_else(|| anyhow::anyhow!("Missing archive member"))?;
            anyhow::ensure!(
                member.regular && !member.encrypted,
                "This member cannot be previewed (directory, link or encrypted file)"
            );
            return archive::member_preview(&member.name, &archive::read(&path, index, cancel)?)
                .map(Some);
        }
        let mut rows = BTreeMap::new();
        for member in &members {
            inspection::check(cancel)?;
            let parts: Vec<_> = member.name.split('/').filter(|p| !p.is_empty()).collect();
            anyhow::ensure!(parts.len() <= 64, "Archive path exceeds 64 components");
            for end in 1..=parts.len() {
                let name: Vec<String> = parts[..end].iter().map(|s| (*s).to_owned()).collect();
                let directory = end < parts.len() || member.directory;
                rows.entry(name).or_insert_with(|| Row {
                    cells: vec![
                        parts[end - 1].into(),
                        if directory {
                            String::new()
                        } else {
                            format!(
                                "{} bytes{}",
                                member.size,
                                if member.encrypted {
                                    " · encrypted"
                                } else {
                                    ""
                                }
                            )
                        },
                    ],
                    depth: end - 1,
                    branch: directory,
                    target: (!directory && member.regular && !member.encrypted)
                        .then(|| member.index.to_string()),
                });
            }
        }
        Ok(Some(Inspection {
            kind: bibli_core::inspection::InspectionKind::Archive,
            title: format!("Archive · {} members", members.len()),
            rows: rows.into_values().collect(),
            columns: vec!["Member".into(), "Size".into()],
            notice: "Select a member to preview · folders collapse · links are never followed"
                .into(),
            ..Inspection::default()
        }))
    }
}
