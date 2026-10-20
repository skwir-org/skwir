//! A plugin from outside's fields, thumbnails and previews, as the core's
//! providers: each asks the plugin on the worker the core runs it on, for
//! one file, never a private one.

use std::ffi::OsStr;
use std::path::Path;
use std::sync::{Arc, Weak};

use skwir_core::cache::CacheSlot;
use skwir_core::document::DocumentSession;
use skwir_core::fields::FieldProvider;
use skwir_core::inspection::{self, InspectRequest};
use skwir_core::preview::{Converted, Preview, PreviewProvider, PreviewRequest};
use skwir_core::thumbs::{Image, ThumbRequest, ThumbnailProvider};
use skwir_core::{App, CancelToken, Value};
use skwir_platform::mime::{self, Types};
use skwir_plugin_api::protocol::{
    self, ConvertAnswer, ConvertParams, FieldAnswer, FieldParams, InspectParams, ThumbnailAnswer,
    ThumbnailParams, method,
};

use crate::call::Call;
use crate::external::{External, Runner};
use crate::pages::Pages;

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
                converted: preview
                    .converts
                    .then(|| app.cache.namespace(owner).folder("converted")),
                pages: preview
                    .pages
                    .then(|| app.cache.namespace(owner).within("pages")),
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
    "This file is private: a plugin from outside skwir does not read it (Settings, Privacy)";

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
        // The plugin answers one call at a time: a thumbnail asked while it
        // works is set aside, holding no worker, and asked again once the
        // turn is free.
        let Some(turn) = self.runner.turn.try_take() else {
            let runner = Arc::clone(&self.runner);
            return Err(
                skwir_core::thumbs::Busy::new(move |then| runner.turn.when_free(then)).into(),
            );
        };
        let call = Call::new(vec![path], self.runner.granted, cancel.clone());
        let side = req.size.pixels();
        let params = ThumbnailParams {
            file: call.describe(0, req.mime.as_deref().map(str::to_owned)),
            size: side,
        };
        let answer = self.runner.call_in(
            &turn,
            method::THUMBNAIL,
            serde_json::to_value(params)?,
            &call,
        )?;
        let answer: ThumbnailAnswer = serde_json::from_value(answer)?;
        let Some(png) = answer.png else {
            return Ok(None);
        };
        let png = skwir_plugin_api::base64::decode(&png)
            .ok_or_else(|| anyhow::anyhow!("a thumbnail that is no base64"))?;
        let decoded = skwir_platform::inspection::picture(&png, side)?;
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
    /// Where the files it converts to are kept, when it converts: the
    /// plugin's folder of the cache.
    converted: Option<std::path::PathBuf>,
    /// Where the pages it drew are kept, when it shows files a page at a
    /// time.
    pages: Option<CacheSlot>,
}

/// What the files a plugin converted to may take of the cache: the oldest
/// go first.
const CONVERTED_BUDGET: u64 = 256 * 1024 * 1024;

/// What a folder of converted files holds when the plugin made nothing of
/// that version of the file.
const NOTHING: &str = ".nothing";

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
    /// Reached when the plugin answered nothing (no page, no file): the
    /// file is its own, which it shows nothing of.
    fn render(&self, _request: &PreviewRequest, _cancel: &CancelToken) -> anyhow::Result<Preview> {
        anyhow::bail!("“{}” shows nothing of this file", self.runner.title)
    }
    /// The file a page at a time, when the plugin shows it so (ADR-047):
    /// read by skwir's reader, which turns, selects and searches it.
    fn open_document(
        &self,
        request: &PreviewRequest,
        lifetime: &CancelToken,
    ) -> anyhow::Result<Option<Arc<dyn DocumentSession>>> {
        let Some(kept) = &self.pages else {
            return Ok(None);
        };
        let path = request.entry.resolve()?;
        anyhow::ensure!(!private(&self.app, &path), PRIVATE);
        let pages = Pages::open(
            Arc::clone(&self.runner),
            kept.clone(),
            path,
            request.entry.id,
            request.mime.clone(),
            lifetime,
        )?;
        Ok(pages.map(|pages| Arc::new(pages) as Arc<dyn DocumentSession>))
    }
    fn inspect(
        &self,
        request: &PreviewRequest,
        view: &InspectRequest,
        cancel: &CancelToken,
    ) -> anyhow::Result<Option<inspection::Inspection>> {
        // A preview that converts or shows pages answers those, not this.
        if self.converted.is_some() || self.pages.is_some() {
            return Ok(None);
        }
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
    /// The file it converts this one to (ADR-047): kept in the plugin's
    /// folder of the cache under the file's version and the plugin's, so
    /// a page turned, the file shown again or skwir started again asks the
    /// plugin nothing.
    fn converted(
        &self,
        request: &PreviewRequest,
        cancel: &CancelToken,
    ) -> anyhow::Result<Option<Converted>> {
        let Some(folder) = &self.converted else {
            return Ok(None);
        };
        let path = request.entry.resolve()?;
        anyhow::ensure!(!private(&self.app, &path), PRIVATE);
        let key = skwir_platform::preview::made_key(
            request.entry.id,
            &path,
            self.runner.made_by(),
            "converted",
        )?;
        let kept = folder.join(blake3::hash(&key).to_hex().as_str());
        // It made nothing of this version of the file (it shows it a page
        // at a time instead): it is not asked again.
        if kept.join(NOTHING).exists() {
            return Ok(None);
        }
        if let Some(made) = only_file(&kept) {
            return Ok(Some(self.shown(made)));
        }
        let call = Call::new(vec![path], self.runner.granted, cancel.clone());
        let params = ConvertParams {
            file: call.describe(0, request.mime.clone()),
        };
        let answer = self
            .runner
            .call(method::CONVERT, serde_json::to_value(params)?, &call)?;
        let Some(answer): Option<ConvertAnswer> = serde_json::from_value(answer)? else {
            // Remembered: a page turned asks the plugin nothing.
            let _ = std::fs::create_dir_all(&kept)
                .and_then(|()| std::fs::write(kept.join(NOTHING), b""));
            return Ok(None);
        };
        let bytes = skwir_plugin_api::base64::decode(&answer.data)
            .ok_or_else(|| anyhow::anyhow!("the plugin's file is no base64"))?;
        // Its name is a name: what it says of folders is dropped.
        let name = Path::new(&answer.name)
            .file_name()
            .filter(|name| !name.is_empty())
            .map_or_else(|| "converted".into(), std::ffi::OsStr::to_os_string);
        let made = keep(folder, &kept, &name, &bytes)?;
        Ok(Some(self.shown(made)))
    }
}

impl Previews {
    /// `made` as the panel shows it: typed by its name and bytes, its
    /// links closed unless the plugin may use the network anyway.
    fn shown(&self, made: std::path::PathBuf) -> Converted {
        let mime = made
            .file_name()
            .and_then(|name| mime::identify(name, &made))
            .map(|mime| mime.to_string());
        let closed_links = (!self
            .runner
            .granted
            .has(skwir_core::permissions::Capability::Network))
        .then(|| {
            format!(
                "Links are not followed in a preview “{}” made: it may not use the network",
                self.runner.title
            )
        });
        Converted {
            path: made,
            mime,
            closed_links,
        }
    }
}

/// The one file of `folder`, if it holds one.
fn only_file(folder: &Path) -> Option<std::path::PathBuf> {
    let mut files = std::fs::read_dir(folder).ok()?.flatten();
    let file = files.next()?.path();
    (files.next().is_none() && file.is_file()).then_some(file)
}

/// Keep `bytes` as `name` in `kept`, a folder of `folder` written whole
/// under another name first, so a preview never reads one half written;
/// then the oldest of `folder` go, beyond [`CONVERTED_BUDGET`].
fn keep(
    folder: &Path,
    kept: &Path,
    name: &OsStr,
    bytes: &[u8],
) -> anyhow::Result<std::path::PathBuf> {
    std::fs::create_dir_all(folder)?;
    let writing = tempfile::Builder::new()
        .prefix(".writing-")
        .tempdir_in(folder)?;
    std::fs::write(writing.path().join(name), bytes)?;
    match std::fs::rename(writing.path(), kept) {
        Ok(()) => std::mem::forget(writing),
        // Another preview of the same file kept it first.
        Err(_) if only_file(kept).is_some() => {}
        Err(error) => return Err(error.into()),
    }
    trim(folder, kept);
    only_file(kept).ok_or_else(|| anyhow::anyhow!("the converted file is gone"))
}

/// The oldest folders of `folder` removed until what it holds fits
/// [`CONVERTED_BUDGET`], never `kept`. A file being shown stays readable
/// where it is opened; one shown again is made again.
fn trim(folder: &Path, kept: &Path) {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return;
    };
    let mut made: Vec<(std::time::SystemTime, u64, std::path::PathBuf)> = entries
        .flatten()
        .filter(|entry| entry.path() != kept)
        .filter_map(|entry| {
            let file = only_file(&entry.path())?;
            let meta = file.metadata().ok()?;
            Some((meta.modified().ok()?, meta.len(), entry.path()))
        })
        .collect();
    let kept_size = only_file(kept)
        .and_then(|file| file.metadata().ok())
        .map_or(0, |meta| meta.len());
    let mut total: u64 = kept_size + made.iter().map(|(_, size, _)| size).sum::<u64>();
    made.sort();
    for (_, size, path) in made {
        if total <= CONVERTED_BUDGET {
            break;
        }
        if std::fs::remove_dir_all(&path).is_ok() {
            total -= size;
        }
    }
}

/// What a plugin's preview shows, as the core draws it.
pub(crate) fn inspection_of(answer: protocol::Inspection) -> inspection::Inspection {
    use skwir_plugin_api::protocol::InspectionKind as Kind;
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
                section: None,
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A plugin that draws a thumbnail of one pixel, counting its calls.
    struct Pixel(std::sync::atomic::AtomicUsize);

    /// A PNG of one transparent pixel.
    const PIXEL: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x01\0\0\0\x01\x08\x06\0\0\0\x1f\x15\xc4\x89\0\0\0\nIDATx\x9cc\0\x01\0\0\x05\0\x01\r\n-\xb4\0\0\0\0IEND\xaeB`\x82";

    impl crate::call::Transport for Pixel {
        fn call(
            &self,
            method: &str,
            _: serde_json::Value,
            _: &Call,
        ) -> anyhow::Result<serde_json::Value> {
            assert_eq!(method, method::THUMBNAIL);
            self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            Ok(serde_json::to_value(ThumbnailAnswer {
                png: Some(skwir_plugin_api::base64::encode(PIXEL)),
            })?)
        }
        fn stop(&self) {}
    }

    /// The plugin answers one call at a time: a thumbnail asked while it
    /// works is set aside (`Busy`), holding no worker, and drawn once the
    /// turn is free.
    #[test]
    fn a_thumbnail_asked_while_its_plugin_works_is_set_aside() {
        let tmp = tempfile::tempdir().unwrap();
        let app = App::new(skwir_core::app::AppConfig::ephemeral(
            tmp.path().join("state"),
        ))
        .unwrap();
        let file = tmp.path().join("it's a picture.ppm");
        std::fs::write(&file, "P1 1 1 0").unwrap();
        let module = tmp.path().join("plugin.wasm");
        std::fs::write(&module, "module").unwrap();
        let pixel = Arc::new(Pixel(std::sync::atomic::AtomicUsize::new(0)));
        let runner = Arc::new(Runner::answered_by(&module, pixel.clone()));
        let thumbnails = Thumbnails {
            runner: Arc::clone(&runner),
            app: Arc::downgrade(&app),
            types: types(&["image/x-portable-pixmap".to_owned()]),
        };
        let req = ThumbRequest {
            entry: skwir_core::op::EntryRef::from_path(&file).unwrap(),
            mime: Some("image/x-portable-pixmap".into()),
            mtime: None,
            size: skwir_core::thumbs::ThumbSize::Normal,
        };
        let cancel = CancelToken::new();
        let working = runner.turn.try_take().unwrap();
        let error = thumbnails.thumbnail(&req, &cancel).unwrap_err();
        assert!(error.is::<skwir_core::thumbs::Busy>(), "{error:#}");
        assert_eq!(pixel.0.load(std::sync::atomic::Ordering::Relaxed), 0);
        drop(working);
        let drawn = thumbnails.thumbnail(&req, &cancel).unwrap().unwrap();
        assert_eq!((drawn.width, drawn.height), (1, 1));
        assert_eq!(pixel.0.load(std::sync::atomic::Ordering::Relaxed), 1);
    }
}
