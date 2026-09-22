//! Bounded, syntax-preserving structured code previews.
use bibli_core::{
    App, CancelToken,
    inspection::{Block, InspectRequest, Inspection, InspectionKind},
    plugin::Plugin,
    preview::{PreviewProvider, PreviewRequest},
};
use bibli_platform::inspection::{self, limits};
use std::sync::Arc;
pub struct PreviewPlugin;
impl Plugin for PreviewPlugin {
    fn id(&self) -> &'static str {
        "preview-structured"
    }
    fn title(&self) -> &'static str {
        "Preview: structured data"
    }
    fn description(&self) -> &'static str {
        "Show JSON, YAML and TOML with syntax colors and foldable blocks"
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
        let language = inspection::extension(&path);
        let result = inspection::read_text(&path, cancel).and_then(|(text, encoding)| {
            let mut data = code(&text, &language, view.source, cancel)?;
            data.notice = encoding.notice().unwrap_or_default();
            Ok(data)
        });
        match result {
            Ok(data) => Ok(Some(data)),
            // The source view takes its language from the name, like any
            // text file, and may hold no line at all (an empty file).
            Err(error) if !cancel.is_cancelled() => {
                let mut source = inspection::source(&path, view.offset, cancel)?;
                source.notice = format!("Showing source: {error}");
                Ok(Some(source))
            }
            Err(error) => Err(error),
        }
    }
}

mod folds;

fn code(
    source: &str,
    language: &str,
    original: bool,
    cancel: &CancelToken,
) -> anyhow::Result<Inspection> {
    inspection::check(cancel)?;
    // An empty file has nothing to format: it is shown as it is.
    let text = if language == "json" && !original && !source.trim().is_empty() {
        let value: serde_json::Value = serde_json::from_str(source)?;
        let mut pending = vec![(&value, 0)];
        let mut count = 0;
        while let Some((node, depth)) = pending.pop() {
            inspection::check(cancel)?;
            count += 1;
            anyhow::ensure!(
                count <= limits::CODE_NODES && depth <= limits::NESTING,
                "Code preview exceeds {} nodes or {} levels",
                limits::CODE_NODES,
                limits::NESTING
            );
            match node {
                serde_json::Value::Array(items) => {
                    pending.extend(items.iter().map(|v| (v, depth + 1)));
                }
                serde_json::Value::Object(items) => {
                    pending.extend(items.values().map(|v| (v, depth + 1)));
                }
                _ => {}
            }
        }
        serde_json::to_string_pretty(&value)?
    } else {
        if language == "json" && !source.trim().is_empty() {
            // Folds are read lexically from text known to be JSON.
            serde_json::from_str::<serde_json::Value>(source)?;
        }
        source.to_owned()
    };
    anyhow::ensure!(
        text.len() <= limits::TEXT_INPUT,
        "Formatted preview exceeds {}",
        limits::describe(limits::TEXT_INPUT)
    );
    let lines: Vec<_> = text.split('\n').collect();
    anyhow::ensure!(
        lines.len() <= limits::CODE_LINES
            && lines.iter().all(|line| line.len() <= limits::LINE_BYTES),
        "Code preview exceeds {} lines or {} bytes per line",
        limits::CODE_LINES,
        limits::LINE_BYTES
    );
    let folds = folds::parse(&text, language, cancel)?;
    let blocks = lines
        .into_iter()
        .map(|line| Block {
            text: line.trim_end_matches('\r').into(),
            code: true,
            language: Some(language.into()),
            ..Block::default()
        })
        .collect();
    Ok(Inspection {
        kind: InspectionKind::Code,
        title: format!(
            "{} · {}",
            language.to_ascii_uppercase(),
            if original { "Source" } else { "Code" }
        ),
        blocks,
        folds,
        source_available: true,
        ..Inspection::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_formats_without_folding_delimiters_in_strings() {
        let source = r#"{"title":"café [braces] and \"quote\"","items":[{"enabled":true},2]}"#;
        let data = code(source, "json", false, &CancelToken::new()).unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&data.plain_text()).unwrap(),
            serde_json::from_str::<serde_json::Value>(source).unwrap()
        );
        assert!(
            data.blocks
                .iter()
                .any(|b| b.text == "      \"enabled\": true")
        );
        assert_eq!(data.folds.len(), 3);
        assert_eq!(data.folds[0], 0..data.blocks.len() - 1);
        let raw = code(source, "json", true, &CancelToken::new()).unwrap();
        assert_eq!(raw.plain_text(), source);
        assert!(raw.folds.is_empty());
    }

    #[test]
    fn toml_keeps_comments_order_and_multiline_strings() {
        let source = "# Example\n[\"first.section\"]\ntext = \"\"\"\n[not.a.table]\n# not a comment\n\"\"\"\nitems = [\n  1,\n  2,\n]\n\n[[second]]\nname = 'café'\n[[second]]\nname = 'next'\n";
        let data = code(source, "toml", false, &CancelToken::new()).unwrap();
        assert_eq!(data.plain_text(), source);
        assert_eq!(data.folds, [1..11, 2..5, 6..9, 11..13, 13..16]);
    }

    #[test]
    fn yaml_folds_collections_without_mistaking_scalar_contents() {
        let source = "group:\n  title: 'café: [example]'\n  items:\n    - one\n    - two\nother:\n  text: |\n    fake:\n      - value\n  enabled: true\n";
        let data = code(source, "yaml", false, &CancelToken::new()).unwrap();
        assert_eq!(data.plain_text(), source);
        assert_eq!(data.folds, [0..5, 2..5, 5..10]);
    }

    fn inspect(path: &std::path::Path, source: bool) -> Inspection {
        let request = PreviewRequest {
            entry: bibli_core::op::EntryRef::from_path(path).unwrap(),
            mime: Some("application/json".into()),
            page: 1,
            seconds: None,
            max_side: 64,
        };
        let view = InspectRequest {
            source,
            ..InspectRequest::default()
        };
        Provider
            .inspect(&request, &view, &CancelToken::new())
            .unwrap()
            .unwrap()
    }

    #[test]
    fn an_empty_or_marked_file_is_read_not_refused() {
        let dir = tempfile::tempdir().unwrap();
        let empty = dir.path().join("empty.json");
        std::fs::write(&empty, b"").unwrap();
        for source in [false, true] {
            let data = inspect(&empty, source);
            assert_eq!(data.kind, InspectionKind::Code);
            assert!(data.plain_text().is_empty());
            assert!(data.notice.is_empty(), "{}", data.notice);
        }
        // A byte order mark is not JSON's business: the value still folds.
        let marked = dir.path().join("it's marked.json");
        std::fs::write(&marked, "\u{feff}{\"a\": [1, 2]}".as_bytes()).unwrap();
        let data = inspect(&marked, false);
        assert!(data.notice.is_empty(), "{}", data.notice);
        assert!(!data.folds.is_empty());
        assert!(!data.plain_text().contains('\u{feff}'));
        // A legacy encoding is read, and said.
        let legacy = dir.path().join("legacy.yaml");
        std::fs::write(&legacy, b"name: caf\xe9\n").unwrap();
        let data = inspect(&legacy, false);
        assert_eq!(data.plain_text(), "name: caf\u{e9}\n");
        assert_eq!(data.notice, "Read as Windows-1252");
    }

    #[test]
    fn invalid_large_and_cancelled_documents_are_bounded() {
        let cancel = CancelToken::new();
        assert!(code("{broken", "json", false, &cancel).is_err());
        assert!(
            code(
                &format!("name = '{}'", "a".repeat(4096)),
                "toml",
                false,
                &cancel
            )
            .is_err()
        );
        assert!(code(&"\n".repeat(10001), "yaml", false, &cancel).is_err());
        cancel.cancel();
        assert!(code("{}", "json", false, &cancel).is_err());
    }
}
