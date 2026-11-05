//! What skwir asks Windows itself (ADR-035): the drives, who the user
//! is, the types files are registered as (ADR-037). Written on Linux,
//! compiled for Windows; not run under Wine for what Wine answers
//! otherwise (its drives are a Linux machine's folders).

// This module is the system's calls on Windows, which take raw pointers
// and handles: each `unsafe` block says why its call is sound, and
// nothing unsafe leaves the module.
#![allow(unsafe_code)]

use std::path::{Path, PathBuf};

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, LocalFree};
use windows_sys::Win32::Security::Authorization::ConvertSidToStringSidW;
use windows_sys::Win32::Security::{GetTokenInformation, TOKEN_QUERY, TOKEN_USER, TokenUser};
use windows_sys::Win32::Storage::FileSystem::{GetDriveTypeW, GetLogicalDrives};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows_sys::Win32::System::WindowsProgramming::{DRIVE_FIXED, DRIVE_REMOVABLE};

/// The roots of the drives on this machine (`C:\`), local ones only: a
/// network drive or a disc can hang whoever asks it, as a network mount
/// can on Linux.
pub fn local_drives() -> Vec<PathBuf> {
    // SAFETY: no arguments; a bit mask of the drive letters in use.
    let mask = unsafe { GetLogicalDrives() };
    (0..26u8)
        .filter(|n| mask & (1 << n) != 0)
        .map(|n| format!("{}:\\", char::from(b'A' + n)))
        .filter(|root| {
            let wide: Vec<u16> = root.encode_utf16().chain([0]).collect();
            // SAFETY: a NUL-terminated root that outlives the call.
            let kind = unsafe { GetDriveTypeW(wide.as_ptr()) };
            matches!(kind, DRIVE_FIXED | DRIVE_REMOVABLE)
        })
        .map(PathBuf::from)
        .collect()
}

/// The user this process runs as, by the SID Windows names them with
/// (`S-1-5-21-…`): what names the user's folder in each Recycle Bin.
pub fn user_sid() -> Option<String> {
    let mut token: HANDLE = std::ptr::null_mut();
    // SAFETY: the current process's pseudo-handle, and an out pointer.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &raw mut token) } == 0 {
        return None;
    }
    let sid = token_sid(token);
    // SAFETY: the token just opened, closed once.
    unsafe { CloseHandle(token) };
    sid
}

/// The user `token` names, by their SID: its `TokenUser`.
pub(crate) fn token_sid(token: HANDLE) -> Option<String> {
    let mut needed = 0u32;
    // SAFETY: asks the size only: a null buffer of length 0.
    unsafe { GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &raw mut needed) };
    // Aligned for the TOKEN_USER at its start.
    let mut buffer = vec![0u64; usize::try_from(needed).ok()?.div_ceil(8)];
    // SAFETY: `buffer` holds `needed` bytes.
    if unsafe {
        GetTokenInformation(
            token,
            TokenUser,
            buffer.as_mut_ptr().cast(),
            needed,
            &raw mut needed,
        )
    } == 0
    {
        return None;
    }
    // SAFETY: the system wrote a TOKEN_USER at the buffer's start, whose
    // SID points into the same buffer, alive for this function.
    let user = unsafe { &*buffer.as_ptr().cast::<TOKEN_USER>() };
    let mut text: *mut u16 = std::ptr::null_mut();
    // SAFETY: a SID from the token, and an out pointer the system fills
    // with a string it allocated.
    if unsafe { ConvertSidToStringSidW(user.User.Sid, &raw mut text) } == 0 {
        return None;
    }
    let mut len = 0;
    // SAFETY: the system's string is NUL-terminated: read up to the NUL.
    while unsafe { *text.add(len) } != 0 {
        len += 1;
    }
    // SAFETY: `len` units before the NUL.
    let sid = String::from_utf16(unsafe { std::slice::from_raw_parts(text, len) }).ok();
    // SAFETY: the string the system allocated, freed once.
    unsafe { LocalFree(text.cast()) };
    sid
}

/// The user's first day of the week, as their locale says it: 0 for
/// Monday to 6 for Sunday (`LOCALE_IFIRSTDAYOFWEEK`).
pub fn first_weekday() -> Option<i8> {
    use windows_sys::Win32::Globalization::{GetLocaleInfoEx, LOCALE_IFIRSTDAYOFWEEK};
    let mut text = [0u16; 4];
    // SAFETY: the user's locale (a null name), and a buffer of the length
    // given.
    let n = unsafe {
        GetLocaleInfoEx(
            std::ptr::null(),
            LOCALE_IFIRSTDAYOFWEEK,
            text.as_mut_ptr(),
            4,
        )
    };
    let n = usize::try_from(n).ok()?.checked_sub(1)?;
    String::from_utf16(text.get(..n)?).ok()?.parse().ok()
}

/// What a drive says of itself: its label (empty when it has none) and
/// its filesystem (`NTFS`, `FAT32`, `exFAT`).
pub fn volume_information(root: &Path) -> Option<(String, String)> {
    let root = wide_path(root);
    let mut label = [0u16; 261];
    let mut filesystem = [0u16; 261];
    // SAFETY: a NUL-terminated root, and two buffers of the lengths
    // given; the other out values are not asked.
    let ok = unsafe {
        windows_sys::Win32::Storage::FileSystem::GetVolumeInformationW(
            root.as_ptr(),
            label.as_mut_ptr(),
            261,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            filesystem.as_mut_ptr(),
            261,
        )
    };
    let text = |units: &[u16]| {
        let end = units.iter().position(|u| *u == 0).unwrap_or(units.len());
        String::from_utf16_lossy(&units[..end])
    };
    (ok != 0).then(|| (text(&label), text(&filesystem)))
}

/// Free (for this user) and total bytes of the volume holding `path`.
pub fn space(path: &Path) -> Option<(u64, u64)> {
    let path = wide_path(path);
    let (mut free, mut total) = (0u64, 0u64);
    // SAFETY: a NUL-terminated path, and out values the call fills.
    let ok = unsafe {
        windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW(
            path.as_ptr(),
            &raw mut free,
            &raw mut total,
            std::ptr::null_mut(),
        )
    };
    (ok != 0 && total > 0).then_some((free, total))
}

/// What a file call on a drive BitLocker keeps locked fails with:
/// `FVE_E_LOCKED_VOLUME`, and `FVE_E_VOLUME_NOT_BOUND` (its key not at
/// hand), as Double Commander reads them.
const LOCKED: [u32; 2] = [0x8031_0000, 0x8031_0017];

/// Whether `code`, a file call's last error, says BitLocker keeps the
/// volume locked.
fn is_locked_error(code: u32) -> bool {
    LOCKED.contains(&code)
}

/// Whether BitLocker keeps the drive at `root` (`E:\`) locked: asking it
/// for its space fails for that. Asks no administrator, as Windows'
/// own ways of reading it do (WMI's needs one).
pub fn drive_locked(root: &Path) -> bool {
    let wide = wide_path(root);
    let (mut free, mut total) = (0u64, 0u64);
    // SAFETY: a NUL-terminated root, and out values the call fills.
    let ok = unsafe {
        windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW(
            wide.as_ptr(),
            &raw mut free,
            &raw mut total,
            std::ptr::null_mut(),
        )
    };
    // SAFETY: no arguments; this thread's last error, the call's.
    ok == 0 && is_locked_error(unsafe { windows_sys::Win32::Foundation::GetLastError() })
}

/// How long a drive is looked at, once Windows' unlock dialog was opened
/// in a program Skwir cannot wait for, before it is left locked.
const UNLOCK_WAIT: std::time::Duration = std::time::Duration::from_secs(120);

/// Unlock the drive `device` (`E:`) BitLocker keeps locked through
/// Windows' own dialog (ADR-088): the drive's Unlock Drive verb
/// (`unlock-bde`, Explorer's, which runs `bdeunlock.exe` as the user and
/// offers to unlock it automatically from then on). Waits for the dialog
/// to close, or `cancel`. Where the drive then is, `None` when it stays
/// locked.
pub fn unlock_drive(
    device: &str,
    cancel: &skwir_core::CancelToken,
) -> anyhow::Result<Option<PathBuf>> {
    let root = PathBuf::from(format!("{}\\", device.trim_end_matches('\\')));
    if !drive_locked(&root) {
        return Ok(Some(root));
    }
    let (tx, rx) = crossbeam_channel::bounded(1);
    let asked = root.clone();
    // A thread of its own: the shell wants its own COM there.
    std::thread::Builder::new()
        .name("skwir-unlock".into())
        .spawn(move || {
            let _ = tx.send(run_verb(&asked, "unlock-bde"));
        })?;
    let cancelled = cancel.subscribe();
    let waited = crossbeam_channel::select! {
        recv(rx) -> ran => ran.map_err(|_| anyhow::anyhow!("Windows' unlock dialog did not answer"))??,
        recv(cancelled.receiver()) -> _ => return Ok(None),
    };
    if !waited {
        let deadline = std::time::Instant::now() + UNLOCK_WAIT;
        while drive_locked(&root) && std::time::Instant::now() < deadline {
            if cancel.is_cancelled() {
                return Ok(None);
            }
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
    }
    Ok((!drive_locked(&root)).then_some(root))
}

/// Run the shell's `verb` on `path` and wait for the program it started
/// to end: whether it could be waited for (a verb a running program
/// carries out gives none to wait for).
fn run_verb(path: &Path, verb: &str) -> anyhow::Result<bool> {
    use ::windows::Win32::System::Com::{
        COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx,
    };
    use windows_sys::Win32::System::Threading::{INFINITE, WaitForSingleObject};
    use windows_sys::Win32::UI::Shell::{
        SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW,
        ShellExecuteExW,
    };
    // SAFETY: this thread's own COM, as ShellExecuteEx asks for, on a
    // thread that does nothing else.
    let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) };
    let file = wide_path(path);
    let verb: Vec<u16> = verb.encode_utf16().chain([0]).collect();
    // SAFETY: a plain C struct, every field of which may be zero.
    let mut info: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
    info.cbSize = u32::try_from(std::mem::size_of::<SHELLEXECUTEINFOW>())?;
    info.fMask = SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI;
    info.lpVerb = verb.as_ptr();
    info.lpFile = file.as_ptr();
    info.nShow = ::windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL.0;
    // SAFETY: `info` filled as the call asks, its strings outliving it.
    if unsafe { ShellExecuteExW(&raw mut info) } == 0 {
        anyhow::bail!(
            "Windows' unlock dialog could not be opened: {}",
            std::io::Error::last_os_error()
        );
    }
    if info.hProcess.is_null() {
        return Ok(false);
    }
    // SAFETY: the process handle the call opened for this thread, waited
    // on, then closed once.
    unsafe {
        WaitForSingleObject(info.hProcess, INFINITE);
        CloseHandle(info.hProcess);
    }
    Ok(true)
}

/// Whether the drive at `root` is one the user plugs in and out.
pub fn is_removable_drive(root: &Path) -> bool {
    let root = wide_path(root);
    // SAFETY: a NUL-terminated root that outlives the call.
    unsafe { GetDriveTypeW(root.as_ptr()) == DRIVE_REMOVABLE }
}

fn wide_path(path: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    path.as_os_str().encode_wide().chain([0]).collect()
}

/// Whether the process `pid` runs: `None` when Windows will not say.
pub fn process_runs(pid: u32) -> Option<bool> {
    use windows_sys::Win32::Foundation::{ERROR_INVALID_PARAMETER, STILL_ACTIVE};
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    // SAFETY: plain values; a handle or null.
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process.is_null() {
        let error = std::io::Error::last_os_error();
        // No such process; any other refusal (another user's) says it is.
        return Some(
            error.raw_os_error().and_then(|c| u32::try_from(c).ok())
                != Some(ERROR_INVALID_PARAMETER),
        );
    }
    let mut code = 0u32;
    // SAFETY: the handle just opened, and an out value.
    let asked = unsafe { GetExitCodeProcess(process, &raw mut code) };
    // SAFETY: the handle just opened, closed once.
    unsafe { CloseHandle(process) };
    (asked != 0).then(|| code == STILL_ACTIVE.cast_unsigned())
}

/// A job object holding a child and everything it starts, which all end
/// when the job is told to or let go of (`KILL_ON_JOB_CLOSE`): Windows'
/// process group (`process::ChildGuard`).
pub struct Job(std::os::windows::io::OwnedHandle);

impl Job {
    /// A job for the running `child`, which it now holds. `None` when
    /// Windows refuses (a child already in a job that allows no other).
    pub fn holding(child: &std::process::Child) -> Option<Job> {
        use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
        use windows_sys::Win32::System::JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
            SetInformationJobObject,
        };
        // SAFETY: no attributes, no name: a new job's handle, or null.
        let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if handle.is_null() {
            return None;
        }
        // SAFETY: a handle just made, owned here alone.
        let job = Job(unsafe { OwnedHandle::from_raw_handle(handle) });
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let size = u32::try_from(size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>()).ok()?;
        // SAFETY: the job's handle, and the record of the size given.
        let set = unsafe {
            SetInformationJobObject(
                job.0.as_raw_handle(),
                JobObjectExtendedLimitInformation,
                (&raw const limits).cast(),
                size,
            )
        };
        // SAFETY: the job's and the running child's handles.
        let assigned =
            unsafe { AssignProcessToJobObject(job.0.as_raw_handle(), child.as_raw_handle()) };
        (set != 0 && assigned != 0).then_some(job)
    }

    /// Let go of the job, leaving what it holds running: a mount's
    /// program that outlives skwir (`process::run_leaving`).
    pub fn release(self) {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::System::JobObjects::{
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
            SetInformationJobObject,
        };
        // No limit: closing the job ends nothing.
        let limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        let Ok(size) = u32::try_from(size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>()) else {
            return;
        };
        // SAFETY: the job's handle, and the record of the size given.
        unsafe {
            SetInformationJobObject(
                self.0.as_raw_handle(),
                JobObjectExtendedLimitInformation,
                (&raw const limits).cast(),
                size,
            )
        };
    }

    /// The processes the job holds, by id.
    pub fn processes(&self) -> Vec<u32> {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::System::JobObjects::{
            JOBOBJECT_BASIC_PROCESS_ID_LIST, JobObjectBasicProcessIdList, QueryInformationJobObject,
        };
        // The record's head, then room for 1024 ids, aligned as it is.
        let mut room = vec![0usize; 2 + 1024];
        let Ok(size) = u32::try_from(room.len() * size_of::<usize>()) else {
            return Vec::new();
        };
        // SAFETY: the job's handle, and a buffer of the size given, aligned
        // for the record, which the call fills.
        let read = unsafe {
            QueryInformationJobObject(
                self.0.as_raw_handle(),
                JobObjectBasicProcessIdList,
                room.as_mut_ptr().cast(),
                size,
                std::ptr::null_mut(),
            )
        };
        if read == 0 {
            return Vec::new();
        }
        let list = room.as_ptr().cast::<JOBOBJECT_BASIC_PROCESS_ID_LIST>();
        // SAFETY: the record the call filled, its ids within the buffer
        // (at most what it holds room for).
        let ids = unsafe {
            let count = ((*list).NumberOfProcessIdsInList as usize).min(room.len() - 1);
            std::slice::from_raw_parts(
                std::ptr::addr_of!((*list).ProcessIdList).cast::<usize>(),
                count,
            )
        };
        ids.iter()
            .filter_map(|&id| u32::try_from(id).ok())
            .collect()
    }

    /// End everything in the job.
    pub fn stop(&self) {
        use std::os::windows::io::AsRawHandle;
        // SAFETY: the job's handle; an exit code for what it ends.
        unsafe {
            windows_sys::Win32::System::JobObjects::TerminateJobObject(self.0.as_raw_handle(), 1)
        };
    }
}

/// Hold the processes `pids` where they are, each of their threads
/// suspended, or let them go on (`held` false): Windows has no `SIGSTOP`.
/// A thread started meanwhile is not held.
pub fn hold(pids: &[u32], held: bool) {
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
    };
    use windows_sys::Win32::System::Threading::{
        OpenThread, ResumeThread, SuspendThread, THREAD_SUSPEND_RESUME,
    };
    if pids.is_empty() {
        return;
    }
    // SAFETY: a snapshot of every thread; its handle, or the invalid one.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
    if snapshot == INVALID_HANDLE_VALUE || snapshot.is_null() {
        return;
    }
    // SAFETY: a handle just made, owned here alone.
    let snapshot = unsafe { OwnedHandle::from_raw_handle(snapshot) };
    let Ok(size) = u32::try_from(size_of::<THREADENTRY32>()) else {
        return;
    };
    let mut entry = THREADENTRY32 {
        dwSize: size,
        ..THREADENTRY32::default()
    };
    // SAFETY: the snapshot's handle, and a record of the size it says.
    let mut more = unsafe { Thread32First(snapshot.as_raw_handle(), &raw mut entry) } != 0;
    while more {
        if pids.contains(&entry.th32OwnerProcessID) {
            // SAFETY: a thread's id; its handle, or null.
            let thread = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID) };
            if !thread.is_null() {
                // SAFETY: a handle just made, owned here alone, closed
                // when dropped.
                let thread = unsafe { OwnedHandle::from_raw_handle(thread) };
                // SAFETY: a thread's handle opened to suspend and resume it.
                unsafe {
                    if held {
                        SuspendThread(thread.as_raw_handle());
                    } else {
                        ResumeThread(thread.as_raw_handle());
                    }
                }
            }
        }
        // SAFETY: as above.
        more = unsafe { Thread32Next(snapshot.as_raw_handle(), &raw mut entry) } != 0;
    }
}

/// Named pipes only the user opens (`crate::askpass`): each instance
/// made with a DACL naming the user alone, local clients alone.
pub mod pipe {
    use std::ffi::OsStr;
    use std::io;
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};

    use windows_sys::Win32::Foundation::{
        ERROR_PIPE_CONNECTED, GetLastError, INVALID_HANDLE_VALUE, LocalFree,
    };
    use windows_sys::Win32::Security::Authorization::{
        ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
    };
    use windows_sys::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_DUPLEX,
    };
    use windows_sys::Win32::System::Pipes::{
        ConnectNamedPipe, CreateNamedPipeW, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS,
        PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
    };

    /// The size of each way's buffer: a frame of a question or an answer.
    const BUFFER: u32 = 64 * 1024;

    /// A new instance of the pipe `name` (`\\.\pipe\…`), waiting for a
    /// client: the pipe's first where `first` (it fails where one of that
    /// name is there, made by anyone).
    ///
    /// # Errors
    /// When Windows will not say who the user is, or makes no pipe.
    pub fn instance(name: &OsStr, first: bool) -> io::Result<OwnedHandle> {
        let sid = super::user_sid()
            .ok_or_else(|| io::Error::other("Windows does not say who the user is"))?;
        // Protected, its one entry the user's: all access, for them alone.
        let sddl: Vec<u16> = format!("D:P(A;;GA;;;{sid})")
            .encode_utf16()
            .chain([0])
            .collect();
        let mut descriptor: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
        // SAFETY: a NUL-terminated SDDL string, and an out pointer the
        // system fills with a descriptor it allocated; no size asked.
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                SDDL_REVISION_1,
                &raw mut descriptor,
                std::ptr::null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        let attributes = SECURITY_ATTRIBUTES {
            nLength: u32::try_from(std::mem::size_of::<SECURITY_ATTRIBUTES>()).unwrap_or(u32::MAX),
            lpSecurityDescriptor: descriptor,
            bInheritHandle: 0,
        };
        let wide: Vec<u16> = name.encode_wide().chain([0]).collect();
        let alone = if first {
            FILE_FLAG_FIRST_PIPE_INSTANCE
        } else {
            0
        };
        // SAFETY: a NUL-terminated name and the attributes above, alive
        // for the call; plain values otherwise.
        let handle = unsafe {
            CreateNamedPipeW(
                wide.as_ptr(),
                PIPE_ACCESS_DUPLEX | alone,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                PIPE_UNLIMITED_INSTANCES,
                BUFFER,
                BUFFER,
                0,
                &raw const attributes,
            )
        };
        let failed = io::Error::last_os_error();
        // SAFETY: the descriptor the system allocated, freed once, after
        // the call that read it.
        unsafe { LocalFree(descriptor) };
        if handle == INVALID_HANDLE_VALUE {
            return Err(failed);
        }
        // SAFETY: a handle just made, owned from here on.
        Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
    }

    /// Wait for a client of `pipe`, an instance [`instance`] made: the
    /// connection, read and written as a file, closed when dropped.
    ///
    /// # Errors
    /// When the wait fails.
    pub fn accept(pipe: OwnedHandle) -> io::Result<std::fs::File> {
        // SAFETY: a pipe instance's handle, owned by `pipe`; no overlap.
        let connected = unsafe { ConnectNamedPipe(pipe.as_raw_handle(), std::ptr::null_mut()) };
        // A client that came before the wait is connected as well.
        // SAFETY: no arguments.
        if connected == 0 && unsafe { GetLastError() } != ERROR_PIPE_CONNECTED {
            return Err(io::Error::last_os_error());
        }
        Ok(std::fs::File::from(pipe))
    }
}

/// The Credential Manager's generic credentials: skwir's secrets
/// (`crate::secrets`), each named `target`, its bytes the secret.
pub mod credentials {
    use std::io;

    use windows_sys::Win32::Foundation::ERROR_NOT_FOUND;
    use windows_sys::Win32::Security::Credentials::{
        CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC, CREDENTIALW, CredDeleteW, CredEnumerateW,
        CredFree, CredReadW, CredWriteW,
    };

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain([0]).collect()
    }

    fn not_found(error: &io::Error) -> bool {
        error.raw_os_error().and_then(|c| u32::try_from(c).ok()) == Some(ERROR_NOT_FOUND)
    }

    /// Keep `secret` as `target`, replacing what was kept; `label` is the
    /// comment the Credential Manager shows.
    pub fn write(target: &str, label: &str, secret: &[u8]) -> io::Result<()> {
        let (mut target, mut label) = (wide(target), wide(label));
        let size = u32::try_from(secret.len()).map_err(|_| io::ErrorKind::InvalidInput)?;
        let credential = CREDENTIALW {
            Type: CRED_TYPE_GENERIC,
            TargetName: target.as_mut_ptr(),
            Comment: label.as_mut_ptr(),
            CredentialBlobSize: size,
            CredentialBlob: secret.as_ptr().cast_mut(),
            Persist: CRED_PERSIST_LOCAL_MACHINE,
            ..CREDENTIALW::default()
        };
        // SAFETY: every pointer is to a buffer alive for the call, which
        // copies them; the blob is only read.
        if unsafe { CredWriteW(&raw const credential, 0) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    /// The secret kept as `target`, if any.
    pub fn read(target: &str) -> io::Result<Option<Vec<u8>>> {
        let target = wide(target);
        let mut found: *mut CREDENTIALW = std::ptr::null_mut();
        // SAFETY: a NUL-terminated name, and an out pointer the system
        // fills with a credential it allocated.
        if unsafe { CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &raw mut found) } == 0 {
            let error = io::Error::last_os_error();
            return if not_found(&error) {
                Ok(None)
            } else {
                Err(error)
            };
        }
        // SAFETY: the credential the system returned (read where it is,
        // whatever its alignment), whose blob is `CredentialBlobSize`
        // bytes; copied before it is freed, once.
        let secret = unsafe {
            let c = found.read_unaligned();
            let bytes = if c.CredentialBlob.is_null() {
                Vec::new()
            } else {
                std::slice::from_raw_parts(c.CredentialBlob, c.CredentialBlobSize as usize).to_vec()
            };
            CredFree(found.cast());
            bytes
        };
        Ok(Some(secret))
    }

    /// Forget every secret whose name `filter` matches (a trailing `*`
    /// for every name it starts), or the one named so.
    pub fn delete(filter: &str) -> io::Result<()> {
        if !filter.ends_with('*') {
            let target = wide(filter);
            // SAFETY: a NUL-terminated name.
            if unsafe { CredDeleteW(target.as_ptr(), CRED_TYPE_GENERIC, 0) } == 0 {
                let error = io::Error::last_os_error();
                if !not_found(&error) {
                    return Err(error);
                }
            }
            return Ok(());
        }
        let wide_filter = wide(filter);
        let mut count = 0u32;
        let mut list: *mut *mut CREDENTIALW = std::ptr::null_mut();
        // SAFETY: a NUL-terminated filter, and out pointers the system
        // fills with an array it allocated.
        if unsafe { CredEnumerateW(wide_filter.as_ptr(), 0, &raw mut count, &raw mut list) } == 0 {
            let error = io::Error::last_os_error();
            return if not_found(&error) {
                Ok(())
            } else {
                Err(error)
            };
        }
        // SAFETY: `count` credentials the system returned, their names
        // NUL-terminated, read where they are whatever their alignment
        // (Wine packs them unaligned); read before the array is freed,
        // once.
        let names: Vec<String> = unsafe {
            let names = (0..count as usize)
                .filter_map(|i| {
                    let credential = list.add(i).read_unaligned().read_unaligned();
                    let name = credential.TargetName;
                    if name.is_null() {
                        return None;
                    }
                    let mut units = Vec::new();
                    while let unit @ 1.. = name.add(units.len()).read_unaligned() {
                        units.push(unit);
                    }
                    String::from_utf16(&units).ok()
                })
                .collect();
            CredFree(list.cast());
            names
        };
        for name in names {
            delete(&name)?;
        }
        Ok(())
    }
}

/// What the registry's classes say (`HKEY_CLASSES_ROOT`: the user's over
/// the machine's) and what the shell makes of them: the types files are
/// registered as (ADR-037).
pub mod classes {
    use std::ptr::{null, null_mut};

    use windows_sys::Win32::Foundation::{ERROR_SUCCESS, S_OK};
    use windows_sys::Win32::System::Registry::{HKEY_CLASSES_ROOT, RRF_RT_REG_SZ, RegGetValueW};
    use windows_sys::Win32::UI::Shell::{
        ASSOCF_INIT_IGNOREUNKNOWN, ASSOCSTR_FRIENDLYDOCNAME, AssocQueryStringW,
    };

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain([0]).collect()
    }

    /// The text up to the first NUL, trimmed; `None` when that is empty.
    fn text(units: &[u16]) -> Option<String> {
        let units = units.split(|u| *u == 0).next()?;
        let text = String::from_utf16(units).ok()?;
        let text = text.trim();
        (!text.is_empty()).then(|| text.to_owned())
    }

    /// The text value `name` (empty: the key's default) of `key` under
    /// `HKEY_CLASSES_ROOT` (`.png`, `Content Type`); `None` where there is
    /// none, or it is no text.
    pub fn value(key: &str, name: &str) -> Option<String> {
        let (key, name) = (wide(key), wide(name));
        let mut bytes: u32 = 0;
        // SAFETY: a NUL-terminated key and name that outlive the call; no
        // buffer, so the registry only says how many bytes the value takes.
        let asked = unsafe {
            RegGetValueW(
                HKEY_CLASSES_ROOT,
                key.as_ptr(),
                name.as_ptr(),
                RRF_RT_REG_SZ,
                null_mut(),
                null_mut(),
                &raw mut bytes,
            )
        };
        if asked != ERROR_SUCCESS || bytes == 0 {
            return None;
        }
        let mut units = vec![0u16; usize::try_from(bytes).ok()?.div_ceil(2)];
        let mut size = u32::try_from(units.len() * 2).ok()?;
        // SAFETY: as above, with a buffer of `size` bytes, which the
        // registry writes at most.
        let read = unsafe {
            RegGetValueW(
                HKEY_CLASSES_ROOT,
                key.as_ptr(),
                name.as_ptr(),
                RRF_RT_REG_SZ,
                null_mut(),
                units.as_mut_ptr().cast(),
                &raw mut size,
            )
        };
        if read != ERROR_SUCCESS {
            return None;
        }
        units.truncate(usize::try_from(size).ok()? / 2);
        text(&units)
    }

    /// What Explorer calls files named `*.extension` (`.txt`: "Text
    /// Document"), in the user's language; `None` for an extension
    /// nothing registered.
    pub fn friendly_name(extension: &str) -> Option<String> {
        let extension = wide(extension);
        let mut len: u32 = 0;
        // SAFETY: a NUL-terminated extension that outlives the call; no
        // buffer, so the shell only says how many units the name takes.
        unsafe {
            AssocQueryStringW(
                ASSOCF_INIT_IGNOREUNKNOWN,
                ASSOCSTR_FRIENDLYDOCNAME,
                extension.as_ptr(),
                null(),
                null_mut(),
                &raw mut len,
            )
        };
        if len == 0 {
            return None;
        }
        let mut units = vec![0u16; usize::try_from(len).ok()?];
        // SAFETY: as above, with a buffer of `len` units, which the shell
        // writes at most.
        let answered = unsafe {
            AssocQueryStringW(
                ASSOCF_INIT_IGNOREUNKNOWN,
                ASSOCSTR_FRIENDLYDOCNAME,
                extension.as_ptr(),
                null(),
                units.as_mut_ptr(),
                &raw mut len,
            )
        };
        if answered != S_OK {
            return None;
        }
        text(&units)
    }

    /// Set a class's text value for a test: in Wine's scratch prefix
    /// alone, never a user's registry. Through `HKEY_CLASSES_ROOT`, which
    /// Wine keeps as the machine's classes alone (it does not merge the
    /// user's in, as Windows does).
    #[cfg(test)]
    pub fn set(key: &str, name: &str, value: &str) {
        use windows_sys::Win32::System::Registry::{REG_SZ, RegSetKeyValueW};
        let key = wide(key);
        let name = wide(name);
        let data = wide(value);
        // SAFETY: NUL-terminated key, name and data that outlive the call,
        // the data's length in bytes, its NUL included.
        let set = unsafe {
            RegSetKeyValueW(
                HKEY_CLASSES_ROOT,
                key.as_ptr(),
                name.as_ptr(),
                REG_SZ,
                data.as_ptr().cast(),
                u32::try_from(data.len() * 2).unwrap(),
            )
        };
        assert_eq!(set, ERROR_SUCCESS, "set {value:?}");
    }

    /// Remove a class `set` made, with what is under it.
    #[cfg(test)]
    pub fn remove(key: &str) {
        use windows_sys::Win32::System::Registry::RegDeleteTreeW;
        let key = wide(key);
        // SAFETY: a NUL-terminated key that outlives the call.
        unsafe { RegDeleteTreeW(HKEY_CLASSES_ROOT, key.as_ptr()) };
    }
}

/// What Windows says of text (ADR-117): the family it writes messages
/// in, and the Text size chosen in Settings › Accessibility, each read
/// where Windows keeps it and followed there.
pub mod style {
    use std::ptr::{null, null_mut};

    use windows_sys::Win32::Foundation::{CloseHandle, ERROR_SUCCESS, HANDLE, WAIT_OBJECT_0};
    use windows_sys::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_NOTIFY, REG_NOTIFY_CHANGE_LAST_SET, RRF_RT_REG_DWORD,
        RegCloseKey, RegGetValueW, RegNotifyChangeKeyValue, RegOpenKeyExW,
    };
    use windows_sys::Win32::System::Threading::{
        CreateEventW, INFINITE, SetEvent, WaitForMultipleObjects,
    };

    /// Where the Text size is kept, as a percentage (100 to 225).
    pub const ACCESSIBILITY: &str = r"Software\Microsoft\Accessibility";
    const TEXT_SCALE: &str = "TextScaleFactor";
    /// Where the fonts of messages, menus and captions are kept.
    const WINDOW_METRICS: &str = r"Control Panel\Desktop\WindowMetrics";
    /// Where Windows keeps the accent the user chose (its palette of
    /// shades, and the window frames' copy of it), whether a contrast
    /// theme is on, and the colors it asks applications to draw with:
    /// watched, so a change is read again (ADR-071).
    pub const ACCENT: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\Accent";
    pub const DWM: &str = r"Software\Microsoft\Windows\DWM";
    const HIGH_CONTRAST: &str = r"Control Panel\Accessibility\HighContrast";
    const SYSTEM_COLORS: &str = r"Control Panel\Colors";

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain([0]).collect()
    }

    /// The family Windows writes its messages in (`Segoe UI`): the message
    /// font of its non-client metrics, as its own dialogs take it.
    pub fn message_font() -> Option<String> {
        use ::windows::Win32::UI::WindowsAndMessaging::{
            NONCLIENTMETRICSW, SPI_GETNONCLIENTMETRICS, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
            SystemParametersInfoW,
        };
        let size = u32::try_from(std::mem::size_of::<NONCLIENTMETRICSW>()).ok()?;
        let mut metrics = NONCLIENTMETRICSW {
            cbSize: size,
            ..NONCLIENTMETRICSW::default()
        };
        // SAFETY: a structure of the size it says, which the call fills
        // and which outlives it.
        unsafe {
            SystemParametersInfoW(
                SPI_GETNONCLIENTMETRICS,
                size,
                Some((&raw mut metrics).cast()),
                SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
            )
        }
        .ok()?;
        let face = &metrics.lfMessageFont.lfFaceName;
        let face = face.split(|u| *u == 0).next()?;
        let family = String::from_utf16(face).ok()?;
        let family = family.trim();
        (!family.is_empty()).then(|| family.to_owned())
    }

    /// The accent the user chose (Settings › Personalization › Colors), as
    /// `UISettings` gives it to applications and gpui reads the look, in
    /// sRGB from 0 to 1; `None` where it gives none.
    pub fn accent() -> Option<[f32; 3]> {
        use ::windows::UI::ViewManagement::{UIColorType, UISettings};
        use ::windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};
        // WinRT on a thread of skwir's own: the multithreaded apartment,
        // joined once and kept while the thread lives (as the shell's
        // thumbnails' threads do).
        // SAFETY: no reserved argument; a failure (another apartment
        // joined already) leaves the thread as it was.
        let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        let color = UISettings::new()
            .and_then(|settings| settings.GetColorValue(UIColorType::Accent))
            .ok()?;
        Some([color.R, color.G, color.B].map(|c| f32::from(c) / 255.))
    }

    /// Whether a contrast theme is on (`SPI_GETHIGHCONTRAST`).
    pub fn high_contrast() -> bool {
        use ::windows::Win32::UI::Accessibility::{HCF_HIGHCONTRASTON, HIGHCONTRASTW};
        use ::windows::Win32::UI::WindowsAndMessaging::{
            SPI_GETHIGHCONTRAST, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW,
        };
        let Ok(size) = u32::try_from(std::mem::size_of::<HIGHCONTRASTW>()) else {
            return false;
        };
        let mut contrast = HIGHCONTRASTW {
            cbSize: size,
            ..HIGHCONTRASTW::default()
        };
        // SAFETY: a structure of the size it says, which the call fills
        // and which outlives it.
        let read = unsafe {
            SystemParametersInfoW(
                SPI_GETHIGHCONTRAST,
                size,
                Some((&raw mut contrast).cast()),
                SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
            )
        };
        read.is_ok() && contrast.dwFlags.contains(HCF_HIGHCONTRASTON)
    }

    /// The colors a contrast theme asks applications to draw with
    /// (`GetSysColor`), each `0x00BBGGRR`: the window's ground and text,
    /// a button's face, text grayed, and what is highlighted and its text.
    pub fn contrast_colors() -> [u32; 6] {
        use ::windows::Win32::Graphics::Gdi::{
            COLOR_BTNFACE, COLOR_GRAYTEXT, COLOR_HIGHLIGHT, COLOR_HIGHLIGHTTEXT, COLOR_WINDOW,
            COLOR_WINDOWTEXT, GetSysColor,
        };
        [
            COLOR_WINDOW,
            COLOR_WINDOWTEXT,
            COLOR_BTNFACE,
            COLOR_GRAYTEXT,
            COLOR_HIGHLIGHT,
            COLOR_HIGHLIGHTTEXT,
        ]
        // SAFETY: a color index Windows defines; the call only reads.
        .map(|index| unsafe { GetSysColor(index) })
    }

    /// The Text size as a factor (1.25 for 125 %); `None` where none was
    /// chosen, as Windows keeps none until the slider moves.
    pub fn text_scale() -> Option<f64> {
        dword(ACCESSIBILITY, TEXT_SCALE).and_then(scale_of)
    }

    /// A Text size Windows keeps (a percentage) as a factor; `None` out of
    /// the slider's range.
    pub fn scale_of(percent: u32) -> Option<f64> {
        (100..=225)
            .contains(&percent)
            .then(|| f64::from(percent) / 100.)
    }

    /// The number `name` of `key` under the user's registry.
    fn dword(key: &str, name: &str) -> Option<u32> {
        let (key, name) = (wide(key), wide(name));
        let mut value: u32 = 0;
        let mut size = u32::try_from(std::mem::size_of::<u32>()).ok()?;
        // SAFETY: a NUL-terminated key and name and a number of the size
        // said, all outliving the call.
        let read = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                name.as_ptr(),
                RRF_RT_REG_DWORD,
                null_mut(),
                (&raw mut value).cast(),
                &raw mut size,
            )
        };
        (read == ERROR_SUCCESS).then_some(value)
    }

    /// Set the Text size for a test: in Wine's scratch prefix alone, never
    /// a user's registry.
    #[cfg(test)]
    pub fn set_text_scale(percent: u32) {
        use windows_sys::Win32::System::Registry::{REG_DWORD, RegSetKeyValueW};
        let (key, name) = (wide(ACCESSIBILITY), wide(TEXT_SCALE));
        // SAFETY: a NUL-terminated key and name and a number of the size
        // said, all outliving the call.
        let set = unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                name.as_ptr(),
                REG_DWORD,
                (&raw const percent).cast(),
                4,
            )
        };
        assert_eq!(set, ERROR_SUCCESS, "set {percent}");
    }

    /// Remove the Text size `set_text_scale` set.
    #[cfg(test)]
    pub fn remove_text_scale() {
        use windows_sys::Win32::System::Registry::RegDeleteKeyValueW;
        let (key, name) = (wide(ACCESSIBILITY), wide(TEXT_SCALE));
        // SAFETY: a NUL-terminated key and name outliving the call.
        unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr()) };
    }

    /// An event of its own, closed when dropped.
    struct Event(HANDLE);

    impl Event {
        fn new(manual: bool) -> Option<Self> {
            // SAFETY: no attributes nor name; a handle or null.
            let handle = unsafe { CreateEventW(null(), i32::from(manual), 0, null()) };
            (!handle.is_null()).then_some(Event(handle))
        }
    }

    impl Drop for Event {
        fn drop(&mut self) {
            // SAFETY: a handle this owns, closed once.
            unsafe { CloseHandle(self.0) };
        }
    }

    /// A key of the user's registry opened to be told of its changes.
    struct Watched {
        key: HKEY,
        changed: Event,
    }

    impl Watched {
        fn open(path: &str) -> Option<Self> {
            let path = wide(path);
            let mut key: HKEY = null_mut();
            // SAFETY: a NUL-terminated path outliving the call; the key is
            // written on success.
            let opened = unsafe {
                RegOpenKeyExW(
                    HKEY_CURRENT_USER,
                    path.as_ptr(),
                    0,
                    KEY_NOTIFY,
                    &raw mut key,
                )
            };
            if opened != ERROR_SUCCESS {
                return None;
            }
            let watched = Watched {
                key,
                changed: Event::new(false)?,
            };
            watched.arm().then_some(watched)
        }

        /// Ask to be told, once, of the next change of a value of it.
        fn arm(&self) -> bool {
            // SAFETY: a key and an event this owns, both open while the
            // registry may signal it.
            let armed = unsafe {
                RegNotifyChangeKeyValue(self.key, 0, REG_NOTIFY_CHANGE_LAST_SET, self.changed.0, 1)
            };
            armed == ERROR_SUCCESS
        }
    }

    impl Drop for Watched {
        fn drop(&mut self) {
            // SAFETY: a key this owns, closed once; its pending notice ends
            // with it.
            unsafe { RegCloseKey(self.key) };
        }
    }

    /// Call `changed` once its keys are watched, then each time the Text
    /// size, the window metrics, the accent, the contrast theme or its
    /// colors change, until `stop`, waiting on the registry's notices (no
    /// timer): what changed while `changed` read is told again, as each
    /// key is asked again before. Calls it once and returns when no key
    /// can be watched; a key Windows has not made yet (no accent ever
    /// chosen) is left out, as is one Windows will not tell of again.
    pub fn follow(stop: &skwir_core::CancelToken, mut changed: impl FnMut()) {
        let watched: Vec<Watched> = [
            ACCESSIBILITY,
            WINDOW_METRICS,
            ACCENT,
            DWM,
            HIGH_CONTRAST,
            SYSTEM_COLORS,
        ]
        .into_iter()
        .filter_map(Watched::open)
        .collect();
        let Some(stopped) = Event::new(true).filter(|_| !watched.is_empty()) else {
            tracing::debug!("Windows' style is read, not followed");
            changed();
            return;
        };
        // The token's signal set as the event, from a thread that waits on
        // it (a handle crosses as a number): a child of it, which a failure
        // here cancels too.
        let stop = stop.child();
        let signal = stop.subscribe();
        let handle = stopped.0 as usize;
        let setter = std::thread::Builder::new()
            .name("skwir-style-stop".into())
            .spawn(move || {
                let _ = signal.receiver().recv();
                // SAFETY: the event outlives this thread, joined before it
                // is closed.
                unsafe { SetEvent(handle as HANDLE) };
            });
        let Ok(setter) = setter else {
            changed();
            return;
        };
        let mut events: Vec<HANDLE> = watched.iter().map(|w| w.changed.0).collect();
        events.push(stopped.0);
        let count = u32::try_from(events.len()).unwrap_or(u32::MAX);
        // Read once every key is watched: a change made meanwhile is told.
        changed();
        while !stop.is_cancelled() {
            // SAFETY: open events this owns, as many as said.
            let woke = unsafe { WaitForMultipleObjects(count, events.as_ptr(), 0, INFINITE) };
            let Some(which) = woke
                .checked_sub(WAIT_OBJECT_0)
                .and_then(|i| usize::try_from(i).ok())
                .and_then(|i| watched.get(i))
            else {
                break;
            };
            // Asked again before reading, so a change made while reading
            // is told. Refused, that key is not followed any more.
            if !which.arm() {
                tracing::debug!("a key of Windows' style is no longer followed");
            }
            changed();
        }
        // Ended by a failure, the setter is let go all the same.
        stop.cancel();
        let _ = setter.join();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_system_says_who_the_user_is_their_drives_and_their_week() {
        let sid = user_sid().expect("the user's SID");
        assert!(sid.starts_with("S-1-"), "{sid}");
        let drives = local_drives();
        assert!(drives.iter().any(|d| d.as_os_str() == "C:\\"), "{drives:?}");
        assert!(first_weekday().is_some_and(|d| (0..7).contains(&d)));
        let (_, filesystem) = volume_information(Path::new("C:\\")).expect("C:'s volume");
        assert!(!filesystem.is_empty());
        let (free, total) = space(Path::new("C:\\")).expect("C:'s space");
        assert!(free <= total);
        assert_eq!(process_runs(std::process::id()), Some(true));
        // Read, so not locked (Wine has no BitLocker: it says so of none).
        assert!(!drive_locked(Path::new("C:\\")));
    }

    /// BitLocker's two answers for a drive locked; any other failure (an
    /// empty card reader's `ERROR_NOT_READY`) is not one.
    #[test]
    fn a_drive_bitlocker_keeps_locked_is_told_by_its_error() {
        assert!(is_locked_error(0x8031_0000));
        assert!(is_locked_error(0x8031_0017));
        assert!(!is_locked_error(21), "ERROR_NOT_READY");
        assert!(!is_locked_error(5), "ERROR_ACCESS_DENIED");
    }

    #[test]
    fn windows_names_the_font_of_its_messages_and_keeps_a_text_size_in_range() {
        let family = style::message_font().expect("the message font");
        assert!(!family.is_empty());
        assert_eq!(style::scale_of(100), Some(1.));
        assert_eq!(style::scale_of(125), Some(1.25));
        assert_eq!(style::scale_of(225), Some(2.25));
        assert_eq!(style::scale_of(99), None);
        assert_eq!(style::scale_of(226), None);
    }

    /// A Text size chosen is followed as Windows keeps it. In the Wine
    /// prefix of the tests alone, never a user's registry.
    #[test]
    fn a_text_size_chosen_is_followed_in_the_registry() {
        struct Removed;
        impl Drop for Removed {
            fn drop(&mut self) {
                style::remove_text_scale();
            }
        }
        if std::env::var_os("SKWIR_TEST_UNDER_WINE").is_none() {
            eprintln!("skipped: writes the registry; only in Wine's scratch prefix");
            return;
        }
        let _removed = Removed;
        style::set_text_scale(150);
        assert_eq!(style::text_scale(), Some(1.5));
        let stop = skwir_core::CancelToken::new();
        let (told, heard) = std::sync::mpsc::channel();
        let following = {
            let stop = stop.clone();
            std::thread::spawn(move || style::follow(&stop, || told.send(()).unwrap()))
        };
        // Told once its keys are watched, before anything changes.
        heard
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("told once watched");
        // Written until it is told: the watch may not be set up yet.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let mut percent = 150;
        loop {
            percent = if percent == 175 { 200 } else { 175 };
            style::set_text_scale(percent);
            if heard
                .recv_timeout(std::time::Duration::from_millis(200))
                .is_ok()
            {
                break;
            }
            assert!(std::time::Instant::now() < deadline, "never told");
        }
        assert_eq!(style::text_scale(), Some(f64::from(percent) / 100.));
        stop.cancel();
        following.join().unwrap();
    }

    /// In the Wine prefix of the tests alone (`just test-windows`), never
    /// a user's Credential Manager.
    #[test]
    fn a_secret_goes_in_and_comes_out_of_the_credential_manager() {
        if std::env::var_os("SKWIR_TEST_UNDER_WINE").is_none() {
            eprintln!("skipped: only in Wine's scratch prefix, never a user's keyring");
            return;
        }
        let target = "skwir-test/map-tiles.servers/a1/key";
        credentials::delete("skwir-test/*").unwrap();
        assert_eq!(credentials::read(target).unwrap(), None);
        credentials::write(target, "skwir: a test", b"it's secret").unwrap();
        credentials::write("skwir-test/map-tiles.servers/a1/other", "x", b"2").unwrap();
        assert_eq!(
            credentials::read(target).unwrap().as_deref(),
            Some(b"it's secret".as_slice())
        );
        credentials::delete("skwir-test/map-tiles.servers/a1/*").unwrap();
        assert_eq!(credentials::read(target).unwrap(), None);
    }
}
