//! What the system says of its network connection (ADR-142): whether it
//! is metered, so what may wait for another connection (a check for a new
//! version) waits.

/// Whether the connection is metered, as the system says it: Linux's
/// desktop portal (`NetworkMonitor.GetMetered`), asked on a connection of
/// its own given up on after a while, as a portal that hangs would hold a
/// call on the shared one for good (`AGENTS.md`, the session bus). `None`
/// where nothing says: no portal, no session bus, or a system whose
/// answer is not read yet (Windows' connection cost, macOS's
/// `isExpensive`). Waits on the bus: call it on a worker.
pub fn metered() -> Option<bool> {
    imp::metered()
}

#[cfg(target_os = "linux")]
mod imp {
    use std::time::Duration;

    const DESTINATION: &str = "org.freedesktop.portal.Desktop";
    const PATH: &str = "/org/freedesktop/portal/desktop";
    const MONITOR: &str = "org.freedesktop.portal.NetworkMonitor";
    /// How long the portal may take: it answers from memory once started.
    const PATIENCE: Duration = Duration::from_secs(3);

    pub(super) fn metered() -> Option<bool> {
        let bus = zbus::blocking::connection::Builder::session()
            .map(|builder| builder.method_timeout(PATIENCE))
            .and_then(zbus::blocking::connection::Builder::build)
            .inspect_err(|error| tracing::debug!("whether the network is metered: {error}"))
            .ok()?;
        bus.call_method(Some(DESTINATION), PATH, Some(MONITOR), "GetMetered", &())
            .and_then(|reply| reply.body().deserialize::<bool>())
            .inspect_err(|error| tracing::debug!("whether the network is metered: {error}"))
            .ok()
    }
}

#[cfg(not(target_os = "linux"))]
mod imp {
    pub(super) fn metered() -> Option<bool> {
        None
    }
}
