//! Toolkit-independent previews shared by document and media plugins.
//! Providers run only on workers, once per request, never while painting rows.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

use crate::{CancelToken, op::EntryRef, thumbs::Image};

#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PreviewRequest {
    pub entry: EntryRef,
    pub mime: Option<String>,
    /// One-based document page.
    pub page: u32,
    /// Seek position; `None` asks for a representative video frame.
    pub seconds: Option<f64>,
    /// Bound both image dimensions; backends impose their own upper limit too.
    pub max_side: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum PreviewInfo {
    Document { page: u32, pages: u32 },
    Video { seconds: f64, duration: Option<f64> },
}

#[derive(Clone, Debug)]
pub struct Preview {
    pub image: Image,
    pub info: PreviewInfo,
}

pub trait PreviewProvider: Send + Sync {
    fn id(&self) -> &'static str;
    /// Fallback providers sort after format-specific providers, including on re-enable.
    fn priority(&self) -> i32 {
        0
    }
    fn accepts(&self, request: &PreviewRequest) -> bool;
    fn render(&self, _request: &PreviewRequest, _cancel: &CancelToken) -> anyhow::Result<Preview> {
        anyhow::bail!("This provider uses structured previews")
    }
    fn inspect(
        &self,
        _request: &PreviewRequest,
        _view: &crate::inspection::InspectRequest,
        _cancel: &CancelToken,
    ) -> anyhow::Result<Option<crate::inspection::Inspection>> {
        Ok(None)
    }
    fn open_media(
        &self,
        _request: &PreviewRequest,
        _lifetime: &CancelToken,
        _wake: crate::location::Waker,
    ) -> anyhow::Result<Option<Arc<dyn crate::media::MediaSession>>> {
        Ok(None)
    }
    fn open_document(
        &self,
        _request: &PreviewRequest,
        _lifetime: &CancelToken,
    ) -> anyhow::Result<Option<Arc<dyn crate::document::DocumentSession>>> {
        Ok(None)
    }
}

#[derive(Clone)]
struct Registered {
    owner: String,
    provider: Arc<dyn PreviewProvider>,
    lifetime: CancelToken,
}

#[derive(Default)]
pub struct Previews {
    providers: RwLock<Vec<Registered>>,
    revision: AtomicU64,
}

impl Previews {
    pub fn inspect(
        &self,
        request: &PreviewRequest,
        view: &crate::inspection::InspectRequest,
        cancel: &CancelToken,
    ) -> anyhow::Result<Option<crate::inspection::Inspection>> {
        let providers = self.providers.read().clone();
        for p in providers {
            if view
                .provider
                .as_deref()
                .map_or_else(|| p.provider.accepts(request), |id| p.provider.id() == id)
            {
                let cancel = cancel.linked(&p.lifetime);
                anyhow::ensure!(!cancel.is_cancelled(), "preview cancelled");
                let result = p.provider.inspect(request, view, &cancel)?;
                anyhow::ensure!(!cancel.is_cancelled(), "preview cancelled");
                // The first matching provider owns the presentation, just as in
                // open_media/open_document. A reader without an inspection result
                // must not be displaced by a lower-priority hexadecimal fallback.
                return Ok(result);
            }
        }
        Ok(None)
    }

    /// Blocking startup on a scheduler worker; subsequent controls are nonblocking.
    pub fn open_media(
        &self,
        request: &PreviewRequest,
        lifetime: &CancelToken,
        wake: crate::location::Waker,
    ) -> anyhow::Result<Option<Arc<dyn crate::media::MediaSession>>> {
        let providers = self.providers.read().clone();
        for p in providers {
            let lifetime = lifetime.linked(&p.lifetime);
            anyhow::ensure!(!lifetime.is_cancelled(), "preview cancelled");
            if p.provider.accepts(request) {
                let session = p.provider.open_media(request, &lifetime, wake)?;
                anyhow::ensure!(!lifetime.is_cancelled(), "preview cancelled");
                return Ok(session);
            }
        }
        Ok(None)
    }

    /// Open an optional reader session on a worker. Its lifetime follows both
    /// the preview and the provider registration, independent of page requests.
    pub fn open_document(
        &self,
        request: &PreviewRequest,
        lifetime: &CancelToken,
    ) -> anyhow::Result<Option<Arc<dyn crate::document::DocumentSession>>> {
        let providers = self.providers.read().clone();
        for p in providers {
            let lifetime = lifetime.linked(&p.lifetime);
            anyhow::ensure!(!lifetime.is_cancelled(), "preview cancelled");
            if p.provider.accepts(request) {
                let session = p.provider.open_document(request, &lifetime)?;
                anyhow::ensure!(!lifetime.is_cancelled(), "preview cancelled");
                return Ok(session);
            }
        }
        Ok(None)
    }

    pub fn register(&self, owner: &str, provider: Arc<dyn PreviewProvider>) {
        let mut providers = self.providers.write();
        providers.push(Registered {
            owner: owner.into(),
            provider,
            lifetime: CancelToken::new(),
        });
        providers.sort_by_key(|p| std::cmp::Reverse(p.provider.priority()));
        self.revision.fetch_add(1, Ordering::Release);
    }

    pub fn unregister_owner(&self, owner: &str) {
        self.providers.write().retain(|p| {
            if p.owner == owner {
                p.lifetime.cancel();
                false
            } else {
                true
            }
        });
        self.revision.fetch_add(1, Ordering::Release);
    }

    pub fn revision(&self) -> u64 {
        self.revision.load(Ordering::Acquire)
    }

    pub fn ids(&self) -> Vec<&'static str> {
        self.providers
            .read()
            .iter()
            .map(|p| p.provider.id())
            .collect()
    }

    /// Blocking worker entry point. Disabling a provider cancels its active
    /// requests; re-enabling gets a fresh lifetime and cannot revive old jobs.
    pub fn render(
        &self,
        request: &PreviewRequest,
        cancel: &CancelToken,
    ) -> anyhow::Result<Option<Preview>> {
        let providers = self.providers.read().clone();
        for p in providers {
            let cancel = cancel.linked(&p.lifetime);
            anyhow::ensure!(!cancel.is_cancelled(), "preview cancelled");
            if p.provider.accepts(request) {
                let result = p.provider.render(request, &cancel)?;
                anyhow::ensure!(!cancel.is_cancelled(), "preview cancelled");
                return Ok(Some(result));
            }
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::EntryId;
    use std::time::Duration;

    struct SlowProvider(crossbeam_channel::Sender<()>);
    impl PreviewProvider for SlowProvider {
        fn inspect(
            &self,
            req: &PreviewRequest,
            _: &crate::inspection::InspectRequest,
            cancel: &CancelToken,
        ) -> anyhow::Result<Option<crate::inspection::Inspection>> {
            self.render(req, cancel)?;
            Ok(Some(crate::inspection::Inspection::default()))
        }
        fn id(&self) -> &'static str {
            "slow"
        }
        fn accepts(&self, _: &PreviewRequest) -> bool {
            true
        }
        fn open_media(
            &self,
            request: &PreviewRequest,
            lifetime: &CancelToken,
            _: crate::location::Waker,
        ) -> anyhow::Result<Option<Arc<dyn crate::media::MediaSession>>> {
            self.render(request, lifetime)?;
            Ok(None)
        }
        fn open_document(
            &self,
            request: &PreviewRequest,
            lifetime: &CancelToken,
        ) -> anyhow::Result<Option<Arc<dyn crate::document::DocumentSession>>> {
            self.render(request, lifetime)?;
            Ok(None)
        }
        fn render(&self, _: &PreviewRequest, cancel: &CancelToken) -> anyhow::Result<Preview> {
            self.0.send(()).unwrap();
            let deadline = std::time::Instant::now() + Duration::from_secs(2);
            while !cancel.is_cancelled() && std::time::Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(1));
            }
            anyhow::ensure!(cancel.is_cancelled(), "cancellation never arrived");
            // Even if a provider returns late, the registry rejects this result.
            Ok(Preview {
                image: Image {
                    width: 1,
                    height: 1,
                    rgba: vec![0; 4].into(),
                },
                info: PreviewInfo::Document { page: 1, pages: 1 },
            })
        }
    }

    #[test]
    fn disabling_cancels_in_flight_work_and_reenable_has_a_fresh_lifetime() {
        let previews = Arc::new(Previews::default());
        let (tx, rx) = crossbeam_channel::bounded(1);
        let provider = Arc::new(SlowProvider(tx));
        let request = PreviewRequest {
            entry: EntryRef::new(EntryId { dev: 1, ino: 1 }, "/synthetic.pdf"),
            mime: None,
            page: 1,
            seconds: None,
            max_side: 128,
        };
        for attempt in 0..8 {
            previews.register("test", provider.clone());
            let service = Arc::clone(&previews);
            let req = request.clone();
            let worker = std::thread::spawn(move || {
                if attempt < 2 {
                    service.render(&req, &CancelToken::new()).map(|_| ())
                } else if attempt < 4 {
                    service.open_document(&req, &CancelToken::new()).map(|_| ())
                } else if attempt < 6 {
                    service
                        .open_media(&req, &CancelToken::new(), Arc::new(|| {}))
                        .map(|_| ())
                } else {
                    service
                        .inspect(
                            &req,
                            &crate::inspection::InspectRequest::default(),
                            &CancelToken::new(),
                        )
                        .map(|_| ())
                }
            });
            rx.recv_timeout(Duration::from_secs(2)).unwrap();
            previews.unregister_owner("test");
            assert!(
                worker
                    .join()
                    .unwrap()
                    .unwrap_err()
                    .to_string()
                    .contains("cancelled")
            );
            assert!(previews.ids().is_empty());
        }
        assert!(
            previews
                .render(&request, &CancelToken::new())
                .unwrap()
                .is_none()
        );
    }
}
