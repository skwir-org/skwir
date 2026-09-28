//! bibli plugin: thumbnails. Registers, in order, the freedesktop cache
//! (read and write-back), the in-process image decoder, and the desktop's
//! installed thumbnailers into the core thumbnail service. The same
//! thumbnailers preview the pictures bibli does not decode (`Pictures`).
//!
//! Settings: `thumbnails.write_cache` (bool, default true) and
//! `thumbnails.budget_mb` (integer, default 64).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

use bibli_core::App;
use bibli_core::cancel::CancelToken;
use bibli_core::plugin::Plugin;
use bibli_core::preview::{Preview, PreviewInfo, PreviewProvider, PreviewRequest};
use bibli_core::scheduler::Priority;
use bibli_core::settings::SettingSpec;
use bibli_core::thumbs::{Image, ThumbRequest, ThumbnailProvider};
use bibli_platform::thumb_providers::{self, FreedesktopCache, ImageDecoder, Thumbnailers};
use bibli_platform::thumbnails::{ThumbCache, ThumbSize};

const ID: &str = "thumbnails";
const WRITE_CACHE: &str = "thumbnails.write_cache";
const BUDGET_MB: &str = "thumbnails.budget_mb";

/// The plugin.
#[derive(Default)]
pub struct ThumbnailsPlugin {
    thumbnailers: Arc<Discovered>,
}

/// The desktop's thumbnailers. Finding them reads every `.thumbnailer`
/// file, which registration must not do: a worker fills this once, and
/// until then the provider accepts nothing; files asked for meanwhile are
/// asked again once it is filled (`Thumbnails::providers_changed`). Kept
/// across settings changes and re-enabling.
#[derive(Default)]
struct Discovered {
    started: AtomicBool,
    found: OnceLock<Thumbnailers>,
}

impl ThumbnailProvider for Discovered {
    fn id(&self) -> &'static str {
        "thumbnailers"
    }

    fn accepts(&self, req: &ThumbRequest) -> bool {
        self.found.get().is_some_and(|t| t.accepts(req))
    }

    fn thumbnail(&self, req: &ThumbRequest, cancel: &CancelToken) -> anyhow::Result<Option<Image>> {
        match self.found.get() {
            Some(t) => t.thumbnail(req, cancel),
            None => Ok(None),
        }
    }
}

/// Pictures bibli does not decode itself (HEIF, AVIF, JPEG XL, a camera's
/// raw file) previewed as the desktop's thumbnailer draws them, at the
/// largest size the thumbnail cache knows, and kept there like any
/// thumbnail so the next look, and other applications, find it drawn. A 3D
/// model is one too (`pictured`): what f3d or Blender draws of it.
struct Pictures {
    thumbnailers: Arc<Discovered>,
    cache: Option<ThumbCache>,
    write_back: bool,
}

/// What the preview asks a thumbnailer to draw.
const PICTURE_SIZE: ThumbSize = ThumbSize::XXLarge;

impl PreviewProvider for Pictures {
    fn id(&self) -> &'static str {
        "desktop-pictures"
    }

    /// A fallback: a provider that reads the format itself goes first.
    fn priority(&self) -> i32 {
        -50
    }

    fn accepts(&self, req: &PreviewRequest) -> bool {
        req.mime.as_deref().is_some_and(|mime| {
            pictured(mime)
                && !thumb_providers::decodes(mime)
                && self
                    .thumbnailers
                    .found
                    .get()
                    .is_some_and(|t| t.claims(mime))
        })
    }

    fn render(&self, req: &PreviewRequest, cancel: &CancelToken) -> anyhow::Result<Preview> {
        let path = req.entry.resolve()?;
        let mime = req.mime.as_deref().unwrap_or_default();
        let cached = self
            .cache
            .as_ref()
            .and_then(|c| c.load(&path, PICTURE_SIZE));
        let image = if let Some(image) = cached {
            image
        } else {
            self.draw(&path, mime, cancel)?
        };
        // Only a picture has a camera's size to read.
        let written = mime
            .starts_with("image/")
            .then(|| bibli_platform::exif::dimensions(&path))
            .flatten();
        let (width, height) = own_size(written, (image.width, image.height));
        Ok(Preview {
            image,
            info: PreviewInfo::Picture { width, height },
        })
    }
}

/// Whether a file of type `mime` is something to look at, which a
/// thumbnailer's picture shows: an image, a 3D model (the `model` media
/// type: STL, glTF, STEP, IGES), or what the MIME database draws as a
/// picture (`Kind::Image`: a Blender file, a 3MF package).
fn pictured(mime: &str) -> bool {
    mime.starts_with("image/")
        || mime.starts_with("model/")
        || bibli_platform::mime::is_kind(
            &bibli_platform::mime::Mime(mime.to_owned()),
            bibli_platform::mime::Kind::Image,
        )
}

impl Pictures {
    /// The picture drawn by its thumbnailer, and kept in the cache.
    fn draw(
        &self,
        path: &std::path::Path,
        mime: &str,
        cancel: &CancelToken,
    ) -> anyhow::Result<Image> {
        let thumbnailers = self
            .thumbnailers
            .found
            .get()
            .ok_or_else(|| anyhow::anyhow!("No thumbnailer draws this picture"))?;
        let image = thumbnailers
            .draw(path, mime, PICTURE_SIZE.pixels(), cancel)?
            .ok_or_else(|| anyhow::anyhow!("cancelled"))?;
        if let Some(cache) = self.cache.as_ref().filter(|_| self.write_back)
            && let Ok(modified) = std::fs::metadata(path).and_then(|m| m.modified())
            && let Err(error) = cache.store(path, PICTURE_SIZE, &image, modified)
        {
            tracing::debug!("picture cache write failed: {error:#}");
        }
        Ok(image)
    }
}

/// The picture's own size, as its camera wrote it (`written`), standing the
/// way the thumbnailer drew it (`drawn`): a camera writes the sensor's
/// size, a thumbnailer turns the picture upright. Without one, what was
/// drawn.
fn own_size(written: Option<(u32, u32)>, drawn: (u32, u32)) -> (u32, u32) {
    let Some((width, height)) = written else {
        return drawn;
    };
    if (width > height) == (drawn.0 > drawn.1) || drawn.0 == drawn.1 {
        (width, height)
    } else {
        (height, width)
    }
}

fn budget(app: &App) {
    let budget_mb = app
        .settings
        .get::<i64>(BUDGET_MB)
        .unwrap_or(64)
        .clamp(4, 4096);
    app.thumbnails
        .set_budget(usize::try_from(budget_mb).unwrap_or(64) << 20);
}

impl ThumbnailsPlugin {
    /// The providers, in order: the cache first, then the decoder, then
    /// the desktop's thumbnailers.
    fn providers(&self, app: &App) {
        let write_back = app.settings.get::<bool>(WRITE_CACHE).unwrap_or(true);
        app.thumbnails.unregister_owner(ID);
        app.thumbnails
            .register(ID, Arc::new(FreedesktopCache::desktop(write_back)));
        app.thumbnails.register(ID, Arc::new(ImageDecoder));
        app.thumbnails.register(
            ID,
            Arc::clone(&self.thumbnailers) as Arc<dyn ThumbnailProvider>,
        );
        app.previews.unregister(ID, "desktop-pictures");
        app.previews.register(
            ID,
            Arc::new(Pictures {
                thumbnailers: Arc::clone(&self.thumbnailers),
                cache: ThumbCache::desktop(),
                write_back,
            }),
        );
    }
}

impl Plugin for ThumbnailsPlugin {
    fn id(&self) -> &'static str {
        ID
    }

    fn title(&self) -> &'static str {
        "Thumbnails"
    }

    fn description(&self) -> &'static str {
        "Thumbnails from the desktop cache, an image decoder, and installed thumbnailers"
    }

    fn category(&self) -> bibli_core::plugin::Category {
        bibli_core::plugin::Category::Previews
    }

    fn on_settings_changed(&self, app: &App, keys: &[String]) {
        if keys.iter().any(|k| k == BUDGET_MB) {
            budget(app);
        }
        // Only the cache provider depends on it, but the order matters, so
        // the list is registered again; nothing is discovered again.
        if keys.iter().any(|k| k == WRITE_CACHE) {
            self.providers(app);
        }
    }

    fn register(&self, app: &App) {
        app.settings.declare(SettingSpec::toggle(
            WRITE_CACHE,
            "Share generated thumbnails",
            "Store new thumbnails in the desktop's thumbnail cache so other applications reuse them.",
            true,
        ));
        app.settings.declare(
            SettingSpec::integer(
                BUDGET_MB,
                "Memory budget",
                "Decoded thumbnails kept in memory. Higher values scroll large image folders more smoothly.",
                64,
                (4, 4096),
            )
            .unit("MB")
            .step(16),
        );
        budget(app);
        self.providers(app);
        if !self.thumbnailers.started.swap(true, Ordering::AcqRel) {
            let discovered = Arc::clone(&self.thumbnailers);
            let service = Arc::clone(&app.thumbnails);
            app.scheduler.spawn(
                Priority::Background,
                "thumbnailers.discover",
                app.lifetime().child(),
                move || {
                    let found = Thumbnailers::from_system();
                    let any = !found.is_empty();
                    let _ = discovered.found.set(found);
                    if any {
                        service.providers_changed();
                    }
                },
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bibli_core::app::AppConfig;
    use bibli_core::op::EntryRef;
    use bibli_core::scheduler::{Priority, Scheduler};
    use bibli_core::thumbs::{
        Image, ThumbRequest, ThumbSize, ThumbState, ThumbnailProvider, Thumbnails,
    };
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    fn images10k() -> Option<PathBuf> {
        let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../bench-data/images10k");
        p.join("img-00000.png").exists().then_some(p)
    }

    fn wait_ready(t: &Arc<Thumbnails>, req: &ThumbRequest) -> Arc<Image> {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            match t.get(req.clone(), Priority::Visible) {
                ThumbState::Ready(img) => return img,
                ThumbState::Unavailable => panic!("unavailable: {}", req.entry.path.display()),
                ThumbState::Pending => {
                    assert!(Instant::now() < deadline, "timed out");
                    std::thread::sleep(Duration::from_millis(2));
                }
            }
        }
    }

    #[test]
    #[allow(unsafe_code)]
    fn decodes_and_writes_back_to_the_cache() {
        let Some(images) = images10k() else {
            eprintln!("bench-data/images10k missing; run `just bench-gen images10k`");
            return;
        };
        let cache = tempfile::tempdir().unwrap();
        // SAFETY: one test per process under nextest; the env is ours.
        unsafe { std::env::set_var("XDG_CACHE_HOME", cache.path()) };
        let tmp = tempfile::tempdir().unwrap();
        let app = App::new(AppConfig::ephemeral(tmp.path())).unwrap();
        app.plugins.add(Arc::new(ThumbnailsPlugin::default()), &app);
        assert!(
            app.thumbnails
                .provider_ids()
                .starts_with(&["freedesktop-cache", "image-decoder"])
        );

        let src = std::fs::canonicalize(images.join("img-00042.png")).unwrap();
        let req = ThumbRequest {
            entry: EntryRef::from_path(&src).unwrap(),
            mime: None,
            mtime: None,
            size: ThumbSize::Normal,
        };
        let img = wait_ready(&app.thumbnails, &req);
        assert!(img.width <= 128 && img.height <= 128);
        assert_eq!(img.rgba.len(), (img.width * img.height * 4) as usize);
        let cached = cache
            .path()
            .join("thumbnails/normal")
            .join(format!("{}.png", bibli_platform::thumbnails::key(&src)));
        assert!(
            cached.exists(),
            "generated thumbnail was written back to the cache"
        );
        assert_eq!(
            bibli_platform::thumbnails::png_text_chunk(&cached, "Software").as_deref(),
            Some("bibli")
        );
        assert!(app.plugins.set_enabled(ID, false, &app));
        assert!(app.thumbnails.provider_ids().is_empty());
    }

    /// A provider slow enough that ordering is observable, logging completions.
    struct Slow(parking_lot::Mutex<Vec<u64>>);

    impl ThumbnailProvider for Slow {
        fn id(&self) -> &'static str {
            "slow"
        }
        fn accepts(&self, _: &ThumbRequest) -> bool {
            true
        }
        fn thumbnail(
            &self,
            req: &ThumbRequest,
            _: &bibli_core::CancelToken,
        ) -> anyhow::Result<Option<Image>> {
            std::thread::sleep(Duration::from_millis(3));
            self.0.lock().push(req.entry.id.ino);
            Ok(Some(Image {
                width: 1,
                height: 1,
                rgba: vec![0; 4].into(),
            }))
        }
    }

    #[test]
    fn visible_requests_finish_before_the_background_backlog() {
        let t = Thumbnails::new(Scheduler::new(2));
        let slow = Arc::new(Slow(parking_lot::Mutex::new(Vec::new())));
        t.register("test", slow.clone());
        let req = |ino: u64| ThumbRequest {
            entry: EntryRef::new(bibli_core::EntryId { dev: 1, ino }, format!("/x/{ino}")),
            mime: None,
            mtime: None,
            size: ThumbSize::Normal,
        };
        for ino in 1000..1300 {
            t.get(req(ino), Priority::Background);
        }
        for ino in 1..=20 {
            t.get(req(ino), Priority::Visible);
        }
        let deadline = Instant::now() + Duration::from_secs(10);
        while (1..=20).any(|ino| matches!(t.get(req(ino), Priority::Visible), ThumbState::Pending))
        {
            assert!(
                Instant::now() < deadline,
                "visible thumbnails never completed"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
        let log = slow.0.lock();
        let last_visible = log.iter().rposition(|ino| *ino <= 20).unwrap();
        let background_before = log[..=last_visible]
            .iter()
            .filter(|ino| **ino >= 1000)
            .count();
        assert!(
            background_before <= 40,
            "{background_before} background thumbnails completed before the visible ones"
        );
    }

    #[test]
    fn a_picture_bibli_cannot_decode_is_previewed_as_its_thumbnailer_draws_it() {
        use bibli_platform::thumb_providers::ThumbnailerEntry;
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        // A "HEIF" photo that is a PNG inside, and a thumbnailer that
        // draws it by copying it, as `heif-thumbnailer -s %s %i %o` would.
        let photo = dir.path().join("a photo’s name.heic");
        image::RgbImage::from_pixel(30, 20, image::Rgb([0, 128, 255]))
            .save_with_format(&photo, image::ImageFormat::Png)
            .unwrap();
        let tool = dir.path().join("draw");
        std::fs::write(&tool, "#!/bin/sh\ncp \"$2\" \"$3\"\n").unwrap();
        std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o755)).unwrap();
        let discovered = Arc::new(Discovered::default());
        let _ = discovered
            .found
            .set(Thumbnailers::new(vec![ThumbnailerEntry {
                name: "heif".into(),
                try_exec: None,
                exec: format!("{} %s %i %o", tool.display()),
                mime_types: ["image/heif", "image/png", "model/stl", "text/plain"]
                    .map(str::to_owned)
                    .to_vec(),
            }]));
        let cache = ThumbCache::at(dir.path().join("cache"));
        let pictures = Pictures {
            thumbnailers: discovered,
            cache: Some(cache.clone()),
            write_back: true,
        };
        let request = |mime: &str| PreviewRequest {
            entry: EntryRef::from_path(&photo).unwrap(),
            mime: Some(mime.into()),
            page: 1,
            seconds: None,
            max_side: 2048,
            text_size: None,
        };
        assert!(
            !pictures.accepts(&request("image/png")),
            "bibli decodes a PNG itself"
        );
        assert!(!pictures.accepts(&request("image/jxl")), "nothing draws it");
        assert!(
            pictures.accepts(&request("model/stl")),
            "a 3D model is shown as its thumbnailer draws it"
        );
        assert!(
            !pictures.accepts(&request("text/plain")),
            "text is read, not pictured, whatever claims it"
        );
        let heif = request("image/heif");
        assert!(pictures.accepts(&heif));
        let preview = pictures.render(&heif, &CancelToken::new()).unwrap();
        assert_eq!((preview.image.width, preview.image.height), (30, 20));
        assert_eq!(
            preview.info,
            PreviewInfo::Picture {
                width: 30,
                height: 20
            }
        );
        assert!(
            cache.lookup(&photo, PICTURE_SIZE).is_some(),
            "kept in the desktop's cache"
        );
    }

    #[test]
    fn a_pictures_own_size_stands_the_way_it_was_drawn() {
        assert_eq!(own_size(None, (1024, 768)), (1024, 768));
        assert_eq!(own_size(Some((4032, 3024)), (1024, 768)), (4032, 3024));
        // The camera wrote the sensor's size; the thumbnailer turned it up.
        assert_eq!(own_size(Some((4032, 3024)), (768, 1024)), (3024, 4032));
        assert_eq!(own_size(Some((4032, 3024)), (512, 512)), (4032, 3024));
    }
}
