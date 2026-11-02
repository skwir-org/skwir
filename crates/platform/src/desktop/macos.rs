//! macOS: the user notification center, the dock tile's badge, an activity
//! of the process's. No presence and no hold of the session's end here: a
//! program without windows stays in the Dock, whose badge already shows
//! the progress and whose click the UI follows; the UI answers
//! `applicationShouldTerminate:` itself.

use std::cell::OnceCell;
use std::time::Instant;

use block2::RcBlock;
use crossbeam_channel::Receiver;
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2::runtime::{Bool, NSObjectProtocol, ProtocolObject};
use objc2_app_kit::NSApplication;
use objc2_foundation::{NSActivityOptions, NSArray, NSBundle, NSError, NSProcessInfo, NSString};
use objc2_user_notifications::{
    UNAuthorizationOptions, UNMutableNotificationContent, UNNotificationRequest,
    UNUserNotificationCenter,
};

use super::{Notice, Presence, Progress, Setup, Shown, State};

/// The progress as the dock's badge shows it: whole percents, none for
/// an error; paused shown as it was.
pub(super) fn shown(progress: Option<Progress>) -> Option<Shown> {
    progress
        .filter(|progress| progress.state != State::Error)
        .map(|progress| Shown {
            permille: super::permille(progress.fraction, 10),
            state: State::Normal,
        })
}

pub(super) struct Backend {
    app_id: String,
    /// The center, asked once: none outside an application bundle.
    center: OnceCell<Option<Retained<UNUserNotificationCenter>>>,
    /// The key of the notice shown.
    shown: Option<u64>,
    /// The activity holding off sleep, while kept awake.
    activity: Option<Retained<ProtocolObject<dyn NSObjectProtocol>>>,
}

impl Backend {
    pub(super) fn new(setup: &Setup) -> Backend {
        // macOS brings the program forward itself on a click of its
        // notice: nothing is told. The center names the program by its
        // bundle, and the Dock shows its icon.
        let _ = (&setup.asked, &setup.app_name, &setup.icon);
        Backend {
            app_id: setup.app_id.clone(),
            center: OnceCell::new(),
            shown: None,
            activity: None,
        }
    }

    /// Nothing of the desktop's own wakes this.
    #[allow(clippy::unused_self)] // Each system's backend is asked alike.
    pub(super) fn wakes(&self) -> Receiver<()> {
        crossbeam_channel::never()
    }

    #[allow(clippy::unused_self)] // Each system's backend is asked alike.
    pub(super) fn due(&self) -> Option<Instant> {
        None
    }

    #[allow(clippy::unused_self)] // Each system's backend is asked alike.
    pub(super) fn tick(&mut self, now: Instant) {
        let _ = now;
    }

    #[allow(clippy::unused_self)] // Each system's backend is asked alike.
    pub(super) fn woken(&mut self) {}

    fn identifier(&self, key: u64) -> Retained<NSString> {
        NSString::from_str(&format!("{}.notice.{key}", self.app_id))
    }

    fn center(&self) -> Option<Retained<UNUserNotificationCenter>> {
        self.center
            .get_or_init(|| {
                // The center ends the program when it runs outside an
                // application bundle (a build run from the tree).
                if NSBundle::mainBundle().bundleIdentifier().is_none() {
                    tracing::debug!("notices are not shown: not run from an application bundle");
                    return None;
                }
                let center = UNUserNotificationCenter::currentNotificationCenter();
                // Asked once: the system asks the user the first time.
                let answered = RcBlock::new(|granted: Bool, _error: *mut NSError| {
                    if !granted.as_bool() {
                        tracing::debug!("notices are not shown: not allowed");
                    }
                });
                center.requestAuthorizationWithOptions_completionHandler(
                    UNAuthorizationOptions::Alert,
                    &answered,
                );
                Some(center)
            })
            .clone()
    }

    pub(super) fn notify(&mut self, key: u64, notice: &Notice) {
        let Some(center) = self.center() else {
            return;
        };
        if let Some(previous) = self.shown.replace(key).filter(|previous| *previous != key) {
            remove(&center, &self.identifier(previous));
        }
        let content = UNMutableNotificationContent::new();
        content.setTitle(&NSString::from_str(&notice.title));
        content.setBody(&NSString::from_str(&notice.body));
        let request = UNNotificationRequest::requestWithIdentifier_content_trigger(
            &self.identifier(key),
            &content,
            None,
        );
        let added = RcBlock::new(|error: *mut NSError| {
            if !error.is_null() {
                tracing::debug!("the notice was not shown");
            }
        });
        center.addNotificationRequest_withCompletionHandler(&request, Some(&added));
    }

    pub(super) fn withdraw(&mut self, key: Option<u64>) {
        let Some(shown) = self
            .shown
            .take_if(|shown| key.is_none_or(|key| key == *shown))
        else {
            return;
        };
        if let Some(Some(center)) = self.center.get() {
            remove(center, &self.identifier(shown));
        }
    }

    /// The badge on the dock's tile, set on the main thread, where
    /// `AppKit` asks for it.
    #[allow(clippy::unused_self)] // Each system's backend is told alike.
    pub(super) fn progress(&mut self, shown: Option<Shown>) {
        let label = shown.map(|shown| format!("{}%", shown.permille / 10));
        dispatch2::DispatchQueue::main().exec_async(move || {
            let Some(main) = MainThreadMarker::new() else {
                return;
            };
            let label = label.as_deref().map(NSString::from_str);
            NSApplication::sharedApplication(main)
                .dockTile()
                .setBadgeLabel(label.as_deref());
        });
    }

    pub(super) fn hold(&mut self, why: &str) {
        self.activity = Some(
            NSProcessInfo::processInfo().beginActivityWithOptions_reason(
                NSActivityOptions::UserInitiated,
                &NSString::from_str(why),
            ),
        );
    }

    pub(super) fn let_go(&mut self) {
        if let Some(activity) = self.activity.take() {
            #[allow(unsafe_code)]
            // SAFETY: the token `beginActivity` gave, ended once.
            unsafe {
                NSProcessInfo::processInfo().endActivity(&activity);
            }
        }
    }

    /// Nothing: a program without windows stays in the Dock, whose badge
    /// shows the progress, and the UI follows the Dock's click.
    #[allow(clippy::unused_self)] // Each system's backend is told alike.
    pub(super) fn presence(&mut self, presence: Option<&Presence>) {
        let _ = presence;
    }

    /// Nothing: the UI answers `applicationShouldTerminate:` itself.
    #[allow(clippy::unused_self)] // Each system's backend is told alike.
    pub(super) fn hold_session(&mut self, why: Option<&str>) {
        let _ = why;
    }
}

fn remove(center: &UNUserNotificationCenter, identifier: &NSString) {
    let identifiers = NSArray::from_slice(&[identifier]);
    center.removePendingNotificationRequestsWithIdentifiers(&identifiers);
    center.removeDeliveredNotificationsWithIdentifiers(&identifiers);
}
