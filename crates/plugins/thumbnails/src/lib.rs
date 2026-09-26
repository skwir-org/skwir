//! bibli plugin: thumbnails. Registers, in order, the freedesktop cache
//! (read and write-back), the in-process image decoder, and the desktop's
//! installed thumbnailers into the core thumbnail service.
//!
//! Settings: `thumbnails.write_cache` (bool, default true) and
//! `thumbnails.budget_mb` (integer, default 64).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

use bibli_core::App;
use bibli_core::cancel::CancelToken;
use bibli_core::plugin::Plugin;
use bibli_core::scheduler::Priority;
use bibli_core::settings::SettingSpec;
use bibli_core::thumbs::{Image, ThumbRequest, ThumbnailProvider};
use bibli_platform::thumb_providers::{FreedesktopCache, ImageDecoder, Thumbnailers};

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
}
