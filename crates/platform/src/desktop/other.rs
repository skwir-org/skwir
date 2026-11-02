//! Other systems: nothing told.

use std::time::Instant;

use crossbeam_channel::Receiver;

use super::{Notice, Presence, Progress, Setup, Shown};

pub(super) fn shown(progress: Option<Progress>) -> Option<Shown> {
    let _ = progress;
    None
}

/// Nothing told: said once, when the module's thread starts.
pub(super) struct Backend;

impl Backend {
    pub(super) fn new(setup: &Setup) -> Backend {
        let _ = (&setup.app_id, &setup.app_name, &setup.asked, &setup.icon);
        tracing::debug!(
            "{}",
            skwir_core::system::not_yet("Telling the desktop of long actions")
        );
        Backend
    }

    pub(super) fn wakes(&self) -> Receiver<()> {
        crossbeam_channel::never()
    }

    pub(super) fn due(&self) -> Option<Instant> {
        None
    }

    pub(super) fn tick(&mut self, now: Instant) {
        let _ = now;
    }

    pub(super) fn woken(&mut self) {}

    pub(super) fn notify(&mut self, key: u64, notice: &Notice) {
        let _ = (key, notice);
    }

    pub(super) fn withdraw(&mut self, key: Option<u64>) {
        let _ = key;
    }

    pub(super) fn progress(&mut self, shown: Option<Shown>) {
        let _ = shown;
    }

    pub(super) fn hold(&mut self, why: &str) {
        let _ = why;
    }

    pub(super) fn let_go(&mut self) {}

    pub(super) fn presence(&mut self, presence: Option<&Presence>) {
        let _ = presence;
    }

    pub(super) fn hold_session(&mut self, why: Option<&str>) {
        let _ = why;
    }
}
