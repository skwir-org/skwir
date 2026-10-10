//! bibli plugin: thumbnails. Registers, in order, the freedesktop cache
//! (read and write-back), the in-process image decoder, the desktop's
//! installed thumbnailers, and `ImageMagick` as the last resort, into the
//! core thumbnail service. The same thumbnailers, then `ImageMagick`,
//! preview the pictures bibli does not decode (`Pictures`). Thumbnailers
//! and `ImageMagick` run isolated (ADR-031).
//!
//! Settings: `thumbnails.write_cache` (bool, default true),
//! `thumbnails.budget_mb` (integer, default 64) and
//! `thumbnails.imagemagick` (bool, default true).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

use bibli_core::App;
use bibli_core::cancel::CancelToken;
use bibli_core::plugin::{Plugin, Requirement};
use bibli_core::preview::{Preview, PreviewInfo, PreviewProvider, PreviewRequest};
use bibli_core::scheduler::Priority;
use bibli_core::settings::SettingSpec;
use bibli_core::thumbs::{Image, Stage, ThumbRequest, ThumbnailProvider};
use bibli_platform::magick::Magick;
use bibli_platform::thumb_providers::{self, FreedesktopCache, ImageDecoder, Thumbnailers};
use bibli_platform::thumbnails::{ThumbCache, ThumbSize};

const ID: &str = "thumbnails";
const WRITE_CACHE: &str = "thumbnails.write_cache";
const BUDGET_MB: &str = "thumbnails.budget_mb";
const IMAGEMAGICK: &str = "thumbnails.imagemagick";

/// The plugin.
#[derive(Default)]
pub struct ThumbnailsPlugin {
    thumbnailers: Arc<Discovered>,
}

/// The desktop's thumbnailers and `ImageMagick`. Finding them reads every
/// `.thumbnailer` file and runs `ImageMagick`, which registration must not
/// do: a worker fills this once, and until then the providers accept
/// nothing; files asked for meanwhile are asked again once it is filled
/// (`Thumbnails::providers_changed`). Kept across settings changes and
/// re-enabling.
#[derive(Default)]
struct Discovered {
    started: AtomicBool,
    found: OnceLock<Thumbnailers>,
    magick: OnceLock<Option<Magick>>,
}

impl Discovered {
    fn magick(&self) -> Option<&Magick> {
        self.magick.get().and_then(Option::as_ref)
    }
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

/// `ImageMagick`, the last resort: a picture nothing else drew (ADR-031).
struct Converted(Arc<Discovered>);

impl ThumbnailProvider for Converted {
    fn id(&self) -> &'static str {
        "imagemagick"
    }

    fn stage(&self) -> Stage {
        Stage::LastResort
    }

    fn accepts(&self, req: &ThumbRequest) -> bool {
        self.0
            .magick()
            .zip(mime_of(req))
            .is_some_and(|(magick, mime)| magick.draws(&mime))
    }

    fn thumbnail(&self, req: &ThumbRequest, cancel: &CancelToken) -> anyhow::Result<Option<Image>> {
        let (Some(magick), Some(mime)) = (self.0.magick(), mime_of(req)) else {
            return Ok(None);
        };
        magick.draw(&req.entry.path, &mime, req.size.pixels(), cancel)
    }
}

/// The type of a request: the caller's, else the name's.
fn mime_of(req: &ThumbRequest) -> Option<String> {
    match &req.mime {
        Some(mime) => Some(mime.to_string()),
        None => bibli_platform::mime::by_name(req.entry.path.file_name()?).map(|m| m.0),
    }
}

/// Pictures bibli does not decode itself (HEIF, AVIF, JPEG XL, a camera's
/// raw file) previewed as the desktop's thumbnailer draws them, else as
/// `ImageMagick` does, at the largest size the thumbnail cache knows, and
/// kept there like any thumbnail so the next look, and other applications,
/// find it drawn. A 3D model is one too (`pictured`): what f3d or Blender
/// draws of it.
struct Pictures {
    thumbnailers: Arc<Discovered>,
    /// Whether `ImageMagick` may draw what no thumbnailer does.
    magick: bool,
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
                && (self.claimed(mime) || self.converted(mime).is_some())
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
    /// Whether a desktop thumbnailer draws pictures of type `mime`.
    fn claimed(&self, mime: &str) -> bool {
        self.thumbnailers
            .found
            .get()
            .is_some_and(|t| t.claims(mime))
    }

    /// `ImageMagick`, if it may draw pictures of type `mime`.
    fn converted(&self, mime: &str) -> Option<&Magick> {
        self.thumbnailers
            .magick()
            .filter(|magick| self.magick && magick.draws(mime))
    }

    /// The picture drawn by its thumbnailer, else by `ImageMagick` (when the
    /// thumbnailer failed too), and kept in the cache.
    fn draw(
        &self,
        path: &std::path::Path,
        mime: &str,
        cancel: &CancelToken,
    ) -> anyhow::Result<Image> {
        let pixels = PICTURE_SIZE.pixels();
        let mut failed = None;
        let mut drawn = None;
        if let Some(thumbnailers) = self.thumbnailers.found.get().filter(|t| t.claims(mime)) {
            match thumbnailers.draw(path, mime, pixels, cancel) {
                Ok(image) => drawn = Some(image),
                Err(error) => failed = Some(error),
            }
        }
        if drawn.is_none()
            && let Some(magick) = self.converted(mime)
        {
            drawn = Some(magick.draw(path, mime, pixels, cancel)?);
        }
        let image = match (drawn, failed) {
            (Some(Some(image)), _) => image,
            (Some(None), _) => anyhow::bail!("cancelled"),
            (None, Some(error)) => return Err(error),
            (None, None) => anyhow::bail!("No thumbnailer draws this picture"),
        };
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
    /// the system's thumbnails (macOS, Windows) or the desktop's
    /// thumbnailers (Linux), and `ImageMagick` last of all (its stage).
    fn providers(&self, app: &App) {
        let write_back = app.settings.get::<bool>(WRITE_CACHE).unwrap_or(true);
        let magick = app.settings.get::<bool>(IMAGEMAGICK).unwrap_or(true);
        app.thumbnails.unregister_owner(ID);
        app.thumbnails
            .register(ID, Arc::new(FreedesktopCache::desktop(write_back)));
        app.thumbnails.register(ID, Arc::new(ImageDecoder));
        // The system's own (Quick Look, the Windows shell; ADR-041), where
        // a Linux desktop's thumbnailers are.
        if bibli_platform::system_thumbs::AVAILABLE {
            app.thumbnails.register(
                ID,
                Arc::new(bibli_platform::system_thumbs::SystemThumbnails),
            );
        }
        app.thumbnails.register(
            ID,
            Arc::clone(&self.thumbnailers) as Arc<dyn ThumbnailProvider>,
        );
        if magick {
            app.thumbnails
                .register(ID, Arc::new(Converted(Arc::clone(&self.thumbnailers))));
        }
        app.previews.unregister(ID, "desktop-pictures");
        app.previews.register(
            ID,
            Arc::new(Pictures {
                thumbnailers: Arc::clone(&self.thumbnailers),
                magick,
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
        "Thumbnails from the desktop cache, an image decoder, installed thumbnailers and ImageMagick"
    }

    fn category(&self) -> bibli_core::plugin::Category {
        bibli_core::plugin::Category::Previews
    }

    fn requirements(&self) -> Vec<Requirement> {
        vec![Requirement::program(
            "magick",
            "Pictures no thumbnailer draws (HEIC, AVIF, JPEG XL, raw files…)",
        )]
        .into_iter()
        .chain(bibli_platform::sandbox::PROGRAM.map(|program| {
            Requirement::program(program, "Thumbnailers and ImageMagick run isolated")
        }))
        .collect()
    }

    fn on_settings_changed(&self, app: &App, keys: &[String]) {
        if keys.iter().any(|k| k == BUDGET_MB) {
            budget(app);
        }
        // Only some providers depend on them, but the order matters, so
        // the list is registered again; nothing is discovered again.
        if keys.iter().any(|k| k == WRITE_CACHE || k == IMAGEMAGICK) {
            self.providers(app);
        }
    }

    fn capabilities(&self) -> bibli_core::permissions::Capabilities {
        use bibli_core::permissions::Capability;
        bibli_core::permissions::Capabilities::of(&[Capability::Read, Capability::Programs])
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
        app.settings.declare(SettingSpec::toggle(
            IMAGEMAGICK,
            "Use ImageMagick",
            "Draw the pictures no thumbnailer draws (HEIC, AVIF, JPEG XL, camera raw files, Photoshop and GIMP images…) with ImageMagick when it is installed, isolated from your files and the network.",
            true,
        ));
        budget(app);
        self.providers(app);
        if !self.thumbnailers.started.swap(true, Ordering::AcqRel) {
            let discovered = Arc::clone(&self.thumbnailers);
            let service = Arc::clone(&app.thumbnails);
            let kept = app.cache.namespace(ID);
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
                    let magick = Magick::find_kept(Some(&kept));
                    let any = magick.is_some();
                    let _ = discovered.magick.set(magick);
                    if any {
                        service.providers_changed();
                    }
                },
            );
        }
    }
}

#[cfg(test)]
// Helpers of the tests that run on Linux alone (ADR-035).
#[cfg_attr(not(target_os = "linux"), allow(dead_code, unused_imports))]
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

    // Types from the free desktops' database (step E, ADR-035).
    #[cfg(target_os = "linux")]
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

        let src = bibli_core::os_path::canonical(images.join("img-00042.png")).unwrap();
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

    #[cfg(unix)]
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
            magick: false,
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

    #[cfg(unix)]
    #[test]
    fn imagemagick_draws_last_what_no_thumbnailer_could() {
        use bibli_platform::thumb_providers::ThumbnailerEntry;
        use std::os::unix::fs::PermissionsExt;
        if !bibli_platform::sandbox::available() {
            eprintln!("bubblewrap cannot isolate here");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let photo = dir.path().join("a photo’s [name].heic");
        std::fs::write(&photo, b"not read by the fakes").unwrap();
        let executable = |name: &str, script: &str| {
            let path = dir.path().join(name);
            std::fs::write(&path, script).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
            path
        };
        // A thumbnailer that fails, and an ImageMagick that draws a 3×2
        // picture where its last argument says, whatever it is asked.
        let failing = executable("thumbnail", "#!/bin/sh\nexit 1\n");
        let magick = executable(
            "magick",
            "#!/bin/sh\nfor last; do :; done\nprintf 'P3\\n3 2\\n255\\n0 0 255 0 0 255 0 0 255 0 0 255 0 0 255 0 0 255\\n' > \"${last#PAM:}\"\n",
        );
        let discovered = Arc::new(Discovered::default());
        let _ = discovered
            .found
            .set(Thumbnailers::new(vec![ThumbnailerEntry {
                name: "heif".into(),
                try_exec: None,
                exec: format!("{} %i %o", failing.display()),
                mime_types: vec!["image/heif".into()],
            }]));
        let _ = discovered.magick.set(Some(Magick::listing(
            magick,
            "     HEIC  rw+   High Efficiency Image Format\n",
        )));
        let converted = Converted(Arc::clone(&discovered));
        assert_eq!(converted.stage(), Stage::LastResort);
        let thumb = ThumbRequest {
            entry: EntryRef::from_path(&photo).unwrap(),
            mime: Some("image/heif".into()),
            mtime: None,
            size: ThumbSize::Normal,
        };
        assert!(converted.accepts(&thumb));
        let image = converted
            .thumbnail(&thumb, &CancelToken::new())
            .unwrap()
            .expect("drawn");
        assert_eq!((image.width, image.height), (3, 2));
        assert_eq!(&image.rgba[..4], &[0, 0, 255, 255]);

        let pictures = |magick| Pictures {
            thumbnailers: Arc::clone(&discovered),
            magick,
            cache: None,
            write_back: false,
        };
        let preview = PreviewRequest {
            entry: EntryRef::from_path(&photo).unwrap(),
            mime: Some("image/heif".into()),
            page: 1,
            seconds: None,
            max_side: 2048,
            text_size: None,
        };
        assert!(
            pictures(false)
                .render(&preview, &CancelToken::new())
                .is_err(),
            "the thumbnailer fails, and ImageMagick is off"
        );
        let shown = pictures(true)
            .render(&preview, &CancelToken::new())
            .unwrap();
        assert_eq!((shown.image.width, shown.image.height), (3, 2));
        let jxl = PreviewRequest {
            mime: Some("image/jxl".into()),
            ..preview
        };
        assert!(
            !pictures(true).accepts(&jxl),
            "this ImageMagick does not read it"
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
