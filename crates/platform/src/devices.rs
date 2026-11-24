//! What Windows says of the drives Skwir watches folders on (ADR-149, 16).
//! Windows' folder watch (`ReadDirectoryChangesW`, notify's) holds the
//! folder it watches open, so Windows' Eject (Explorer's, Safely Remove
//! Hardware's) found a drive in use while a pane showed a folder there. A
//! hidden window, on a thread of its own, is registered for the notices of
//! a handle on each drive a watch is on (`RegisterDeviceNotificationW`,
//! `DBT_DEVTYP_HANDLE`), and answers them as Linux's desktops' notices are
//! answered (`places::follow_unmounts`, the places plugin's `App::release`):
//!
//! - asked to remove the drive (`DBT_DEVICEQUERYREMOVE`), or about to lock
//!   or dismount its volume (`GUID_IO_VOLUME_LOCK`, `_DISMOUNT`: a card's or
//!   a disc's eject, a format, a check), `UnmountNotice::Announced` is told
//!   (what shows a file there lets go of it, the panes there read by path),
//!   the watches there are let go of (`watch::let_go_on`) and the folders
//!   held there waited for (`fs::wait_let_go`), [`WAIT`] at most, its
//!   handle closed: the window answers, and the removal goes on;
//! - the removal refused or cancelled (`DBT_DEVICEQUERYREMOVEFAILED`, the
//!   lock or dismount failed or ended), the watches are watched again,
//!   each told to read its folder again (`watch::watch_again_on`),
//!   `Refused` is told, and the drive is registered anew, as Windows asks
//!   (a registration names the handle it was made for);
//! - removed, asked or pulled out (`DBT_DEVICEREMOVEPENDING`,
//!   `DBT_DEVICEREMOVECOMPLETE`, for the handle or by the volumes'
//!   broadcast, `DBT_DEVTYP_VOLUME`, which a hidden top-level window hears
//!   and a message-only one does not), the watches there are told their
//!   folders went (`watch::gone_on`), and its registration ends.
//!
//! The volumes' broadcast of a drive come or gone, any drive, tells the
//! table (`reach::drives_changed`), which lists Windows' drives: the panes
//! follow it as Linux's follow the mounts (`App::volumes_changed`), a
//! folder there said gone with its drive (`Failure::Left`), and opened
//! again once it is back.
//!
//! The drives registered are those a folder watch is on (core tells each
//! watch's volume, `watch::follow_volumes`), of a kind Windows removes,
//! locks or ejects (removable, fixed, a disc), never the system's own,
//! whose lock (a check asked for at the next start) would let go of every
//! folder on it, nor a network's. Elsewhere nothing is followed here:
//! Linux's watches hold nothing, nor do macOS's.

use std::path::PathBuf;

/// What Windows says of a drive registered for, as the window reads it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Notice {
    /// To be removed, asked first, or its volume about to be locked or
    /// dismounted: what holds it must let go.
    Asked,
    /// It stays: the removal refused or cancelled, the lock or dismount
    /// failed or ended.
    Kept,
    /// Removed, asked or not.
    Gone,
}

/// What the window does for a drive, in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Act {
    /// Open a handle on the drive's root and register for its notices.
    Register,
    /// Close the handle, keeping the registration: what becomes of the
    /// removal is told to it still.
    Close,
    /// End the registration, the handle closed if it is open.
    Unregister,
    /// Tell what follows unmounts that the drive is to go
    /// (`UnmountNotice::Announced`).
    Announce,
    /// Let go of the watches on it, and wait for the folders held there.
    LetGo,
    /// Watch again what was let go of there.
    WatchAgain,
    /// Tell what follows unmounts that it stays (`UnmountNotice::Refused`).
    Refuse,
    /// Tell the watches there that their folders went.
    Gone,
}

/// A drive as the window holds it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Drive {
    /// Registered for its notices.
    registered: bool,
    /// The handle registered for is open: closed once its removal is asked.
    open: bool,
    /// Its removal was asked and is not decided yet.
    letting_go: bool,
}

impl Drive {
    /// What `notice` asks, in order, the drive left as the acts leave it.
    fn told(&mut self, notice: Notice) -> Vec<Act> {
        let mut acts = Vec::new();
        match notice {
            Notice::Asked => {
                // A lock after the removal asked (an eject that dismounts)
                // is told once.
                if !self.letting_go {
                    acts.push(Act::Announce);
                }
                acts.push(Act::LetGo);
                if self.open {
                    acts.push(Act::Close);
                }
                self.letting_go = true;
                self.open = false;
            }
            Notice::Kept => {
                if !self.letting_go {
                    return acts;
                }
                if self.registered {
                    acts.push(Act::Unregister);
                }
                acts.extend([Act::WatchAgain, Act::Refuse]);
                *self = Drive::default();
            }
            Notice::Gone => {
                if self.registered || self.open {
                    acts.push(Act::Unregister);
                }
                acts.push(Act::Gone);
                *self = Drive::default();
            }
        }
        acts
    }

    /// What makes it registered while `wanted` (a watch on it, of a kind
    /// Windows removes) and not otherwise: nothing while its removal is
    /// asked, which decides.
    fn look(&mut self, wanted: bool) -> Option<Act> {
        if self.letting_go {
            return None;
        }
        match (wanted, self.registered) {
            (true, false) => {
                self.registered = true;
                self.open = true;
                Some(Act::Register)
            }
            (false, true) => {
                *self = Drive::default();
                Some(Act::Unregister)
            }
            _ => None,
        }
    }
}

/// A drive's kind, as Windows says it (`GetDriveTypeW`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Removable,
    Fixed,
    Disc,
    /// A network's, a RAM disk, or none known.
    Other,
}

/// Whether a drive of `kind` is registered for: one Windows removes, locks
/// or ejects, never the system's own.
fn registers(kind: Kind, system: bool) -> bool {
    !system && matches!(kind, Kind::Removable | Kind::Fixed | Kind::Disc)
}

/// The roots of the drives a volumes' broadcast names (`dbcv_unitmask`:
/// bit 0 for `A:`).
fn letters(mask: u32) -> Vec<PathBuf> {
    (0..26u8)
        .filter(|n| mask & (1 << n) != 0)
        .map(|n| PathBuf::from(format!("{}:\\", char::from(b'A' + n))))
        .collect()
}

/// Follow what Windows says of removing a drive a folder watch is on,
/// telling `told` (`places::follow_unmounts`), until `stop`: whether its
/// thread started, as Linux's say. The window is made there, never waited
/// for; one that could not be is logged.
#[cfg(windows)]
pub(crate) fn follow(
    stop: &skwir_core::cancel::CancelToken,
    told: std::sync::Arc<dyn Fn(crate::places::UnmountNotice) + Send + Sync>,
) -> bool {
    imp::follow(stop, told, imp::system_volume())
}

#[cfg(windows)]
mod imp {
    // The system's calls on Windows, which take raw pointers and handles:
    // each `unsafe` block says why its call is sound, and nothing unsafe
    // leaves the module.
    #![allow(unsafe_code)]

    use std::cell::RefCell;
    use std::os::windows::ffi::OsStrExt as _;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicIsize, Ordering};
    use std::time::{Duration, Instant};

    use parking_lot::Mutex;
    use skwir_core::cancel::CancelToken;
    use skwir_core::fs::watch;
    use windows_sys::Win32::Foundation::{
        CloseHandle, ERROR_CLASS_ALREADY_EXISTS, GetLastError, HANDLE, HWND, INVALID_HANDLE_VALUE,
        LPARAM, LRESULT, WPARAM,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE,
        FILE_SHARE_READ, FILE_SHARE_WRITE, GetDriveTypeW, OPEN_EXISTING,
    };
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::System::SystemInformation::GetSystemWindowsDirectoryW;
    use windows_sys::Win32::System::WindowsProgramming::{
        DRIVE_CDROM, DRIVE_FIXED, DRIVE_REMOVABLE,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DBT_CUSTOMEVENT, DBT_DEVICEARRIVAL, DBT_DEVICEQUERYREMOVE,
        DBT_DEVICEQUERYREMOVEFAILED, DBT_DEVICEREMOVECOMPLETE, DBT_DEVICEREMOVEPENDING,
        DBT_DEVTYP_HANDLE, DBT_DEVTYP_VOLUME, DBTF_NET, DEV_BROADCAST_HANDLE, DEV_BROADCAST_HDR,
        DEV_BROADCAST_VOLUME, DEVICE_NOTIFY_WINDOW_HANDLE, DefWindowProcW, DestroyWindow,
        DispatchMessageW, GUID_IO_VOLUME_DISMOUNT, GUID_IO_VOLUME_DISMOUNT_FAILED,
        GUID_IO_VOLUME_LOCK, GUID_IO_VOLUME_LOCK_FAILED, GUID_IO_VOLUME_MOUNT,
        GUID_IO_VOLUME_UNLOCK, GetMessageW, HDEVNOTIFY, MSG, PostMessageW, PostQuitMessage,
        RegisterClassW, RegisterDeviceNotificationW, UnregisterDeviceNotification, WM_APP,
        WM_DESTROY, WM_DEVICECHANGE, WNDCLASSW, WS_EX_TOOLWINDOW, WS_POPUP,
    };
    use windows_sys::core::GUID;

    use super::{Act, Drive, Kind, Notice, letters, registers};
    use crate::places::UnmountNotice;

    /// What follows unmounts (`places::follow_unmounts`' `told`).
    type Told = Arc<dyn Fn(UnmountNotice) + Send + Sync>;

    /// How long a removal asked waits for what Skwir holds on the drive
    /// to be let go of: the watches' own threads close their handles within
    /// a tenth of a second; a folder held is closed by a worker.
    pub(super) const WAIT: Duration = Duration::from_secs(2);

    /// Look at the drives core told of (`ASKED`).
    const LOOK: u32 = WM_APP + 1;
    /// End the window, and with it its thread.
    const END: u32 = WM_APP + 2;
    /// The registration of the drive whose root `wparam` points to, as an
    /// `LRESULT`, while the handle it was made for is open; 0 otherwise
    /// (the tests').
    #[cfg(test)]
    const REGISTRATION: u32 = WM_APP + 3;

    /// The window that follows, 0 when none does: one at a time (the places
    /// plugin's, a later one taking over).
    static WINDOW: AtomicIsize = AtomicIsize::new(0);
    /// The drives core told of since the window last looked
    /// (`watch::follow_volumes`).
    static ASKED: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

    thread_local! {
        /// What the window holds, on its own thread.
        static STATE: RefCell<Option<Context>> = const { RefCell::new(None) };
    }

    struct Context {
        told: Told,
        /// The system's drive (`C:\`), never registered for.
        system: Option<PathBuf>,
        drives: Vec<Held>,
    }

    /// A drive, and the handle and registration the window holds for it.
    struct Held {
        volume: PathBuf,
        drive: Drive,
        handle: HANDLE,
        notify: HDEVNOTIFY,
    }

    impl Context {
        /// The drive at `volume`, held from now on if it was not.
        fn held(&mut self, volume: &Path) -> &mut Held {
            if let Some(at) = self.drives.iter().position(|held| held.volume == volume) {
                return &mut self.drives[at];
            }
            self.drives.push(Held {
                volume: volume.to_path_buf(),
                drive: Drive::default(),
                handle: std::ptr::null_mut(),
                notify: std::ptr::null_mut(),
            });
            let last = self.drives.len() - 1;
            &mut self.drives[last]
        }
    }

    /// With what the window holds; `None` off its thread, or where it is
    /// already being looked at.
    fn with<T>(f: impl FnOnce(&mut Context) -> T) -> Option<T> {
        STATE.with(|state| state.try_borrow_mut().ok()?.as_mut().map(f))
    }

    /// The system's drive, by the volume of Windows' own folder.
    pub(super) fn system_volume() -> Option<PathBuf> {
        let mut buffer = [0u16; 261];
        // SAFETY: a buffer of the length given.
        let length = unsafe { GetSystemWindowsDirectoryW(buffer.as_mut_ptr(), 261) };
        let length = usize::try_from(length).ok().filter(|&n| n > 0 && n < 261)?;
        let folder = PathBuf::from(String::from_utf16_lossy(&buffer[..length]));
        skwir_core::fs::reach::volume_of(&folder)
    }

    /// The window made on a thread of its own, followed until `stop`:
    /// whether the thread started.
    pub(super) fn follow(stop: &CancelToken, told: Told, system: Option<PathBuf>) -> bool {
        let stop = stop.clone();
        let started = std::thread::Builder::new()
            .name("skwir-devices".into())
            .spawn(move || run(&stop, told, system));
        if let Err(error) = started {
            tracing::debug!("a drive's removal is not followed: {error}");
            return false;
        }
        true
    }

    fn run(stop: &CancelToken, told: Told, system: Option<PathBuf>) {
        if stop.is_cancelled() {
            return;
        }
        let window = match make() {
            Ok(window) => window,
            Err(error) => {
                tracing::debug!("a drive's removal is not followed: {error}");
                return;
            }
        };
        STATE.with(|state| {
            *state.borrow_mut() = Some(Context {
                told,
                system,
                drives: Vec::new(),
            });
        });
        let previous = WINDOW.swap(window as isize, Ordering::AcqRel);
        if previous != 0 {
            // SAFETY: a window of this module's, whose thread ends it.
            unsafe { PostMessageW(previous as HWND, END, 0, 0) };
        }
        // Told from now on, and the drives watched now looked at.
        for volume in watch::follow_volumes(asked) {
            asked(&volume);
        }
        let bits = window as isize;
        let _ending = stop.on_cancel(move || {
            // SAFETY: this thread's window, or none any more: a message
            // to a window gone is dropped.
            unsafe { PostMessageW(bits as HWND, END, 0, 0) };
        });
        let mut message = MSG::default();
        loop {
            // SAFETY: this thread's own queue, read into a message it owns
            // and handed back as the system asks; 0 once the window ended,
            // -1 on an error.
            unsafe {
                if GetMessageW(&raw mut message, std::ptr::null_mut(), 0, 0) <= 0 {
                    break;
                }
                DispatchMessageW(&raw const message);
            }
        }
        STATE.with(|state| state.borrow_mut().take());
    }

    /// The hidden window: top-level, as the volumes' broadcast reaches
    /// those alone, never shown.
    fn make() -> std::io::Result<HWND> {
        let class: Vec<u16> = "skwir-devices".encode_utf16().chain([0]).collect();
        // SAFETY: this module's procedure, a NUL-terminated name that
        // outlives the calls; the window is this thread's.
        unsafe {
            let instance = GetModuleHandleW(std::ptr::null());
            let definition = WNDCLASSW {
                lpfnWndProc: Some(procedure),
                hInstance: instance,
                lpszClassName: class.as_ptr(),
                ..WNDCLASSW::default()
            };
            if RegisterClassW(&raw const definition) == 0
                && GetLastError() != ERROR_CLASS_ALREADY_EXISTS
            {
                return Err(std::io::Error::last_os_error());
            }
            let window = CreateWindowExW(
                WS_EX_TOOLWINDOW,
                class.as_ptr(),
                class.as_ptr(),
                WS_POPUP,
                0,
                0,
                0,
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                instance,
                std::ptr::null(),
            );
            if window.is_null() {
                return Err(std::io::Error::last_os_error());
            }
            Ok(window)
        }
    }

    /// Core made or ended a watch on `volume` (`watch::follow_volumes`),
    /// on its thread: the window looks at it on its own.
    fn asked(volume: &Path) {
        let first = {
            let mut asked = ASKED.lock();
            let first = asked.is_empty();
            if !asked.iter().any(|v| v == volume) {
                asked.push(volume.to_path_buf());
            }
            first
        };
        let window = WINDOW.load(Ordering::Acquire);
        if first && window != 0 {
            // SAFETY: the window that follows, or one gone: dropped then.
            unsafe { PostMessageW(window as HWND, LOOK, 0, 0) };
        }
    }

    /// The window's procedure.
    unsafe extern "system" fn procedure(
        window: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match message {
            WM_DEVICECHANGE => {
                // SAFETY: what the system hands with this message.
                unsafe { device_changed(window, wparam, lparam) };
                // Granted: Skwir never refuses a removal.
                1
            }
            LOOK => {
                look(window);
                0
            }
            END => {
                end(window);
                0
            }
            #[cfg(test)]
            REGISTRATION => {
                // SAFETY: the tests hand a path that outlives the call.
                let volume = unsafe { &*(wparam as *const PathBuf) };
                with(|cx| {
                    cx.drives
                        .iter()
                        .find(|held| held.volume == *volume && !held.handle.is_null())
                        .map_or(0, |held| held.notify as LRESULT)
                })
                .unwrap_or(0)
            }
            WM_DESTROY => {
                // SAFETY: on the window's own thread.
                unsafe { PostQuitMessage(0) };
                0
            }
            // SAFETY: the system's default, for what is not this module's.
            _ => unsafe { DefWindowProcW(window, message, wparam, lparam) },
        }
    }

    /// What `event` says, `lparam` its `DEV_BROADCAST_HDR` or nothing.
    ///
    /// # Safety
    /// `lparam` is what the system handed with `WM_DEVICECHANGE`.
    unsafe fn device_changed(window: HWND, wparam: WPARAM, lparam: LPARAM) {
        let Ok(event) = u32::try_from(wparam) else {
            return;
        };
        if lparam == 0
            || !matches!(
                event,
                DBT_DEVICEARRIVAL
                    | DBT_DEVICEQUERYREMOVE
                    | DBT_DEVICEQUERYREMOVEFAILED
                    | DBT_DEVICEREMOVEPENDING
                    | DBT_DEVICEREMOVECOMPLETE
                    | DBT_CUSTOMEVENT
            )
        {
            return;
        }
        let header = lparam as *const DEV_BROADCAST_HDR;
        // SAFETY: for these events `lparam` is a header the system keeps
        // for the call, `dbch_size` bytes long, of the kind it says.
        let (size, kind) = unsafe { ((*header).dbch_size, (*header).dbch_devicetype) };
        let size = usize::try_from(size).unwrap_or(0);
        match kind {
            DBT_DEVTYP_HANDLE
                if size >= std::mem::offset_of!(DEV_BROADCAST_HANDLE, dbch_eventguid) =>
            {
                let handle = lparam as *const DEV_BROADCAST_HANDLE;
                // SAFETY: within the `size` bytes the system handed; the
                // event's kind only where it says one.
                let (notify, guid) = unsafe {
                    let guid = (event == DBT_CUSTOMEVENT
                        && size >= std::mem::offset_of!(DEV_BROADCAST_HANDLE, dbch_nameoffset))
                    .then(|| (*handle).dbch_eventguid);
                    ((*handle).dbch_hdevnotify, guid)
                };
                if let Some(notice) = notice_of(event, guid.as_ref()) {
                    told_of(window, notify, notice);
                }
            }
            DBT_DEVTYP_VOLUME if size >= std::mem::size_of::<DEV_BROADCAST_VOLUME>() => {
                let volume = lparam as *const DEV_BROADCAST_VOLUME;
                // SAFETY: a volumes' broadcast, as long as its kind's.
                let (mask, flags) = unsafe { ((*volume).dbcv_unitmask, (*volume).dbcv_flags) };
                // A network drive's: no device of this machine's.
                if flags & DBTF_NET != 0 {
                    return;
                }
                if event == DBT_DEVICEREMOVECOMPLETE {
                    for root in letters(mask) {
                        carry_out(window, &root, Notice::Gone);
                    }
                }
                if matches!(event, DBT_DEVICEARRIVAL | DBT_DEVICEREMOVECOMPLETE) {
                    // The table lists the drives: read again off the
                    // window's thread, what follows it told (the panes:
                    // a folder there said gone, or opened again).
                    let told = std::thread::Builder::new()
                        .name("skwir-drives".into())
                        .spawn(skwir_core::fs::reach::drives_changed);
                    if let Err(error) = told {
                        tracing::debug!("a drive come or gone is not told: {error}");
                    }
                }
            }
            _ => {}
        }
    }

    /// What a handle's notice says, its event's kind (`GUID_IO_VOLUME_…`)
    /// with `DBT_CUSTOMEVENT`.
    fn notice_of(event: u32, guid: Option<&GUID>) -> Option<Notice> {
        let same = |a: &GUID, b: &GUID| {
            a.data1 == b.data1 && a.data2 == b.data2 && a.data3 == b.data3 && a.data4 == b.data4
        };
        match event {
            DBT_DEVICEQUERYREMOVE => Some(Notice::Asked),
            DBT_DEVICEQUERYREMOVEFAILED => Some(Notice::Kept),
            DBT_DEVICEREMOVEPENDING | DBT_DEVICEREMOVECOMPLETE => Some(Notice::Gone),
            DBT_CUSTOMEVENT => {
                let guid = guid?;
                if [GUID_IO_VOLUME_LOCK, GUID_IO_VOLUME_DISMOUNT]
                    .iter()
                    .any(|asked| same(guid, asked))
                {
                    Some(Notice::Asked)
                } else if [
                    GUID_IO_VOLUME_LOCK_FAILED,
                    GUID_IO_VOLUME_UNLOCK,
                    GUID_IO_VOLUME_DISMOUNT_FAILED,
                    GUID_IO_VOLUME_MOUNT,
                ]
                .iter()
                .any(|kept| same(guid, kept))
                {
                    Some(Notice::Kept)
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// A notice of the drive registered as `notify`.
    fn told_of(window: HWND, notify: HDEVNOTIFY, notice: Notice) {
        let volume = with(|cx| {
            cx.drives
                .iter()
                .find(|held| !notify.is_null() && held.notify == notify)
                .map(|held| held.volume.clone())
        })
        .flatten();
        if let Some(volume) = volume {
            carry_out(window, &volume, notice);
        }
    }

    /// Do what `notice` asks of `volume`; then, the drive kept, register
    /// it anew while a watch is on it (a registration names the handle it
    /// was made for). One removed is registered again once a watch is
    /// made there.
    fn carry_out(window: HWND, volume: &Path, notice: Notice) {
        tracing::debug!(?volume, ?notice, "Windows says of a drive");
        let Some(acts) = with(|cx| cx.held(volume).drive.told(notice)) else {
            return;
        };
        for act in acts {
            act_on(window, volume, act);
        }
        if notice == Notice::Kept {
            look_at(window, volume);
        }
        forget(volume);
    }

    /// Look at the drives core told of: registered while a watch is on
    /// one of a kind Windows removes, not otherwise.
    fn look(window: HWND) {
        let volumes = std::mem::take(&mut *ASKED.lock());
        for volume in volumes {
            look_at(window, &volume);
        }
    }

    fn look_at(window: HWND, volume: &Path) {
        let system = with(|cx| cx.system.as_deref() == Some(volume)).unwrap_or(true);
        let wanted = watch::watches_on(volume) && registers(kind_of(volume), system);
        if let Some(act) = with(|cx| cx.held(volume).drive.look(wanted)).flatten() {
            act_on(window, volume, act);
        }
        forget(volume);
    }

    /// A drive the window holds nothing for any more is forgotten.
    fn forget(volume: &Path) {
        with(|cx| {
            cx.drives.retain(|held| {
                held.volume != volume
                    || held.drive != Drive::default()
                    || !held.handle.is_null()
                    || !held.notify.is_null()
            });
        });
    }

    fn kind_of(volume: &Path) -> Kind {
        let root: Vec<u16> = volume.as_os_str().encode_wide().chain([0]).collect();
        // SAFETY: a NUL-terminated path.
        match unsafe { GetDriveTypeW(root.as_ptr()) } {
            DRIVE_REMOVABLE => Kind::Removable,
            DRIVE_FIXED => Kind::Fixed,
            DRIVE_CDROM => Kind::Disc,
            _ => Kind::Other,
        }
    }

    fn act_on(window: HWND, volume: &Path, act: Act) {
        match act {
            Act::Register => {
                let made = register(window, volume);
                with(|cx| {
                    let held = cx.held(volume);
                    match made {
                        Some((handle, notify)) => {
                            held.handle = handle;
                            held.notify = notify;
                        }
                        None => held.drive = Drive::default(),
                    }
                });
            }
            Act::Close => {
                let handle =
                    with(|cx| std::mem::replace(&mut cx.held(volume).handle, std::ptr::null_mut()));
                close(handle.unwrap_or(std::ptr::null_mut()));
            }
            Act::Unregister => {
                let (handle, notify) = with(|cx| {
                    let held = cx.held(volume);
                    (
                        std::mem::replace(&mut held.handle, std::ptr::null_mut()),
                        std::mem::replace(&mut held.notify, std::ptr::null_mut()),
                    )
                })
                .unwrap_or((std::ptr::null_mut(), std::ptr::null_mut()));
                if !notify.is_null() {
                    // SAFETY: a registration this window made, ended once.
                    unsafe { UnregisterDeviceNotification(notify) };
                }
                close(handle);
            }
            Act::Announce | Act::Refuse => {
                let points = vec![volume.to_path_buf()];
                let notice = if act == Act::Announce {
                    UnmountNotice::Announced(points)
                } else {
                    UnmountNotice::Refused(points)
                };
                if let Some(told) = with(|cx| Arc::clone(&cx.told)) {
                    told(notice);
                }
            }
            Act::LetGo => {
                let began = Instant::now();
                let watches = watch::let_go_on(volume, WAIT);
                let held =
                    skwir_core::fs::wait_let_go(volume, WAIT.saturating_sub(began.elapsed()));
                tracing::debug!(
                    ?volume,
                    watches,
                    held,
                    took = ?began.elapsed(),
                    "let go of a drive Windows is to remove"
                );
            }
            Act::WatchAgain => {
                // Off the window's thread: each watch opens its folder.
                let volume = volume.to_path_buf();
                let again = std::thread::Builder::new()
                    .name("skwir-watch-again".into())
                    .spawn(move || watch::watch_again_on(&volume));
                if let Err(error) = again {
                    tracing::debug!("the watches of a drive kept are not watched again: {error}");
                }
            }
            Act::Gone => watch::gone_on(volume),
        }
    }

    /// Open a handle on `volume`'s root and register `window` for its
    /// notices: the handle and its registration.
    pub(super) fn register(window: HWND, volume: &Path) -> Option<(HANDLE, HDEVNOTIFY)> {
        let root: Vec<u16> = volume.as_os_str().encode_wide().chain([0]).collect();
        // SAFETY: a NUL-terminated path; no security attributes nor
        // template. A folder opens only with `FILE_FLAG_BACKUP_SEMANTICS`;
        // shared every way, so it keeps no one else out.
        let handle = unsafe {
            CreateFileW(
                root.as_ptr(),
                FILE_READ_ATTRIBUTES,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS,
                std::ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            tracing::debug!(
                ?volume,
                "a drive's removal is not followed: {}",
                std::io::Error::last_os_error()
            );
            return None;
        }
        let filter = DEV_BROADCAST_HANDLE {
            dbch_size: u32::try_from(std::mem::size_of::<DEV_BROADCAST_HANDLE>()).unwrap_or(0),
            dbch_devicetype: DBT_DEVTYP_HANDLE,
            dbch_handle: handle,
            ..DEV_BROADCAST_HANDLE::default()
        };
        // SAFETY: this thread's window; a filter that outlives the call.
        let notify = unsafe {
            RegisterDeviceNotificationW(
                window,
                (&raw const filter).cast(),
                DEVICE_NOTIFY_WINDOW_HANDLE,
            )
        };
        if notify.is_null() {
            tracing::debug!(
                ?volume,
                "a drive's removal is not followed: {}",
                std::io::Error::last_os_error()
            );
            close(handle);
            return None;
        }
        Some((handle, notify))
    }

    fn close(handle: HANDLE) {
        if !handle.is_null() && handle != INVALID_HANDLE_VALUE {
            // SAFETY: a handle this window opened, closed once.
            unsafe { CloseHandle(handle) };
        }
    }

    /// End the window: each drive's registration ends, and what was let go
    /// of on a drive whose removal is not decided yet is watched again.
    fn end(window: HWND) {
        let drives = with(|cx| std::mem::take(&mut cx.drives)).unwrap_or_default();
        for held in drives {
            if held.drive.letting_go {
                watch::watch_again_on(&held.volume);
            }
            if !held.notify.is_null() {
                // SAFETY: a registration this window made, ended once.
                unsafe { UnregisterDeviceNotification(held.notify) };
            }
            close(held.handle);
        }
        if WINDOW
            .compare_exchange(window as isize, 0, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            watch::unfollow_volumes();
        }
        // SAFETY: this thread's window.
        unsafe { DestroyWindow(window) };
    }

    #[cfg(test)]
    mod tests {
        use std::sync::Arc;
        use std::time::{Duration, Instant};

        use parking_lot::Mutex;
        use skwir_core::cancel::CancelToken;
        use skwir_core::fs::watch;
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            DBT_DEVICEQUERYREMOVE, DBT_DEVICEQUERYREMOVEFAILED, DBT_DEVICEREMOVECOMPLETE,
            DBT_DEVTYP_HANDLE, DEV_BROADCAST_HANDLE, SendMessageW, WM_DEVICECHANGE,
        };

        use super::{HWND, REGISTRATION, WINDOW};
        use crate::places::UnmountNotice;

        /// Wait until `done`, at most a few seconds.
        fn until(what: &str, done: impl Fn() -> bool) {
            let deadline = Instant::now() + Duration::from_secs(10);
            while !done() {
                assert!(Instant::now() < deadline, "{what}");
                std::thread::sleep(Duration::from_millis(20));
            }
        }

        /// The window that follows.
        fn window() -> HWND {
            WINDOW.load(std::sync::atomic::Ordering::Acquire) as HWND
        }

        /// Hand the window a handle's notice, as the system does.
        fn send(event: u32, notify: isize) -> isize {
            let filter = DEV_BROADCAST_HANDLE {
                dbch_size: u32::try_from(std::mem::size_of::<DEV_BROADCAST_HANDLE>()).unwrap(),
                dbch_devicetype: DBT_DEVTYP_HANDLE,
                dbch_hdevnotify: notify as _,
                ..DEV_BROADCAST_HANDLE::default()
            };
            // SAFETY: the window this test started, handed a notice that
            // outlives the call.
            unsafe {
                SendMessageW(
                    window(),
                    WM_DEVICECHANGE,
                    event as usize,
                    (&raw const filter) as isize,
                )
            }
        }

        /// A drive a folder watch is on is registered for, on a real
        /// handle on its root. Asked to remove it, the window tells it
        /// announced, lets go of the watch, closes the handle and answers;
        /// the removal cancelled, the watch is watched again and told to
        /// read its folder, the drive registered anew on a handle opened
        /// again; removed, the watch is told its folder went, and the
        /// registration ends. The notices are handed as Windows hands them,
        /// to the window's procedure: nothing removes a drive here. The
        /// system's drive, which holds the test's folder, is not told apart
        /// from another here.
        #[test]
        fn a_drive_watched_is_registered_and_let_go_of_as_windows_asks() {
            // Wine makes no window without a display, after a while.
            if std::env::var_os("DISPLAY").is_none()
                && skwir_core::system::under_wine("a window without a display")
            {
                return;
            }
            let folder = tempfile::tempdir().unwrap();
            let volume = skwir_core::fs::reach::volume_of(folder.path()).unwrap();
            let told: Arc<Mutex<Vec<UnmountNotice>>> = Arc::default();
            let into = Arc::clone(&told);
            let stop = CancelToken::new();
            assert!(super::follow(
                &stop,
                Arc::new(move |notice| into.lock().push(notice)),
                None,
            ));
            until("the window made", || !window().is_null());
            let changes: Arc<Mutex<Vec<watch::Changes>>> = Arc::default();
            let seen = Arc::clone(&changes);
            let watched = watch::watch(folder.path(), Duration::from_millis(10), &stop, move |c| {
                seen.lock().push(c);
            })
            .unwrap();
            // The registration while its handle is open, 0 otherwise.
            let registration = || {
                // SAFETY: the window this test started; a path that
                // outlives the call.
                unsafe { SendMessageW(window(), REGISTRATION, (&raw const volume) as usize, 0) }
            };
            let deadline = Instant::now() + Duration::from_secs(5);
            while registration() == 0 && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(20));
            }
            let notify = registration();
            if notify == 0 && skwir_core::system::under_wine("a drive's registration") {
                return;
            }
            assert_ne!(notify, 0, "{} registered for", volume.display());
            assert_eq!(
                send(DBT_DEVICEQUERYREMOVE, notify),
                1,
                "the removal granted"
            );
            assert_eq!(
                *told.lock(),
                [UnmountNotice::Announced(vec![volume.clone()])]
            );
            assert_eq!(registration(), 0, "its handle closed");
            changes.lock().clear();
            std::fs::write(folder.path().join("while it's let go of"), b"").unwrap();
            std::thread::sleep(Duration::from_millis(300));
            assert!(changes.lock().is_empty(), "the watch was let go of");
            assert_eq!(send(DBT_DEVICEQUERYREMOVEFAILED, notify), 1);
            assert_eq!(
                told.lock().last(),
                Some(&UnmountNotice::Refused(vec![volume.clone()]))
            );
            let again = registration();
            assert_ne!(again, 0, "registered anew, its handle open");
            until("watched again and told to read its folder", || {
                changes.lock().iter().any(|c| c.rescan)
            });
            changes.lock().clear();
            std::fs::write(folder.path().join("once it stays"), b"").unwrap();
            until("followed again", || {
                changes.lock().iter().any(|c| !c.names.is_empty())
            });
            assert_eq!(send(DBT_DEVICEREMOVECOMPLETE, again), 1);
            until("told its folder went", || {
                changes.lock().iter().any(|c| c.deleted && c.unmounted)
            });
            assert_eq!(registration(), 0, "its registration ended");
            drop(watched);
            stop.cancel();
            until("the window ended", || window().is_null());
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{Act, Drive, Kind, Notice, letters, registers};

    /// A drive watched is registered for; asked to remove it, it is told
    /// announced, let go of and its handle closed; the removal refused, it
    /// is watched again, told refused, and registered anew.
    #[test]
    fn a_removal_asked_then_refused() {
        let mut drive = Drive::default();
        assert_eq!(drive.look(true), Some(Act::Register));
        assert_eq!(drive.look(true), None, "registered once");
        assert_eq!(
            drive.told(Notice::Asked),
            [Act::Announce, Act::LetGo, Act::Close]
        );
        assert_eq!(drive.look(false), None, "its removal decides");
        assert_eq!(
            drive.told(Notice::Kept),
            [Act::Unregister, Act::WatchAgain, Act::Refuse]
        );
        assert_eq!(drive.look(true), Some(Act::Register), "registered anew");
    }

    /// Removed once asked, its registration ends and its watches are told;
    /// pulled out unasked, its handle is closed too.
    #[test]
    fn a_drive_removed() {
        let mut drive = Drive::default();
        drive.look(true);
        drive.told(Notice::Asked);
        assert_eq!(drive.told(Notice::Gone), [Act::Unregister, Act::Gone]);
        assert_eq!(drive, Drive::default());
        let mut pulled = Drive::default();
        pulled.look(true);
        assert_eq!(pulled.told(Notice::Gone), [Act::Unregister, Act::Gone]);
        let mut never = Drive::default();
        assert_eq!(never.told(Notice::Gone), [Act::Gone], "its watches told");
    }

    /// A lock after the removal asked (an eject that dismounts) lets go of
    /// what came since, told once; a lock let go of where nothing was asked
    /// changes nothing.
    #[test]
    fn a_lock_is_asked_once() {
        let mut drive = Drive::default();
        drive.look(true);
        drive.told(Notice::Asked);
        assert_eq!(drive.told(Notice::Asked), [Act::LetGo]);
        let mut idle = Drive::default();
        idle.look(true);
        assert!(idle.told(Notice::Kept).is_empty());
        assert_eq!(idle.look(false), Some(Act::Unregister), "no watch on it");
    }

    #[test]
    fn the_drives_registered_for() {
        assert!(registers(Kind::Removable, false));
        assert!(
            registers(Kind::Fixed, false),
            "a USB disk Windows calls fixed"
        );
        assert!(registers(Kind::Disc, false));
        assert!(!registers(Kind::Fixed, true), "the system's");
        assert!(!registers(Kind::Other, false), "a network's");
        assert_eq!(
            letters(0b1_0000_0100),
            [PathBuf::from("C:\\"), PathBuf::from("I:\\")]
        );
    }
}
