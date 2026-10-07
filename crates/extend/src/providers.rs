//! A plugin from outside's fields, thumbnails and previews, as the core's
//! providers: each asks the plugin on the worker the core runs it on, for
//! one file, never a private one.

use std::ffi::OsStr;
use std::path::Path;
use std::sync::{Arc, Weak};

use bibli_core::fields::FieldProvider;
use bibli_core::inspection::{self, InspectRequest};
use bibli_core::preview::{PreviewProvider, PreviewRequest};
use bibli_core::thumbs::{Image, ThumbRequest, ThumbnailProvider};
use bibli_core::{App, CancelToken, Value};
use bibli_platform::mime::{self, Types};
use bibli_plugin_api::protocol::{
    self, FieldAnswer, FieldParams, InspectParams, ThumbnailAnswer, ThumbnailParams, method,
};

use crate::call::Call;
use crate::external::{External, Runner};

/// Register what `plugin` provides.
pub(crate) fn register(app: &App, plugin: &External) {
    let manifest = plugin.manifest();
    let owner = plugin.runner.id;
    if !manifest.fields.is_empty() {
        let fields: Vec<(&'static str, Option<&'static str>, Types)> = manifest
            .fields
            .iter()
            .map(|field| {
                (
                    crate::intern(&field.key),
                    field.label.as_deref().map(crate::intern),
                    types(&field.types),
                )
            })
            .collect();
        let keys: &'static [&'static str] =
            Box::leak(fields.iter().map(|(key, _, _)| *key).collect());
        app.fields.register(
            owner,
            Arc::new(Fields {
                runner: Arc::clone(&plugin.runner),
                app: app.weak(),
                keys,
                declared: fields,
            }),
        );
    }
    for thumbnails in &manifest.thumbnails {
        app.thumbnails.register(
            owner,
            Arc::new(Thumbnails {
                runner: Arc::clone(&plugin.runner),
                app: app.weak(),
                types: types(&thumbnails.types),
            }),
        );
    }
    for preview in &manifest.previews {
        app.previews.register(
            owner,
            Arc::new(Previews {
                runner: Arc::clone(&plugin.runner),
                app: app.weak(),
                types: types(&preview.types),
                priority: preview.priority,
                shows_bytes: preview.shows_bytes,
            }),
        );
    }
}

/// The types a manifest names, matched as a provider's are: by name,
/// through the database's aliases, without case.
fn types(names: &[String]) -> Types {
    let names: Vec<&'static str> = names.iter().map(|name| crate::intern(name)).collect();
    Types::new(Box::leak(names.into_boxed_slice()))
}

/// Whether `path` is private: a plugin from outside never sees it.
fn private(app: &Weak<App>, path: &Path) -> bool {
    app.upgrade().is_none_or(|app| app.privacy().holds(path))
}

/// The words a preview says for a private file.
const PRIVATE: &str =
    "This file is private: a plugin from outside bibli does not read it (Settings, Privacy)";

struct Fields {
    runner: Arc<Runner>,
    app: Weak<App>,
    keys: &'static [&'static str],
    declared: Vec<(&'static str, Option<&'static str>, Types)>,
}

impl FieldProvider for Fields {
    fn id(&self) -> &'static str {
        self.runner.id
    }
    fn keys(&self) -> &'static [&'static str] {
        self.keys
    }
    fn label(&self, key: &str) -> Option<&'static str> {
        self.declared
            .iter()
            .find(|(k, _, _)| *k == key)
            .and_then(|(_, label, _)| *label)
    }
    fn accepts(&self, key: &str, name: &OsStr) -> bool {
        let Some(mime) = mime::by_name(name) else {
            return false;
        };
        self.declared
            .iter()
            .any(|(k, _, types)| *k == key && types.contains(&mime.0))
    }
    fn read(&self, key: &str, path: &Path, cancel: &CancelToken) -> anyhow::Result<Option<Value>> {
        if private(&self.app, path) {
            return Ok(None);
        }
        let call = Call::new(vec![path.to_owned()], self.runner.granted, cancel.clone());
        let mime = path.file_name().and_then(mime::by_name).map(|m| m.0);
        let params = FieldParams {
            key: key.to_owned(),
            file: call.describe(0, mime),
        };
        let answer = self
            .runner
            .call(method::FIELD, serde_json::to_value(params)?, &call)?;
        let answer: FieldAnswer = serde_json::from_value(answer)?;
        Ok(answer.value.as_ref().and_then(value))
    }
}

/// A field's JSON as the core's value: a list's items each so.
fn value(json: &serde_json::Value) -> Option<Value> {
    Some(match json {
        serde_json::Value::Bool(b) => Value::Bool(*b),
        serde_json::Value::Number(n) => match (n.as_u64(), n.as_i64(), n.as_f64()) {
            (Some(u), _, _) => Value::UInt(u),
            (None, Some(i), _) => Value::Int(i),
            (None, None, Some(f)) => Value::Float(f),
            _ => return None,
        },
        serde_json::Value::String(s) => Value::Text(s.as_str().into()),
        serde_json::Value::Array(items) => Value::List(items.iter().filter_map(value).collect()),
        serde_json::Value::Null | serde_json::Value::Object(_) => return None,
    })
}

struct Thumbnails {
    runner: Arc<Runner>,
    app: Weak<App>,
    types: Types,
}

impl ThumbnailProvider for Thumbnails {
    fn id(&self) -> &'static str {
        self.runner.id
    }
    fn accepts(&self, req: &ThumbRequest) -> bool {
        req.mime
            .as_deref()
            .is_some_and(|mime| self.types.contains(mime))
    }
    fn thumbnail(&self, req: &ThumbRequest, cancel: &CancelToken) -> anyhow::Result<Option<Image>> {
        let path = req.entry.resolve()?;
        if private(&self.app, &path) {
            return Ok(None);
        }
        let call = Call::new(vec![path], self.runner.granted, cancel.clone());
        let side = req.size.pixels();
        let params = ThumbnailParams {
            file: call.describe(0, req.mime.as_deref().map(str::to_owned)),
            size: side,
        };
        let answer = self
            .runner
            .call(method::THUMBNAIL, serde_json::to_value(params)?, &call)?;
        let answer: ThumbnailAnswer = serde_json::from_value(answer)?;
        let Some(png) = answer.png else {
            return Ok(None);
        };
        let png = bibli_plugin_api::base64::decode(&png)
            .ok_or_else(|| anyhow::anyhow!("a thumbnail that is no base64"))?;
        let decoded = bibli_platform::inspection::picture(&png, side)?;
        Ok(Some(Image {
            width: decoded.width,
            height: decoded.height,
            rgba: decoded.rgba.into(),
        }))
    }
}

struct Previews {
    runner: Arc<Runner>,
    app: Weak<App>,
    types: Types,
    priority: i32,
    shows_bytes: bool,
}

impl PreviewProvider for Previews {
    fn id(&self) -> &'static str {
        self.runner.id
    }
    fn priority(&self) -> i32 {
        self.priority
    }
    fn shows_bytes(&self) -> bool {
        self.shows_bytes
    }
    fn accepts(&self, request: &PreviewRequest) -> bool {
        request
            .mime
            .as_deref()
            .is_some_and(|mime| self.types.contains(mime))
    }
    fn inspect(
        &self,
        request: &PreviewRequest,
        view: &InspectRequest,
        cancel: &CancelToken,
    ) -> anyhow::Result<Option<inspection::Inspection>> {
        let path = request.entry.resolve()?;
        anyhow::ensure!(!private(&self.app, &path), PRIVATE);
        let call = Call::new(vec![path], self.runner.granted, cancel.clone());
        let params = InspectParams {
            file: call.describe(0, request.mime.clone()),
            offset: view.offset,
            query: view.query.clone(),
            source: view.source,
            target: view.target.clone(),
        };
        let answer = self
            .runner
            .call(method::INSPECT, serde_json::to_value(params)?, &call)?;
        let answer: Option<protocol::Inspection> = serde_json::from_value(answer)?;
        Ok(answer.map(inspection_of))
    }
}

/// What a plugin's preview shows, as the core draws it.
pub(crate) fn inspection_of(answer: protocol::Inspection) -> inspection::Inspection {
    use bibli_plugin_api::protocol::InspectionKind as Kind;
    let code = answer.kind == Kind::Code;
    inspection::Inspection {
        kind: match answer.kind {
            Kind::Text => inspection::InspectionKind::Text,
            Kind::Code => inspection::InspectionKind::Code,
            Kind::Hex => inspection::InspectionKind::Hex,
            Kind::Table => inspection::InspectionKind::Table,
        },
        title: answer.title,
        blocks: inspection::split_blocks(
            answer
                .text
                .into_iter()
                .map(|text| inspection::Block {
                    text,
                    code,
                    ..inspection::Block::default()
                })
                .collect(),
        ),
        columns: answer.columns,
        rows: answer
            .rows
            .into_iter()
            .map(|row| inspection::Row {
                cells: row.cells,
                bytes: row.bytes,
                depth: row.depth,
                branch: row.branch,
                target: row.target,
            })
            .collect(),
        previous: answer.previous,
        next: answer.next,
        position: answer.position,
        notice: answer.notice,
        details: answer
            .details
            .into_iter()
            .map(|detail| inspection::Detail {
                label: detail.label,
                value: detail.value,
                action: None,
            })
            .collect(),
        ..inspection::Inspection::default()
    }
}
