//! An address as a location (ADR-077): opening `sftp://me@host/docs`
//! connects it, on a thread of its own since the user may be asked, and
//! then sends the pane to the mounted folder (`ListingEvent::Redirect`),
//! which takes the address's place in its history. Typed, bookmarked,
//! given to `skwir`, navigated by a script: one way.

use std::path::PathBuf;
use std::sync::Arc;

use crossbeam_channel::Receiver;
use parking_lot::Mutex;
use skwir_core::app::App;
use skwir_core::cancel::CancelToken;
use skwir_core::entry::EntryId;
use skwir_core::location::{
    Completion, Completions, EmptyState, EventSink, ListingEvent, Location, LocationProvider,
    OpenContext, OpenLocation,
};
use skwir_core::scheduler::Priority;

use crate::Remotes;
use crate::address::{Address, Scheme};
use crate::connector::{Ask, Device};
use crate::known;

pub struct Provider {
    scheme: &'static str,
    remotes: Remotes,
    app: std::sync::Weak<App>,
}

impl Provider {
    pub fn new(scheme: &'static str, remotes: Remotes, app: &App) -> Self {
        Provider {
            scheme,
            remotes,
            app: app.weak(),
        }
    }
}

impl LocationProvider for Provider {
    fn scheme(&self) -> &str {
        self.scheme
    }

    fn open(&self, location: &Location, ctx: OpenContext) -> anyhow::Result<Arc<dyn OpenLocation>> {
        let address = Address::of_location(location)?;
        let (tx, rx) = ctx.channel();
        let open = Arc::new(Connecting {
            location: location.clone(),
            rx,
            how: Arc::new(How {
                address,
                tx,
                cancel: ctx.cancel.clone(),
                remotes: self.remotes.clone(),
                app: self.app.clone(),
                // An agent's call connects only once the user allowed it:
                // the server it names is told who the user is (their keys).
                agent: skwir_core::permissions::agent_call().is_some(),
                running: Mutex::new(false),
            }),
        });
        open.how.connect();
        Ok(open)
    }

    fn title(&self, location: &Location) -> Option<String> {
        Address::of_location(location)
            .ok()
            .map(|a| self.remotes.label_of(&a))
    }

    /// The servers known, as the server is typed (`sftp://me@ho`): those
    /// mounted, the tools' and the bookmarks', and for SFTP the hosts of
    /// the user's ssh configuration. A folder's name after the server is
    /// not completed: the server would be asked. Called on a worker.
    fn complete(&self, rest: &str) -> Option<Completions> {
        let typed = known::typing_server(rest)?;
        let scheme = Scheme::from_name(self.scheme)?;
        let mut addresses = self.remotes.known_addresses();
        if scheme == Scheme::Sftp {
            addresses.extend(
                known::user_ssh_hosts()
                    .iter()
                    .filter_map(|host| known::sftp_of(host)),
            );
        }
        let items = known::of_scheme(scheme, addresses)
            .into_iter()
            .map(|address| {
                let server = known::authority(&address);
                Completion {
                    label: server.clone(),
                    text: server,
                    location: address.location(),
                    inside: Some('/'),
                }
            })
            .collect();
        Some(Completions {
            range: 2..rest.len(),
            typed: typed.to_owned(),
            items,
        })
    }

    fn empty_state(&self, location: &Location) -> Option<EmptyState> {
        let address = Address::of_location(location).ok()?;
        let label = self.remotes.label_of(&address);
        Some(match address.scheme.device() {
            Some(Device::Phone) => EmptyState::new(
                format!("Opening {label}…"),
                "A phone shows its files once it is unlocked.",
            ),
            Some(Device::Camera) => EmptyState::new(
                format!("Opening {label}…"),
                "A camera shows its files while it is switched on.",
            ),
            None => EmptyState::new(
                format!("Connecting to {label}…"),
                "If the server asks for a password, or to be trusted the first time, the window asks you.",
            ),
        })
    }
}

/// An address being connected.
struct Connecting {
    location: Location,
    rx: Receiver<ListingEvent>,
    how: Arc<How>,
}

/// What connecting it needs, shared with the thread that does.
struct How {
    address: Address,
    tx: EventSink,
    cancel: CancelToken,
    remotes: Remotes,
    app: std::sync::Weak<App>,
    agent: bool,
    /// A connection is being made: F5 waits for it.
    running: Mutex<bool>,
}

impl How {
    fn connect(self: &Arc<Self>) {
        {
            let mut running = self.running.lock();
            if *running || self.cancel.is_cancelled() {
                return;
            }
            *running = true;
        }
        let this = Arc::clone(self);
        let spawned = std::thread::Builder::new()
            .name("skwir-connect".into())
            .spawn(move || {
                let outcome = this.reach();
                *this.running.lock() = false;
                if this.cancel.is_cancelled() {
                    return;
                }
                match outcome {
                    Ok(path) => this
                        .tx
                        .send(ListingEvent::Redirect(Location::from_path(path))),
                    Err(error) => {
                        let label = this.remotes.label_of(&this.address);
                        this.tx
                            .send(ListingEvent::Error(match this.address.scheme.device() {
                                Some(_) => format!("Could not open {label}: {error:#}"),
                                None => format!("Could not connect to {label}: {error:#}"),
                            }))
                    }
                };
            });
        if let Err(error) = spawned {
            *self.running.lock() = false;
            self.tx.send(ListingEvent::Error(error.to_string()));
        }
    }

    fn reach(&self) -> anyhow::Result<PathBuf> {
        let app = self
            .app
            .upgrade()
            .ok_or_else(|| anyhow::anyhow!("Skwir is quitting"))?;
        let ask: Arc<dyn Ask> = Arc::new(crate::WindowAsk {
            app: self.app.clone(),
        });
        // A device's files are the user's as a server's are: asked too.
        let (title, detail) = match self.address.scheme.device() {
            Some(_) => (
                format!(
                    "An agent asks to open {}",
                    self.remotes.label_of(&self.address)
                ),
                format!("It asked to open {}.", self.address),
            ),
            None => (
                format!("An agent asks to connect to {}", self.address.host),
                format!(
                    "It asked to open {}. Connecting tells the server who you are.",
                    self.address
                ),
            ),
        };
        if self.agent && !ask.confirm(&title, &detail, &self.cancel) {
            anyhow::bail!("you did not allow it");
        }
        self.remotes.reach(&app, &self.address, &ask, &self.cancel)
    }
}

impl OpenLocation for Connecting {
    fn location(&self) -> &Location {
        &self.location
    }

    fn events(&self) -> &Receiver<ListingEvent> {
        &self.rx
    }

    fn hydrate(&self, _ids: &[EntryId], _priority: Priority) {}

    fn path_of(&self, _id: EntryId) -> Option<PathBuf> {
        None
    }

    /// F5 after a failure tries again.
    fn refresh(&self) {
        self.how.connect();
    }

    fn close(&self) {
        self.how.cancel.cancel();
    }
}
