//! CSV/TSV tables with bounded, streamed record parsing and paged navigation.
use bibli_core::{
    App, CancelToken,
    inspection::{InspectRequest, Inspection, Row},
    plugin::Plugin,
    preview::{PreviewProvider, PreviewRequest},
};
use bibli_platform::inspection;
use std::sync::Arc;
pub struct PreviewPlugin;
impl Plugin for PreviewPlugin {
    fn id(&self) -> &'static str {
        "preview-tables"
    }
    fn title(&self) -> &'static str {
        "Preview: CSV and TSV"
    }
    fn register(&self, app: &App) {
        app.previews.register(self.id(), Arc::new(Provider));
    }
}
struct Provider;
impl PreviewProvider for Provider {
    fn id(&self) -> &'static str {
        "preview-tables"
    }
    fn accepts(&self, req: &PreviewRequest) -> bool {
        matches!(
            inspection::extension(&req.entry.path).as_str(),
            "csv" | "tsv"
        )
    }
    fn inspect(
        &self,
        req: &PreviewRequest,
        view: &InspectRequest,
        cancel: &CancelToken,
    ) -> anyhow::Result<Option<Inspection>> {
        let path = req.entry.resolve()?;
        if view.source {
            return inspection::source(&path, view.offset, cancel).map(Some);
        }
        // Bound record allocation as well as row count (one quoted field can be huge).
        let source = inspection::text(&path, cancel)?;
        let mut reader = csv::ReaderBuilder::new()
            .delimiter(if inspection::extension(&path) == "tsv" {
                b'\t'
            } else {
                b','
            })
            .flexible(true)
            .from_reader(source.as_bytes());
        let columns = reader
            .headers()?
            .iter()
            .take(64)
            .map(str::to_owned)
            .collect();
        let mut rows = Vec::new();
        let mut more = false;
        for (i, record) in reader.records().enumerate() {
            inspection::check(cancel)?;
            let record = record?;
            if (i as u64) < view.offset {
                continue;
            }
            if rows.len() == 500 {
                more = true;
                break;
            }
            anyhow::ensure!(
                record.len() <= 64,
                "Table preview supports at most 64 columns"
            );
            rows.push(Row {
                cells: record.iter().map(str::to_owned).collect(),
                ..Row::default()
            });
        }
        Ok(Some(Inspection {
            kind: bibli_core::inspection::InspectionKind::Table,
            title: format!(
                "Table · rows {}–{}",
                view.offset + 1,
                view.offset + rows.len() as u64
            ),
            columns,
            rows,
            position: view.offset,
            previous: (view.offset > 0).then(|| view.offset.saturating_sub(500)),
            next: more.then_some(view.offset + 500),
            source_available: true,
            ..Inspection::default()
        }))
    }
}
