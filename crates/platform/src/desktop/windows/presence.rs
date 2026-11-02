//! The hidden window, on a thread of its own pumping its messages: the
//! presence's icon in the notification area, and the session's end held
//! off. Windows tells a program's top-level windows (a hidden one too, but
//! not a message-only one) that the notification area started again
//! (`TaskbarCreated`) and that the session ends (`WM_QUERYENDSESSION`); a
//! shutdown's reason is set from the thread that made the window. The
//! module's thread, which pumps nothing, hands it what to show and hold
//! through [`Asks`] and a posted message.
//!
//! Where the user goes on with the shutdown anyway, gpui's own hidden
//! window is told `WM_ENDSESSION` and runs the program's quit handlers:
//! nothing more is done here.

use std::cell::{Cell, RefCell};
use std::sync::Arc;
use std::time::Duration;

use ::windows::Win32::Foundation::{
    E_FAIL, ERROR_CLASS_ALREADY_EXISTS, GetLastError, HWND, LPARAM, LRESULT, TRUE, WPARAM,
};
use ::windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateBitmap, CreateDIBSection, DIB_RGB_COLORS,
    DeleteObject,
};
use ::windows::Win32::System::LibraryLoader::GetModuleHandleW;
use ::windows::Win32::System::Shutdown::{ShutdownBlockReasonCreate, ShutdownBlockReasonDestroy};
use ::windows::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY, NIM_SETVERSION,
    NIN_SELECT, NINF_KEY, NOTIFYICON_VERSION_4, NOTIFYICONDATAW, Shell_NotifyIconW,
};
use ::windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreateIconIndirect, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyIcon,
    DestroyMenu, DestroyWindow, DispatchMessageW, EndMenu, GWLP_USERDATA, GetMessageW,
    GetSystemMetrics, GetWindowLongPtrW, HICON, ICONINFO, MF_SEPARATOR, MF_STRING, MSG,
    PostMessageW, PostQuitMessage, RegisterClassExW, RegisterWindowMessageW, SM_CXSMICON,
    SetForegroundWindow, SetWindowLongPtrW, TPM_NONOTIFY, TPM_RETURNCMD, TPM_RIGHTBUTTON,
    TrackPopupMenuEx, TranslateMessage, WM_APP, WM_CONTEXTMENU, WM_DESTROY, WM_NCDESTROY, WM_NULL,
    WM_QUERYENDSESSION, WNDCLASSEXW, WS_EX_TOOLWINDOW, WS_OVERLAPPED,
};
use ::windows::core::{HSTRING, PCWSTR, w};
use crossbeam_channel::{Receiver, RecvTimeoutError, Sender};
use parking_lot::Mutex;

use super::super::icon::{Badge, Icons};
use super::super::{Asked, OnAsked, Presence};

/// Show what [`Asks::presence`] asks.
pub(super) const PRESENT: u32 = WM_APP + 1;
/// Hold off the session's end as [`Asks::held`] asks.
pub(super) const HOLD: u32 = WM_APP + 2;
/// The notification area telling what was done to the icon.
const TRAY: u32 = WM_APP + 3;
/// End the window, and with it its thread.
const END: u32 = WM_APP + 4;
/// Whether the notification area holds the icon (the tests').
#[cfg(test)]
const ADDED: u32 = WM_APP + 5;
/// The icon's id among the window's.
const ICON: u32 = 1;
/// The key's choice of the icon: Enter or Space on it.
const NIN_KEYSELECT: u32 = NIN_SELECT | NINF_KEY;
/// The menu's entries.
const OPEN: usize = 1;
const PAUSE: usize = 2;
const STOP: usize = 3;
const CLASS: PCWSTR = w!("skwir-desktop");
/// How long the window is waited for to be made, then to end: a thread
/// that pumps its own messages does either at once.
const WAIT: Duration = Duration::from_secs(5);

/// What the module's thread asks the window to show and hold.
#[derive(Default)]
pub(super) struct Asks {
    pub(super) presence: Option<Presence>,
    /// Why the session's end is held off, while it is.
    pub(super) held: Option<String>,
}

/// The hidden window's thread, ended when this is dropped.
pub(super) struct Hidden {
    window: isize,
    /// Disconnected once the thread has ended.
    ended: Receiver<()>,
}

impl Hidden {
    /// The hidden window made on a thread of its own: `None` where it could
    /// not be (no desktop), which is logged.
    pub(super) fn start(
        asks: Arc<Mutex<Asks>>,
        asked: OnAsked,
        taskbar: Sender<()>,
        icon: &'static [u8],
    ) -> Option<Hidden> {
        let (made, window) = crossbeam_channel::bounded(1);
        let (ends, ended) = crossbeam_channel::bounded::<()>(0);
        let started = std::thread::Builder::new()
            .name("skwir-presence".into())
            .spawn(move || {
                run(asks, asked, taskbar, icon, &made);
                drop(ends);
            });
        if let Err(error) = started {
            tracing::debug!("nothing shows the program at work: {error}");
            return None;
        }
        match window.recv_timeout(WAIT) {
            Ok(Some(window)) => Some(Hidden { window, ended }),
            // Said by the thread.
            Ok(None) | Err(RecvTimeoutError::Disconnected) => None,
            Err(RecvTimeoutError::Timeout) => {
                tracing::debug!("nothing shows the program at work: its window was not made");
                None
            }
        }
    }

    /// Tell the window to look again at what is asked (`PRESENT`, `HOLD`).
    pub(super) fn tell(&self, message: u32) {
        #[allow(unsafe_code)]
        // SAFETY: a message posted to the window this made, alive until this
        // is dropped; one that is gone is refused, never followed.
        let posted = unsafe { PostMessageW(Some(self.hwnd()), message, WPARAM(0), LPARAM(0)) };
        if let Err(error) = posted {
            tracing::debug!("the hidden window was not told: {error}");
        }
    }

    #[cfg(test)]
    pub(super) fn window(&self) -> isize {
        self.window
    }

    fn hwnd(&self) -> HWND {
        HWND(self.window as *mut core::ffi::c_void)
    }
}

impl Drop for Hidden {
    fn drop(&mut self) {
        self.tell(END);
        if self.ended.recv_timeout(WAIT) == Err(RecvTimeoutError::Timeout) {
            tracing::debug!("the hidden window's thread did not end");
        }
    }
}

/// Make the window, say which, and pump its messages until it ends.
fn run(
    asks: Arc<Mutex<Asks>>,
    asked: OnAsked,
    taskbar: Sender<()>,
    icon: &'static [u8],
    made: &Sender<Option<isize>>,
) {
    let context = Box::new(Context {
        asks,
        asked,
        taskbar,
        #[allow(unsafe_code)]
        // SAFETY: a name registered for the whole session, 0 if refused.
        taskbar_created: unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) },
        state: RefCell::new(State {
            icons: Icons::new(icon),
            icon: None,
            added: false,
            blocking: false,
            refused: false,
        }),
        in_menu: Cell::new(false),
        ending: Cell::new(false),
    });
    // Freed by this thread once the window ended and its messages were
    // pumped: never while the procedure runs, which may end the window
    // (`DestroyWindow`) from within a call on the context.
    let context = Box::into_raw(context);
    match make(context) {
        Ok(window) if made.send(Some(window.0 as isize)).is_ok() => pump(),
        Ok(window) => {
            // Given up on: nobody would end it.
            #[allow(unsafe_code)]
            // SAFETY: the window this thread made.
            let _ = unsafe { DestroyWindow(window) };
        }
        Err(error) => {
            tracing::debug!(
                "nothing shows the program at work, nor holds off the session's end: {error}"
            );
            let _ = made.send(None);
        }
    }
    #[allow(unsafe_code)]
    // SAFETY: the context made above, which the window, ended or never
    // made, no longer names.
    drop(unsafe { Box::from_raw(context) });
}

/// Pump this thread's messages until its window ended.
fn pump() {
    let mut message = MSG::default();
    loop {
        #[allow(unsafe_code)]
        // SAFETY: this thread's own queue, read into a message it owns and
        // handed back to the system as it asks.
        unsafe {
            // 0 once the window ended, -1 on an error.
            if GetMessageW(&raw mut message, None, 0, 0).0 <= 0 {
                break;
            }
            let _ = TranslateMessage(&raw const message);
            DispatchMessageW(&raw const message);
        }
    }
}

/// The hidden window, `context` named by it until it ends.
fn make(context: *mut Context) -> ::windows::core::Result<HWND> {
    #[allow(unsafe_code)]
    // SAFETY: the class names a procedure of this module's; the window is
    // top-level, hidden, this thread's. The context is named by the window
    // once made, until it ends (`WM_NCDESTROY`); its thread frees it after.
    unsafe {
        let instance = GetModuleHandleW(None)?;
        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(procedure),
            hInstance: instance.into(),
            lpszClassName: CLASS,
            ..Default::default()
        };
        // Registered once a process: a later desktop finds it there.
        if RegisterClassExW(&raw const class) == 0 && GetLastError() != ERROR_CLASS_ALREADY_EXISTS {
            return Err(::windows::core::Error::from_thread());
        }
        let window = CreateWindowExW(
            WS_EX_TOOLWINDOW,
            CLASS,
            w!("skwir"),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(instance.into()),
            None,
        )?;
        SetWindowLongPtrW(window, GWLP_USERDATA, context as isize);
        Ok(window)
    }
}

/// The window's procedure: what is asked looked at, the icon's clicks and
/// menu told, the session's end answered.
#[allow(unsafe_code)]
unsafe extern "system" fn procedure(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // SAFETY: the context this window names, alive until its thread ends,
    // after the window; before it is named, and after, the system's
    // default.
    unsafe {
        let context = GetWindowLongPtrW(window, GWLP_USERDATA) as *const Context;
        if context.is_null() {
            return DefWindowProcW(window, message, wparam, lparam);
        }
        if message == WM_NCDESTROY {
            // Freed by the thread once its messages are pumped.
            SetWindowLongPtrW(window, GWLP_USERDATA, 0);
            return DefWindowProcW(window, message, wparam, lparam);
        }
        let context = &*context;
        match message {
            PRESENT => context.present(window),
            HOLD => context.hold(window),
            TRAY => context.tray(window, wparam, lparam),
            // TRUE lets the session end.
            WM_QUERYENDSESSION => {
                return LRESULT(isize::from(context.asks.lock().held.is_none()));
            }
            END => context.end(window),
            #[cfg(test)]
            ADDED => {
                let added = context.state.try_borrow().is_ok_and(|state| state.added);
                return LRESULT(isize::from(added));
            }
            WM_DESTROY => {
                context.taken_down(window);
                PostQuitMessage(0);
            }
            _ if message != 0 && message == context.taskbar_created => {
                // The notification area started again (Explorer did), and
                // forgot every icon, and the taskbar every button's
                // progress.
                if let Ok(mut state) = context.state.try_borrow_mut() {
                    state.added = false;
                }
                context.present(window);
                let _ = context.taskbar.send(());
            }
            _ => return DefWindowProcW(window, message, wparam, lparam),
        }
        LRESULT(0)
    }
}

/// What the window's procedure keeps, read through a shared reference: the
/// menu's own loop runs the procedure again while it is open.
struct Context {
    asks: Arc<Mutex<Asks>>,
    asked: OnAsked,
    /// Told when the taskbar started again (`TaskbarCreated`).
    taskbar: Sender<()>,
    taskbar_created: u32,
    state: RefCell<State>,
    /// Whether the menu is open.
    in_menu: Cell<bool>,
    /// The window asked to end while the menu was open: once it closes.
    ending: Cell<bool>,
}

struct State {
    icons: Icons,
    /// The icon given to the notification area, and its badge.
    icon: Option<(HICON, Badge)>,
    /// Whether the notification area holds the icon.
    added: bool,
    /// Whether a reason holds off the session's end.
    blocking: bool,
    /// Whether the notification area refused the icon: said once.
    refused: bool,
}

impl Context {
    /// What is asked looked at again later, the state being in use by a
    /// call this one came within.
    fn again(window: HWND, message: u32) {
        #[allow(unsafe_code)]
        // SAFETY: a message posted to this thread's own window.
        let _ = unsafe { PostMessageW(Some(window), message, WPARAM(0), LPARAM(0)) };
    }

    /// Show the presence asked as the notification area's icon, or take it
    /// back.
    fn present(&self, window: HWND) {
        let Ok(mut state) = self.state.try_borrow_mut() else {
            Context::again(window, PRESENT);
            return;
        };
        let presence = self.asks.lock().presence.clone();
        let Some(presence) = presence else {
            remove(window, &mut state);
            return;
        };
        let badge = Badge::of(&presence);
        let mut replaced = None;
        if state.icon.is_none_or(|(_, drawn)| drawn != badge) {
            match icon(&mut state.icons, badge) {
                Ok(made) => replaced = state.icon.replace((made, badge)),
                Err(error) => tracing::debug!("the program's icon was not drawn: {error}"),
            }
        }
        let Some((icon, _)) = state.icon else {
            return;
        };
        let mut data = data(window);
        data.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP | NIF_SHOWTIP;
        data.uCallbackMessage = TRAY;
        data.hIcon = icon;
        let tip = super::super::tip(&presence.title, &presence.said, data.szTip.len());
        data.szTip[..tip.len()].copy_from_slice(&tip);
        #[allow(unsafe_code)]
        // SAFETY: the area's own calls, with data that lives through them;
        // it copies the icon, so the one replaced is destroyed after.
        unsafe {
            let modified = state.added && Shell_NotifyIconW(NIM_MODIFY, &raw const data).as_bool();
            if !modified {
                // Never added, or the area forgot it.
                state.added = Shell_NotifyIconW(NIM_ADD, &raw const data).as_bool();
                if state.added {
                    data.Anonymous.uVersion = NOTIFYICON_VERSION_4;
                    let _ = Shell_NotifyIconW(NIM_SETVERSION, &raw const data);
                    state.refused = false;
                } else if !std::mem::replace(&mut state.refused, true) {
                    tracing::debug!("the notification area refused the program's icon");
                }
            }
            if let Some((replaced, _)) = replaced {
                let _ = DestroyIcon(replaced);
            }
        }
    }

    /// Hold off the session's end with the reason asked, or let go. The
    /// session's end is answered by what is asked, whether or not the
    /// reason could be set.
    fn hold(&self, window: HWND) {
        let Ok(mut state) = self.state.try_borrow_mut() else {
            Context::again(window, HOLD);
            return;
        };
        let held = self.asks.lock().held.clone();
        #[allow(unsafe_code)]
        // SAFETY: calls on the window this thread made, as they ask.
        let done = unsafe {
            match &held {
                Some(why) => ShutdownBlockReasonCreate(window, &HSTRING::from(why.as_str())),
                None if state.blocking => ShutdownBlockReasonDestroy(window),
                None => Ok(()),
            }
        };
        match done {
            Ok(()) => state.blocking = held.is_some(),
            Err(error) => {
                tracing::debug!("no reason is shown for holding off the session's end: {error}");
            }
        }
    }

    /// What the notification area told of the icon (version 4): chosen, or
    /// its menu asked at a point.
    fn tray(&self, window: HWND, wparam: WPARAM, lparam: LPARAM) {
        match (lparam.0 as u32) & 0xffff {
            NIN_SELECT | NIN_KEYSELECT => (self.asked)(Asked::Open { token: None }),
            WM_CONTEXTMENU => {
                let x = i32::from((wparam.0 as u16).cast_signed());
                let y = i32::from(((wparam.0 >> 16) as u16).cast_signed());
                self.menu(window, x, y);
            }
            _ => {}
        }
    }

    /// The icon's menu at `(x, y)`, what is chosen told. Its own loop runs
    /// meanwhile, the window's messages with it.
    fn menu(&self, window: HWND, x: i32, y: i32) {
        let pause = self
            .asks
            .lock()
            .presence
            .as_ref()
            .and_then(|presence| presence.pause);
        #[allow(unsafe_code)]
        // SAFETY: a menu made, shown and destroyed here, on this thread's
        // window, as the documentation asks: the window brought forward
        // first, so the menu closes when clicked away from, and a message
        // posted after, so the next one opens.
        let chosen = unsafe {
            let Ok(menu) = CreatePopupMenu() else {
                return;
            };
            let _ = AppendMenuW(menu, MF_STRING, OPEN, w!("Open Skwir"));
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
            if let Some(pause) = pause {
                let label = if pause { w!("Pause all") } else { w!("Go on") };
                let _ = AppendMenuW(menu, MF_STRING, PAUSE, label);
            }
            let _ = AppendMenuW(menu, MF_STRING, STOP, w!("Stop all and quit"));
            let _ = SetForegroundWindow(window);
            self.in_menu.set(true);
            let chosen = TrackPopupMenuEx(
                menu,
                (TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_NONOTIFY).0,
                x,
                y,
                window,
                None,
            );
            self.in_menu.set(false);
            let _ = PostMessageW(Some(window), WM_NULL, WPARAM(0), LPARAM(0));
            let _ = DestroyMenu(menu);
            chosen.0 as usize
        };
        if self.ending.get() {
            self.end(window);
            return;
        }
        let what = match chosen {
            OPEN => Some(Asked::Open { token: None }),
            PAUSE => pause.map(|pause| if pause { Asked::PauseAll } else { Asked::GoOn }),
            STOP => Some(Asked::StopAll),
            _ => None,
        };
        if let Some(what) = what {
            (self.asked)(what);
        }
    }

    /// End the window: once its menu closed, if open.
    fn end(&self, window: HWND) {
        self.ending.set(true);
        #[allow(unsafe_code)]
        // SAFETY: calls on this thread's own menu and window.
        unsafe {
            if self.in_menu.get() {
                let _ = EndMenu();
            } else {
                let _ = DestroyWindow(window);
            }
        }
    }

    /// The window ends: the icon taken back, the session let go.
    fn taken_down(&self, window: HWND) {
        let Ok(mut state) = self.state.try_borrow_mut() else {
            return;
        };
        remove(window, &mut state);
        if state.blocking {
            #[allow(unsafe_code)]
            // SAFETY: the reason this window set.
            let _ = unsafe { ShutdownBlockReasonDestroy(window) };
            state.blocking = false;
        }
    }
}

/// The icon's data, naming it: what is shown filled in after.
fn data(window: HWND) -> NOTIFYICONDATAW {
    NOTIFYICONDATAW {
        cbSize: size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: window,
        uID: ICON,
        ..Default::default()
    }
}

/// Take the icon back from the notification area, and destroy it.
fn remove(window: HWND, state: &mut State) {
    #[allow(unsafe_code)]
    // SAFETY: the area's own call naming this window's icon; the icon this
    // made, destroyed once.
    unsafe {
        if std::mem::take(&mut state.added) {
            let data = data(window);
            let _ = Shell_NotifyIconW(NIM_DELETE, &raw const data);
        }
        if let Some((icon, _)) = state.icon.take() {
            let _ = DestroyIcon(icon);
        }
    }
}

/// The program's icon with `badge`, at the size the notification area
/// draws its icons.
fn icon(icons: &mut Icons, badge: Badge) -> ::windows::core::Result<HICON> {
    #[allow(unsafe_code)]
    // SAFETY: a call without arguments.
    let edge = unsafe { GetSystemMetrics(SM_CXSMICON) }.clamp(16, 64);
    let size = edge.unsigned_abs();
    let rgba = icons.draw(size, badge).ok_or_else(|| {
        ::windows::core::Error::new(E_FAIL, "the program's icon could not be read")
    })?;
    let header = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: edge,
            // Rows from the top.
            biHeight: -edge,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    // A mask of zeros: the colours' alpha says what shows. Its rows are
    // padded to 16 bits.
    let mask_bits = vec![0u8; size.div_ceil(16) as usize * 2 * size as usize];
    #[allow(unsafe_code)]
    // SAFETY: a section of `size`² pixels of 4 bytes, written within its
    // bounds before the icon copies it; both bitmaps deleted after, as
    // `CreateIconIndirect` copies them.
    unsafe {
        let mut bits = std::ptr::null_mut();
        let colour = CreateDIBSection(
            None,
            &raw const header,
            DIB_RGB_COLORS,
            &raw mut bits,
            None,
            0,
        )?;
        if bits.is_null() {
            let _ = DeleteObject(colour.into());
            return Err(::windows::core::Error::new(
                E_FAIL,
                "no bitmap for the icon",
            ));
        }
        // Blue, green, red, alpha; not premultiplied.
        let pixels = std::slice::from_raw_parts_mut(bits.cast::<u8>(), rgba.len());
        for (to, &[r, g, b, a]) in pixels
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(rgba.as_chunks::<4>().0)
        {
            *to = [b, g, r, a];
        }
        let mask = CreateBitmap(edge, edge, 1, 1, Some(mask_bits.as_ptr().cast()));
        let info = ICONINFO {
            fIcon: TRUE,
            xHotspot: 0,
            yHotspot: 0,
            hbmMask: mask,
            hbmColor: colour,
        };
        let icon = CreateIconIndirect(&raw const info);
        let _ = DeleteObject(colour.into());
        let _ = DeleteObject(mask.into());
        icon
    }
}

/// The icon chosen, as the notification area tells it: sent to `window`.
#[cfg(test)]
pub(in super::super) fn click(window: HWND) {
    #[allow(unsafe_code)]
    // SAFETY: the message the window's procedure answers, sent to it.
    unsafe {
        ::windows::Win32::UI::WindowsAndMessaging::SendMessageW(
            window,
            TRAY,
            Some(WPARAM(0)),
            Some(LPARAM(isize::try_from(NIN_SELECT | (ICON << 16)).unwrap())),
        );
    }
}

/// Whether the notification area holds the icon once the presence asked
/// is shown: shown first, at once, on `window`'s thread.
#[cfg(test)]
pub(in super::super) fn added(window: HWND) -> bool {
    use ::windows::Win32::UI::WindowsAndMessaging::SendMessageW;
    #[allow(unsafe_code)]
    // SAFETY: messages the window's procedure answers, sent to it.
    unsafe {
        SendMessageW(window, PRESENT, Some(WPARAM(0)), Some(LPARAM(0)));
        SendMessageW(window, ADDED, Some(WPARAM(0)), Some(LPARAM(0))) == LRESULT(1)
    }
}
