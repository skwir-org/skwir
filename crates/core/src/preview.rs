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
    fn accepts(&self, request: &PreviewRequest) -> bool;
    fn render(&self, request: &PreviewRequest, cancel: &CancelToken) -> anyhow::Result<Preview>;
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
    pub fn register(&self, owner: &str, provider: Arc<dyn PreviewProvider>) {
        self.providers.write().push(Registered {
            owner: owner.into(),
            provider,
            lifetime: CancelToken::new(),
        });
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
        fn id(&self) -> &'static str {
            "slow"
        }
        fn accepts(&self, _: &PreviewRequest) -> bool {
            true
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
        for _ in 0..2 {
            previews.register("test", provider.clone());
            let service = Arc::clone(&previews);
            let req = request.clone();
            let worker = std::thread::spawn(move || service.render(&req, &CancelToken::new()));
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
