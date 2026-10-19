//! Bounded, syntax-preserving structured code previews.
use skwir_core::{
    App, CancelToken,
    inspection::{Block, InspectRequest, Inspection, InspectionKind},
    plugin::Plugin,
    preview::{PreviewProvider, PreviewRequest},
};
use skwir_platform::inspection::{self, limits, limits::Bounds};
use std::sync::Arc;
pub struct PreviewPlugin;
impl Plugin for PreviewPlugin {
    fn id(&self) -> &'static str {
        "structured-data"
    }
    fn title(&self) -> &'static str {
        "Structured data"
    }
    fn description(&self) -> &'static str {
        "Show JSON (with comments, and JSON5), YAML and TOML with syntax colors and foldable blocks"
    }
    fn category(&self) -> skwir_core::plugin::Category {
        skwir_core::plugin::Category::Previews
    }
    fn capabilities(&self) -> skwir_core::permissions::Capabilities {
        use skwir_core::permissions::Capability;
        skwir_core::permissions::Capabilities::of(&[Capability::Read])
    }
    fn register(&self, app: &App) {
        app.previews.register(self.id(), Arc::new(Provider));
    }
}
struct Provider;
impl PreviewProvider for Provider {
    fn id(&self) -> &'static str {
        "structured-data"
    }
    fn accepts(&self, req: &PreviewRequest) -> bool {
        matches!(
            inspection::extension(&req.entry.path).as_str(),
            "json" | "jsonc" | "json5" | "yaml" | "yml" | "toml"
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
        // A file that cannot be read whole or parsed fails: the panel shows
        // its source instead, saying why (`preview::source_instead`), and
        // offers Full preview when only its size kept it from being shown.
        let bounds = Bounds::of(view);
        let (text, encoding) = inspection::read_text(&path, bounds, cancel)?;
        let mut data = code(&text, &language, view.source, bounds, cancel)?;
        data.notice = encoding.notice().unwrap_or_default();
        Ok(Some(data))
    }
}

mod folds;

fn code(
    source: &str,
    language: &str,
    original: bool,
    bounds: Bounds,
    cancel: &CancelToken,
) -> anyhow::Result<Inspection> {
    inspection::check(cancel)?;
    // JSON is checked by parsing it, and printed from the value unless its
    // source was asked for. JSON as editors write it (`tsconfig.json`,
    // settings) has comments and trailing commas, which printing would
    // lose: like JSONC, it is shown as written once it parses without them
    // (blanked in place, so an error still points at its line). JSON5
    // (unquoted keys, single quotes) is shown as written unchecked: its
    // folds come from the same lexer, which steps over its strings and
    // comments alike. An empty file has nothing to check.
    let parsed = match language {
        "json" | "jsonc" if !source.trim().is_empty() => {
            let value = serde_json::from_str::<serde_json::Value>(source).ok();
            if value.is_none() {
                serde_json::from_str::<serde_json::Value>(&folds::without_comments(source))?;
            }
            value
        }
        _ => None,
    };
    // Only JSON is printed again (indented, its keys in order): elsewhere
    // the file is shown as written, and its source would be the same.
    let printed = language == "json" && parsed.is_some();
    let text = if let Some(value) = parsed.filter(|_| printed && !original) {
        let mut pending = vec![(&value, 0)];
        let mut count = 0;
        while let Some((node, depth)) = pending.pop() {
            inspection::check(cancel)?;
            count += 1;
            if count > bounds.code_nodes() {
                return Err(bounds.refuse(format!(
                    "Code preview exceeds {} nodes",
                    bounds.code_nodes()
                )));
            }
            anyhow::ensure!(
                depth <= limits::NESTING,
                "Code preview exceeds {} levels",
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
        source.to_owned()
    };
    if text.len() > bounds.text_input() {
        return Err(bounds.refuse(format!(
            "Formatted preview exceeds {}",
            limits::describe(bounds.text_input())
        )));
    }
    let lines: Vec<_> = text.split('\n').collect();
    if lines.len() > bounds.code_lines() {
        return Err(bounds.refuse(format!(
            "Code preview exceeds {} lines",
            bounds.code_lines()
        )));
    }
    anyhow::ensure!(
        lines.iter().all(|line| line.len() <= limits::LINE_BYTES),
        "Code preview exceeds {} bytes per line",
        limits::LINE_BYTES
    );
    let folds = folds::parse(&text, language, cancel)?;
    let blocks = lines
        .into_iter()
        .map(|line| Block {
            text: line.trim_end_matches('\r').into(),
            code: true,
            // bat's JSON grammar colours comments too.
            language: Some(
                if language.starts_with("json") {
                    "json"
                } else {
                    language
                }
                .into(),
            ),
            ..Block::default()
        })
        .collect();
    Ok(Inspection {
        kind: InspectionKind::Code,
        title: if printed && !original {
            format!("{} · formatted", language.to_ascii_uppercase())
        } else {
            language.to_ascii_uppercase()
        },
        blocks,
        folds,
        source_available: printed,
        ..Inspection::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_formats_without_folding_delimiters_in_strings() {
        let source = r#"{"title":"café [braces] and \"quote\"","items":[{"enabled":true},2]}"#;
        let data = code(source, "json", false, Bounds::QUICK, &CancelToken::new()).unwrap();
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
        assert_eq!(data.title, "JSON · formatted");
        let raw = code(source, "json", true, Bounds::QUICK, &CancelToken::new()).unwrap();
        assert_eq!(raw.plain_text(), source);
        assert!(raw.folds.is_empty());
        assert_eq!(raw.title, "JSON");
        assert!(data.source_available && raw.source_available);
    }

    #[test]
    fn json_with_comments_is_shown_as_written_and_folded() {
        // As `tsconfig.json` is written: comments, a trailing comma, and
        // brackets in a comment and in a string that fold nothing.
        let source = "{\n  // paths [not a fold\n  \"paths\": {\n    \"@/*\": [\"src/*\"],\n  },\n  /* { */ \"strict\": true,\n}\n";
        for language in ["json", "jsonc"] {
            let data = code(source, language, false, Bounds::QUICK, &CancelToken::new()).unwrap();
            assert_eq!(data.plain_text(), source, "{language} keeps its comments");
            assert_eq!(data.folds, [0..6, 2..4]);
            assert_eq!(data.blocks[0].language.as_deref(), Some("json"));
            assert!(!data.source_available, "its source is what is shown");
        }
        let error = code(
            "{ \"a\": // no value\n }",
            "jsonc",
            false,
            Bounds::QUICK,
            &CancelToken::new(),
        )
        .unwrap_err();
        assert!(error.to_string().contains("line 2"), "{error}");
    }

    #[test]
    fn json5_folds_step_over_single_quoted_strings() {
        let source = "{\n  unquoted: 'a ] and a {',\n  list: [\n    0x10,\n  ],\n}\n";
        let data = code(source, "json5", false, Bounds::QUICK, &CancelToken::new()).unwrap();
        assert_eq!(data.plain_text(), source);
        assert_eq!(data.folds, [0..5, 2..4]);
    }

    #[test]
    fn comments_and_trailing_commas_are_blanked_in_place() {
        let blanked = folds::without_comments("[1, \"x\", \"//\", /* c */ 2, // d\n]");
        assert_eq!(blanked, "[1, \"x\", \"//\",         2      \n]");
        assert_eq!(folds::without_comments("{\"a\": [1,],}"), "{\"a\": [1 ] }");
        assert_eq!(folds::without_comments("1 / 2"), "1 / 2");
    }

    #[test]
    fn toml_keeps_comments_order_and_multiline_strings() {
        let source = "# Example\n[\"first.section\"]\ntext = \"\"\"\n[not.a.table]\n# not a comment\n\"\"\"\nitems = [\n  1,\n  2,\n]\n\n[[second]]\nname = 'café'\n[[second]]\nname = 'next'\n";
        let data = code(source, "toml", false, Bounds::QUICK, &CancelToken::new()).unwrap();
        assert_eq!(data.plain_text(), source);
        assert_eq!(data.folds, [1..11, 2..5, 6..9, 11..13, 13..16]);
        assert_eq!(data.title, "TOML");
        assert!(
            !data.source_available,
            "shown as written: no source to switch to"
        );
    }

    #[test]
    fn yaml_folds_collections_without_mistaking_scalar_contents() {
        let source = "group:\n  title: 'café: [example]'\n  items:\n    - one\n    - two\nother:\n  text: |\n    fake:\n      - value\n  enabled: true\n";
        let data = code(source, "yaml", false, Bounds::QUICK, &CancelToken::new()).unwrap();
        assert_eq!(data.plain_text(), source);
        assert_eq!(data.folds, [0..5, 2..5, 5..10]);
    }

    fn inspect(path: &std::path::Path, source: bool) -> Inspection {
        let request = PreviewRequest {
            entry: skwir_core::op::EntryRef::from_path(path).unwrap(),
            mime: Some("application/json".into()),
            page: 1,
            seconds: None,
            max_side: 64,
            text_size: None,
            fit: None,
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
        assert!(code("{broken", "json", false, Bounds::QUICK, &cancel).is_err());
        assert!(
            code(
                &format!("name = '{}'", "a".repeat(4096)),
                "toml",
                false,
                Bounds::QUICK,
                &cancel
            )
            .is_err()
        );
        // Long: refused while quick, for Full preview to show; a line too
        // long is refused either way.
        let long = "\n".repeat(10001);
        let error = code(&long, "yaml", false, Bounds::QUICK, &cancel).unwrap_err();
        assert!(skwir_core::inspection::TooLarge::is(&error), "{error}");
        let all = code(&long, "yaml", false, Bounds::WHOLE, &cancel).unwrap();
        assert_eq!(all.blocks.len(), 10002);
        let wide = format!("name = '{}'", "a".repeat(4096));
        let error = code(&wide, "toml", false, Bounds::WHOLE, &cancel).unwrap_err();
        assert!(!skwir_core::inspection::TooLarge::is(&error), "{error}");
        cancel.cancel();
        assert!(code("{}", "json", false, Bounds::QUICK, &cancel).is_err());
    }
}
