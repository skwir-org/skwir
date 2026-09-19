//! JSON, YAML and TOML trees; native UI owns expansion and text selection.
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
        "preview-structured"
    }
    fn title(&self) -> &'static str {
        "Preview: structured data"
    }
    fn register(&self, app: &App) {
        app.previews.register(self.id(), Arc::new(Provider));
    }
}
struct Provider;
impl PreviewProvider for Provider {
    fn id(&self) -> &'static str {
        "preview-structured"
    }
    fn accepts(&self, req: &PreviewRequest) -> bool {
        matches!(
            inspection::extension(&req.entry.path).as_str(),
            "json" | "yaml" | "yml" | "toml"
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
        let text = inspection::text(&path, cancel)?;
        let value: serde_json::Value = match inspection::extension(&path).as_str() {
            "toml" => serde_json::to_value(toml::from_str::<toml::Value>(&text)?)?,
            "yaml" | "yml" => serde_yaml_ng::from_str(&text)?,
            _ => serde_json::from_str(&text)?,
        };
        let mut rows = Vec::new();
        flatten("root", &value, 0, &mut rows, cancel)?;
        Ok(Some(Inspection {
            kind: bibli_core::inspection::InspectionKind::Tree,
            title: "Structured data".into(),
            rows,
            columns: vec!["Key".into(), "Value".into()],
            source_available: true,
            ..Inspection::default()
        }))
    }
}
fn flatten(
    key: &str,
    value: &serde_json::Value,
    depth: usize,
    rows: &mut Vec<Row>,
    cancel: &CancelToken,
) -> anyhow::Result<()> {
    inspection::check(cancel)?;
    anyhow::ensure!(
        depth <= 64 && rows.len() < 10000,
        "Structured preview exceeds 64 levels or 10000 nodes"
    );
    let description = match value {
        serde_json::Value::Array(v) => format!("[{} items]", v.len()),
        serde_json::Value::Object(v) => format!("{{{} fields}}", v.len()),
        serde_json::Value::String(s) => s.clone(),
        _ => value.to_string(),
    };
    rows.push(Row {
        cells: vec![key.into(), description],
        depth,
        branch: value.is_array() || value.is_object(),
        ..Row::default()
    });
    match value {
        serde_json::Value::Array(v) => {
            for (i, v) in v.iter().enumerate() {
                flatten(&i.to_string(), v, depth + 1, rows, cancel)?;
            }
        }
        serde_json::Value::Object(v) => {
            for (k, v) in v {
                flatten(k, v, depth + 1, rows, cancel)?;
            }
        }
        _ => {}
    }
    Ok(())
}
