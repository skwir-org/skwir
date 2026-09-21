//! CSV/TSV tables with bounded, streamed record parsing and paged navigation.
use bibli_core::{
    App, CancelToken,
    inspection::{InspectRequest, Inspection, Row},
    plugin::Plugin,
    preview::{PreviewProvider, PreviewRequest},
};
use bibli_platform::inspection::{self, limits};
use std::sync::Arc;
pub struct PreviewPlugin;
impl Plugin for PreviewPlugin {
    fn id(&self) -> &'static str {
        "preview-tables"
    }
    fn title(&self) -> &'static str {
        "Preview: CSV and TSV"
    }
    fn description(&self) -> &'static str {
        "Show CSV and TSV files as aligned tables"
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
        // The whole input is bounded, and each cell is cut to TABLE_CELL
        // characters so one huge quoted field cannot dominate a row.
        let source = inspection::text(&path, cancel)?;
        let mut reader = csv::ReaderBuilder::new()
            .delimiter(if inspection::extension(&path) == "tsv" {
                b'\t'
            } else {
                b','
            })
            .flexible(true)
            .from_reader(source.as_bytes());
        let mut truncated = 0usize;
        let columns: Vec<String> = reader
            .headers()?
            .iter()
            .take(limits::TABLE_COLUMNS)
            .map(|cell| bounded_cell(cell, &mut truncated))
            .collect();
        let mut rows = Vec::new();
        let mut more = false;
        for (i, record) in reader.records().enumerate() {
            inspection::check(cancel)?;
            let record = record?;
            if (i as u64) < view.offset {
                continue;
            }
            if rows.len() == limits::TABLE_ROWS {
                more = true;
                break;
            }
            anyhow::ensure!(
                record.len() <= limits::TABLE_COLUMNS,
                "Table preview supports at most {} columns",
                limits::TABLE_COLUMNS
            );
            rows.push(Row {
                cells: record
                    .iter()
                    .map(|cell| bounded_cell(cell, &mut truncated))
                    .collect(),
                ..Row::default()
            });
        }
        let page = limits::TABLE_ROWS as u64;
        let details = [
            ("Columns", columns.len().to_string()),
            (
                "Rows",
                if more || view.offset > 0 {
                    format!("more than {}", view.offset + rows.len() as u64)
                } else {
                    rows.len().to_string()
                },
            ),
        ]
        .into_iter()
        .filter_map(|(label, value)| inspection::detail(label, value))
        .collect();
        Ok(Some(Inspection {
            details,
            kind: bibli_core::inspection::InspectionKind::Table,
            title: format!(
                "Table · rows {}–{}",
                view.offset + 1,
                view.offset + rows.len() as u64
            ),
            columns,
            rows,
            position: view.offset,
            previous: (view.offset > 0).then(|| view.offset.saturating_sub(page)),
            next: more.then_some(view.offset + page),
            source_available: true,
            notice: if truncated > 0 {
                format!(
                    "{truncated} cell{} longer than {} characters shortened",
                    if truncated == 1 { "" } else { "s" },
                    limits::TABLE_CELL
                )
            } else {
                String::new()
            },
            ..Inspection::default()
        }))
    }
}

/// Keep at most `TABLE_CELL` characters of a cell and mark the cut visibly.
fn bounded_cell(cell: &str, truncated: &mut usize) -> String {
    match cell.char_indices().nth(limits::TABLE_CELL) {
        Some((end, _)) => {
            *truncated += 1;
            format!("{}…", &cell[..end])
        }
        None => cell.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cells_are_cut_at_the_limit_and_counted() {
        let mut truncated = 0;
        let short = "é".repeat(limits::TABLE_CELL);
        assert_eq!(bounded_cell(&short, &mut truncated), short);
        assert_eq!(truncated, 0);
        let long = "é".repeat(limits::TABLE_CELL + 1);
        let cut = bounded_cell(&long, &mut truncated);
        assert_eq!(cut.chars().count(), limits::TABLE_CELL + 1);
        assert!(cut.ends_with('…'));
        assert_eq!(truncated, 1);
    }
}
