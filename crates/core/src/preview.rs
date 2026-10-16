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
    /// The size body text is read at, in pixels, for a provider that lays
    /// text out into pages (a book laid out by `MuPDF`); `None` is its own
    /// default. Pictures and fixed pages ignore it: they are zoomed instead.
    #[serde(default)]
    pub text_size: Option<f32>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum PreviewInfo {
    Document {
        page: u32,
        pages: u32,
    },
    Video {
        seconds: f64,
        duration: Option<f64>,
        /// The film's own size, when the picture was asked smaller (at the
        /// size it is shown at): what it is laid out by and can be had at.
        #[serde(default)]
        size: Option<(u32, u32)>,
    },
    /// A picture another program drew at a size of its choosing (a desktop
    /// thumbnailer, for a format skwir does not decode): what it drew is all
    /// there is. `width` × `height` is the picture's own size when the file
    /// says it, else the size drawn.
    Picture {
        width: u32,
        height: u32,
    },
}

#[derive(Clone, Debug)]
pub struct Preview {
    pub image: Image,
    pub info: PreviewInfo,
}

/// A member of a container, written out as a file of its own so that it can
/// be previewed as what it is.
#[derive(Clone, Debug)]
pub struct Member {
    /// The member's own name: a label for the panel, never a path.
    pub name: String,
    /// Where the provider wrote it, inside the directory it was given; or,
    /// with `own_file`, where it already is.
    pub path: std::path::PathBuf,
    /// The member is a file of its own that the container only names (a
    /// playlist's track): nothing was written, and nothing is removed.
    pub own_file: bool,
}

/// A file shown in another's place: one of a format skwir shows (a PDF,
/// a picture, Markdown, a table), which a provider made of the file asked
/// for (ADR-047). The panel previews it as it would that file.
#[derive(Clone, Debug)]
pub struct Converted {
    /// Where it is: kept by the provider for as long as it may be shown.
    pub path: std::path::PathBuf,
    /// Its type, read from its name and bytes as any file's is.
    pub mime: Option<String>,
    /// Why its links are not followed, when they are not: a plugin from
    /// outside skwir that may not use the network made it, and a link the
    /// user clicks would carry what it read where it pleases.
    pub closed_links: Option<String>,
}

pub trait PreviewProvider: Send + Sync {
    fn id(&self) -> &'static str;
    /// Fallback providers sort after format-specific providers, including on re-enable.
    fn priority(&self) -> i32 {
        0
    }
    /// Whether it shows any file as its bytes, whatever the file is: the
    /// view a preview offers for every file (View as hex), asked by role,
    /// never by a plugin's name (`Previews::bytes_view`).
    fn shows_bytes(&self) -> bool {
        false
    }
    /// Whether it shows what it accepts as media, a film or a track: a
    /// still of it (`render`), and a player when one opens (`open_media`).
    /// The panel asks for the picture while the player opens, so neither
    /// waits for the other.
    fn media(&self) -> bool {
        false
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
    /// Decode a picture an inspection named for later
    /// (`inspection::Picture::Later`), when the view shows it. `request` and
    /// `view` are those the inspection answered; `key` is the provider's own
    /// and never a filesystem path it has not checked.
    fn picture(
        &self,
        _request: &PreviewRequest,
        _view: &crate::inspection::InspectRequest,
        _key: &str,
        _cancel: &CancelToken,
    ) -> anyhow::Result<Option<crate::thumbs::Image>> {
        Ok(None)
    }
    /// A container writes the member its rows call `target` into `into`, a
    /// private directory the caller owns and removes, under a name of the
    /// provider's choosing: a member's own name is a label, never a path.
    /// The caller previews the file like any other, so an archived picture
    /// zooms and an attached PDF opens in the reader. `None` means targets are
    /// not files here (a book's chapters) and `inspect` handles them.
    fn extract(
        &self,
        _request: &PreviewRequest,
        _target: &str,
        _into: &std::path::Path,
        _cancel: &CancelToken,
    ) -> anyhow::Result<Option<Member>> {
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
    /// The file shown in this one's place (ADR-047), asked before anything
    /// else: the panel previews it, by whichever provider shows its type,
    /// once. `None`: the provider shows the file itself.
    fn converted(
        &self,
        _request: &PreviewRequest,
        _cancel: &CancelToken,
    ) -> anyhow::Result<Option<Converted>> {
        Ok(None)
    }
}

/// The provider an inspection goes to, and the pictures it names come back
/// to: the one `view.provider` names, or the first that accepts.
fn inspector<'a>(
    request: &'a PreviewRequest,
    view: &'a crate::inspection::InspectRequest,
) -> impl Fn(&dyn PreviewProvider) -> bool + 'a {
    |p| {
        view.provider
            .as_deref()
            .map_or_else(|| p.accepts(request), |id| p.id() == id)
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
    /// The one dispatch every call shares: the first provider `chosen`
    /// picks (in priority order) runs `call` under a token that also
    /// follows the provider's registration, and a result that arrives after
    /// cancellation is refused, so a disabled provider cannot answer late.
    /// The first chosen provider owns the presentation: a reader without a
    /// result must not be displaced by a lower-priority fallback.
    fn dispatch<T>(
        &self,
        cancel: &CancelToken,
        chosen: impl Fn(&dyn PreviewProvider) -> bool,
        call: impl FnOnce(&dyn PreviewProvider, &CancelToken) -> anyhow::Result<T>,
    ) -> anyhow::Result<Option<T>> {
        anyhow::ensure!(!cancel.is_cancelled(), "preview cancelled");
        let providers = self.providers.read().clone();
        let Some(p) = providers.into_iter().find(|p| chosen(&*p.provider)) else {
            return Ok(None);
        };
        let cancel = cancel.linked(&p.lifetime);
        anyhow::ensure!(!cancel.is_cancelled(), "preview cancelled");
        let result = call(&*p.provider, &cancel)?;
        anyhow::ensure!(!cancel.is_cancelled(), "preview cancelled");
        Ok(Some(result))
    }

    /// Blocking worker entry point. Disabling a provider cancels its active
    /// requests; re-enabling gets a fresh lifetime and cannot revive old jobs.
    pub fn render(
        &self,
        request: &PreviewRequest,
        cancel: &CancelToken,
    ) -> anyhow::Result<Option<Preview>> {
        self.dispatch(
            cancel,
            |p| p.accepts(request),
            |p, cancel| p.render(request, cancel),
        )
    }

    /// The provider `view.provider` names, or the first that accepts.
    pub fn inspect(
        &self,
        request: &PreviewRequest,
        view: &crate::inspection::InspectRequest,
        cancel: &CancelToken,
    ) -> anyhow::Result<Option<crate::inspection::Inspection>> {
        Ok(self
            .dispatch(cancel, inspector(request, view), |p, cancel| {
                p.inspect(request, view, cancel)
            })?
            .flatten())
    }

    /// The provider that answered the inspection decodes the picture it
    /// named: the same choice as `inspect`.
    pub fn picture(
        &self,
        request: &PreviewRequest,
        view: &crate::inspection::InspectRequest,
        key: &str,
        cancel: &CancelToken,
    ) -> anyhow::Result<Option<Image>> {
        Ok(self
            .dispatch(cancel, inspector(request, view), |p, cancel| {
                p.picture(request, view, key, cancel)
            })?
            .flatten())
    }

    /// The first provider that accepts the container extracts from it.
    pub fn extract(
        &self,
        request: &PreviewRequest,
        target: &str,
        into: &std::path::Path,
        cancel: &CancelToken,
    ) -> anyhow::Result<Option<Member>> {
        Ok(self
            .dispatch(
                cancel,
                |p| p.accepts(request),
                |p, cancel| p.extract(request, target, into, cancel),
            )?
            .flatten())
    }

    /// The file the provider that owns `request` shows in its place
    /// (`PreviewProvider::converted`), if it shows another.
    pub fn converted(
        &self,
        request: &PreviewRequest,
        cancel: &CancelToken,
    ) -> anyhow::Result<Option<Converted>> {
        Ok(self
            .dispatch(
                cancel,
                |p| p.accepts(request),
                |p, cancel| p.converted(request, cancel),
            )?
            .flatten())
    }

    /// Whether the provider that shows `request` shows it as media
    /// (`PreviewProvider::media`): the one `open_media` and `render` go to.
    pub fn media(&self, request: &PreviewRequest) -> bool {
        let providers = self.providers.read().clone();
        providers
            .iter()
            .find(|p| p.provider.accepts(request))
            .is_some_and(|p| p.provider.media())
    }

    /// Blocking startup on a scheduler worker; subsequent controls are nonblocking.
    pub fn open_media(
        &self,
        request: &PreviewRequest,
        lifetime: &CancelToken,
        wake: crate::location::Waker,
    ) -> anyhow::Result<Option<Arc<dyn crate::media::MediaSession>>> {
        Ok(self
            .dispatch(
                lifetime,
                |p| p.accepts(request),
                |p, lifetime| p.open_media(request, lifetime, wake),
            )?
            .flatten())
    }

    /// Open an optional reader session on a worker. Its lifetime follows both
    /// the preview and the provider registration, independent of page requests.
    pub fn open_document(
        &self,
        request: &PreviewRequest,
        lifetime: &CancelToken,
    ) -> anyhow::Result<Option<Arc<dyn crate::document::DocumentSession>>> {
        Ok(self
            .dispatch(
                lifetime,
                |p| p.accepts(request),
                |p, lifetime| p.open_document(request, lifetime),
            )?
            .flatten())
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

    /// Withdraw one of `owner`'s providers, `id`, leaving its others (and
    /// the sessions they opened) as they are: an owner of several refreshes
    /// only the one a changed preference concerns.
    pub fn unregister(&self, owner: &str, id: &str) {
        self.providers.write().retain(|p| {
            if p.owner == owner && p.provider.id() == id {
                p.lifetime.cancel();
                false
            } else {
                true
            }
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

    /// The provider that shows any file as its bytes (View as hex), if one
    /// is registered (`PreviewProvider::shows_bytes`).
    pub fn bytes_view(&self) -> Option<&'static str> {
        self.providers
            .read()
            .iter()
            .find(|p| p.provider.shows_bytes())
            .map(|p| p.provider.id())
    }

    pub fn ids(&self) -> Vec<&'static str> {
        self.providers
            .read()
            .iter()
            .map(|p| p.provider.id())
            .collect()
    }

    /// Every provider that accepts `request`, first the one that shows it,
    /// with its owner and priority: which of them competes for a file.
    /// Reads the file as `accepts` may: call from a worker.
    pub fn accepting(&self, request: &PreviewRequest) -> Vec<(String, &'static str, i32)> {
        let providers = self.providers.read().clone();
        providers
            .iter()
            .filter(|p| p.provider.accepts(request))
            .map(|p| (p.owner.clone(), p.provider.id(), p.provider.priority()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
            entry: EntryRef::new(crate::FileId { dev: 1, ino: 1 }, "/synthetic.pdf"),
            mime: None,
            page: 1,
            seconds: None,
            max_side: 128,
            text_size: None,
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
