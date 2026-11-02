//! Windows: toasts, the taskbar's progress, a power request, on the
//! module's thread (COM multithreaded, pumping no messages); the presence's
//! icon and the session's end held off from a hidden window on a thread of
//! its own (`presence`), which pumps them.

use std::cell::OnceCell;
use std::sync::Arc;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::time::Instant;

use ::windows::Data::Xml::Dom::XmlDocument;
use ::windows::Foundation::TypedEventHandler;
use ::windows::UI::Notifications::{ToastNotification, ToastNotificationManager, ToastNotifier};
use ::windows::Win32::Foundation::{CloseHandle, HANDLE, HWND};
use ::windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
};
use ::windows::Win32::System::Power::{
    PowerClearRequest, PowerCreateRequest, PowerRequestSystemRequired, PowerSetRequest,
};
use ::windows::Win32::System::Threading::{
    POWER_REQUEST_CONTEXT_SIMPLE_STRING, REASON_CONTEXT, REASON_CONTEXT_0,
};
use ::windows::Win32::UI::Shell::{
    ITaskbarList3, TBPF_ERROR, TBPF_NOPROGRESS, TBPF_NORMAL, TBPF_PAUSED, TBPFLAG, TaskbarList,
};
use ::windows::core::{HSTRING, IInspectable, PWSTR};
use crossbeam_channel::Receiver;
use parking_lot::Mutex;

use super::{Asked, Clicked, Notice, OnAsked, Presence, Progress, Setup, Shown, State};

mod presence;

#[cfg(test)]
pub(super) use presence::{added, click};

/// The tag and group every notice is shown with: the next takes the
/// last one's place, in the notification center too.
const TAG: &str = "notice";
const GROUP: &str = "skwir";

/// The progress as the taskbar shows it: thousandths, and its state.
pub(super) fn shown(progress: Option<Progress>) -> Option<Shown> {
    progress.map(|progress| Shown {
        permille: super::permille(progress.fraction, 1),
        state: progress.state,
    })
}

pub(super) struct Backend {
    app_id: HSTRING,
    asked: OnAsked,
    icon: &'static [u8],
    window: Arc<AtomicIsize>,
    /// Whether COM was started on this thread, to be ended with it.
    com: bool,
    /// The taskbar's list, made once when first needed.
    taskbar: OnceCell<Option<ITaskbarList3>>,
    /// The progress asked last, shown again on a window given after.
    shown: Option<Shown>,
    /// The toasts' notifier, made once when first needed: none for an
    /// id the system does not know (the zip, Wine).
    notifier: OnceCell<Option<ToastNotifier>>,
    /// The toast shown and its key.
    toast: Option<(u64, ToastNotification)>,
    /// The power request, while the machine is kept awake.
    request: Option<HANDLE>,
    /// What the hidden window shows and holds, shared with its thread.
    asks: Arc<Mutex<presence::Asks>>,
    /// The hidden window's thread, started when the presence or the hold
    /// is first asked: `None` when it could not be made (no desktop).
    hidden: OnceCell<Option<presence::Hidden>>,
}

impl Backend {
    pub(super) fn new(setup: &Setup) -> Backend {
        // Not asked of the system here, but the toast's name, and
        // logind's on Linux.
        let _ = &setup.app_name;
        #[allow(unsafe_code)]
        // SAFETY: this thread's own COM, multithreaded as it pumps no
        // messages (WinRT and the taskbar's list work from there),
        // ended when this is dropped.
        let com = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
        Backend {
            app_id: HSTRING::from(setup.app_id.as_str()),
            asked: Arc::clone(&setup.asked),
            icon: setup.icon,
            window: Arc::clone(&setup.window),
            com,
            taskbar: OnceCell::new(),
            shown: None,
            notifier: OnceCell::new(),
            toast: None,
            request: None,
            asks: Arc::default(),
            hidden: OnceCell::new(),
        }
    }

    /// Nothing of the desktop's own wakes this: the hidden window's
    /// thread answers it.
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

    pub(super) fn notify(&mut self, key: u64, notice: &Notice) {
        let Some(notifier) = self.notifier() else {
            return;
        };
        let toast = match self.toast(key, notice) {
            Ok(toast) => toast,
            Err(error) => {
                tracing::debug!("the notice was not shown: {error}");
                return;
            }
        };
        if let Some((_, shown)) = self.toast.take() {
            let _ = notifier.Hide(&shown);
        }
        match notifier.Show(&toast) {
            Ok(()) => self.toast = Some((key, toast)),
            Err(error) => tracing::debug!("the notice was not shown: {error}"),
        }
    }

    fn notifier(&self) -> Option<ToastNotifier> {
        self.notifier
            .get_or_init(|| {
                ToastNotificationManager::CreateToastNotifierWithId(&self.app_id)
                    .inspect_err(|error| tracing::debug!("notices are not shown: {error}"))
                    .ok()
            })
            .clone()
    }

    /// A toast of `notice`, its click told with `key`. Its kind is not
    /// said: Windows files every toast alike. Windows does not bring
    /// the window forward on a click: the caller does.
    fn toast(&self, key: u64, notice: &Notice) -> ::windows::core::Result<ToastNotification> {
        let xml = format!(
            r#"<toast><visual><binding template="ToastGeneric"><text>{}</text><text>{}</text></binding></visual></toast>"#,
            super::escape(&notice.title),
            super::escape(&notice.body)
        );
        let document = XmlDocument::new()?;
        document.LoadXml(&HSTRING::from(xml))?;
        let toast = ToastNotification::CreateToastNotification(&document)?;
        toast.SetTag(&HSTRING::from(TAG))?;
        toast.SetGroup(&HSTRING::from(GROUP))?;
        let asked = Arc::clone(&self.asked);
        toast.Activated(&TypedEventHandler::<ToastNotification, IInspectable>::new(
            move |_, _| {
                asked(Asked::Notice(Clicked { key, token: None }));
                Ok(())
            },
        ))?;
        Ok(toast)
    }

    pub(super) fn withdraw(&mut self, key: Option<u64>) {
        let Some((_, toast)) = self
            .toast
            .take_if(|(shown, _)| key.is_none_or(|key| key == *shown))
        else {
            return;
        };
        if let Some(Some(notifier)) = self.notifier.get()
            && let Err(error) = notifier.Hide(&toast)
        {
            tracing::debug!("the notice was not taken back: {error}");
        }
        // From the notification center too, where it went once off the
        // screen.
        let removed = ToastNotificationManager::History().and_then(|history| {
            history.RemoveGroupedTagWithId(&HSTRING::from(TAG), &HSTRING::from(GROUP), &self.app_id)
        });
        if let Err(error) = removed {
            tracing::debug!("the notice was not taken back: {error}");
        }
    }

    pub(super) fn progress(&mut self, shown: Option<Shown>) {
        self.shown = shown;
        self.show_progress();
    }

    /// The window given changed: the progress shown on its button.
    pub(super) fn window(&mut self) {
        self.show_progress();
    }

    fn show_progress(&mut self) {
        let window = self.window.load(Ordering::Relaxed);
        if window == 0 {
            return;
        }
        let Some(taskbar) = self.taskbar() else {
            return;
        };
        let window = HWND(window as *mut core::ffi::c_void);
        #[allow(unsafe_code)]
        // SAFETY: the list's own calls, on a handle the caller gave: one
        // that is gone is refused, never followed.
        let shown = unsafe {
            match self.shown {
                None => taskbar.SetProgressState(window, TBPF_NOPROGRESS),
                Some(shown) => taskbar
                    .SetProgressValue(window, u64::from(shown.permille), 1000)
                    .and_then(|()| taskbar.SetProgressState(window, flag(shown.state))),
            }
        };
        // Refused too before the window's button is made.
        if let Err(error) = shown {
            tracing::debug!("the progress is not shown on the taskbar: {error}");
        }
    }

    fn taskbar(&self) -> Option<ITaskbarList3> {
        self.taskbar
            .get_or_init(|| {
                if !self.com {
                    return None;
                }
                #[allow(unsafe_code)]
                // SAFETY: COM started on this thread; the list made and
                // started as its documentation asks, before any use.
                let made = unsafe {
                    CoCreateInstance::<_, ITaskbarList3>(&TaskbarList, None, CLSCTX_INPROC_SERVER)
                        .and_then(|taskbar| taskbar.HrInit().map(|()| taskbar))
                };
                made.inspect_err(|error| {
                    tracing::debug!("the progress is not shown on the taskbar: {error}");
                })
                .ok()
            })
            .clone()
    }

    pub(super) fn hold(&mut self, why: &str) {
        let mut reason: Vec<u16> = why.encode_utf16().chain([0]).collect();
        let context = REASON_CONTEXT {
            // POWER_REQUEST_CONTEXT_VERSION.
            Version: 0,
            Flags: POWER_REQUEST_CONTEXT_SIMPLE_STRING,
            Reason: REASON_CONTEXT_0 {
                SimpleReasonString: PWSTR(reason.as_mut_ptr()),
            },
        };
        #[allow(unsafe_code)]
        // SAFETY: the context and its string live through the calls,
        // which copy them; the request made is set, or closed at once.
        let held = unsafe {
            PowerCreateRequest(&raw const context).and_then(|request| {
                match PowerSetRequest(request, PowerRequestSystemRequired) {
                    Ok(()) => Ok(request),
                    Err(error) => {
                        let _ = CloseHandle(request);
                        Err(error)
                    }
                }
            })
        };
        match held {
            Ok(request) => self.request = Some(request),
            Err(error) => tracing::debug!("the machine may sleep meanwhile: {error}"),
        }
    }

    /// Whether a power request is held.
    #[cfg(test)]
    pub(super) fn holds(&self) -> bool {
        self.request.is_some()
    }

    pub(super) fn let_go(&mut self) {
        if let Some(request) = self.request.take() {
            #[allow(unsafe_code)]
            // SAFETY: the request this made and set, cleared and closed
            // once.
            unsafe {
                let _ = PowerClearRequest(request, PowerRequestSystemRequired);
                let _ = CloseHandle(request);
            }
        }
    }

    /// Show `presence` as the notification area's icon, or take it back.
    pub(super) fn presence(&mut self, presence: Option<&Presence>) {
        self.asks.lock().presence = presence.cloned();
        if let Some(hidden) = self.hidden(presence.is_some()) {
            hidden.tell(presence::PRESENT);
        }
    }

    /// Hold off the session's end with `why`, or let go.
    pub(super) fn hold_session(&mut self, why: Option<&str>) {
        self.asks.lock().held = why.map(str::to_owned);
        if let Some(hidden) = self.hidden(why.is_some()) {
            hidden.tell(presence::HOLD);
        }
    }

    /// The hidden window's thread, started if `start` and not yet.
    fn hidden(&self, start: bool) -> Option<&presence::Hidden> {
        if start {
            self.hidden
                .get_or_init(|| {
                    presence::Hidden::start(
                        Arc::clone(&self.asks),
                        Arc::clone(&self.asked),
                        self.icon,
                    )
                })
                .as_ref()
        } else {
            self.hidden.get()?.as_ref()
        }
    }

    /// The hidden window, if made.
    #[cfg(test)]
    pub(super) fn hidden_window(&self) -> Option<isize> {
        Some(self.hidden.get()?.as_ref()?.window())
    }
}

fn flag(state: State) -> TBPFLAG {
    match state {
        State::Normal => TBPF_NORMAL,
        State::Paused => TBPF_PAUSED,
        State::Error => TBPF_ERROR,
    }
}

impl Drop for Backend {
    fn drop(&mut self) {
        // The icon taken back and the session let go with the window.
        self.hidden.take();
        self.let_go();
        // COM's objects released before COM ends on this thread.
        self.taskbar.take();
        self.toast = None;
        self.notifier.take();
        if self.com {
            #[allow(unsafe_code)]
            // SAFETY: ends the COM this thread started, nothing of it
            // held any more.
            unsafe {
                CoUninitialize();
            }
        }
    }
}
