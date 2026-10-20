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

fn token_sid(token: HANDLE) -> Option<String> {
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

    /// End everything in the job.
    pub fn stop(&self) {
        use std::os::windows::io::AsRawHandle;
        // SAFETY: the job's handle; an exit code for what it ends.
        unsafe {
            windows_sys::Win32::System::JobObjects::TerminateJobObject(self.0.as_raw_handle(), 1)
        };
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
