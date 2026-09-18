//! bibli plugin: thumbnails. Registers, in order, the freedesktop cache
//! (read and write-back), the in-process image decoder, and the desktop's
//! installed thumbnailers into the core thumbnail service.
//!
//! Settings: `thumbnails.write_cache` (bool, default true) and
//! `thumbnails.budget_mb` (integer, default 64).

use std::sync::Arc;

use bibli_core::App;
use bibli_core::plugin::Plugin;
use bibli_core::settings::SettingSpec;
use bibli_platform::thumb_providers::{FreedesktopCache, ImageDecoder, Thumbnailers};

const ID: &str = "thumbnails";

/// The plugin.
#[derive(Debug, Default)]
pub struct ThumbnailsPlugin;

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

    fn register(&self, app: &App) {
        app.settings.declare(SettingSpec {
            key: "thumbnails.write_cache".into(),
            description: "Store generated thumbnails in the freedesktop cache so other applications reuse them".into(),
            default: toml::Value::Boolean(true),
        });
        app.settings.declare(SettingSpec {
            key: "thumbnails.budget_mb".into(),
            description: "Megabytes of decoded thumbnails kept in memory".into(),
            default: toml::Value::Integer(64),
        });
        let write_back = app
            .settings
            .get::<bool>("thumbnails.write_cache")
            .unwrap_or(true);
        let budget_mb = app
            .settings
            .get::<i64>("thumbnails.budget_mb")
            .unwrap_or(64)
            .clamp(4, 4096);
        app.thumbnails
            .set_budget(usize::try_from(budget_mb).unwrap_or(64) << 20);
        app.thumbnails
            .register(ID, Arc::new(FreedesktopCache { write_back }));
        app.thumbnails.register(ID, Arc::new(ImageDecoder));
        let thumbnailers = Thumbnailers::from_system();
        if !thumbnailers.is_empty() {
            app.thumbnails.register(ID, Arc::new(thumbnailers));
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
        app.plugins.add(Arc::new(ThumbnailsPlugin), &app);
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
