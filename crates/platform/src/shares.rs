//! A server's share connected as the system connects it, off Linux
//! (ADR-077: "Windows' shares, macOS's own mounts"). Windows reaches a
//! share by its UNC path (`\\server\share`; a WebDAV folder through its
//! `WebClient` service, `\\server@SSL@443\DavWWWRoot\folder`), connected
//! with `WNetAddConnection2W`; macOS mounts a URL (`smb://`, `afp://`,
//! `nfs://`, a WebDAV folder's `http(s)://`) through `NetFS`, under
//! /Volumes. A password goes in the call alone, never on a command line,
//! and is kept by no one here. Compiled only: Linux runs none of it, and
//! Wine connects no share.

use std::path::{Path, PathBuf};

/// Why a share was not connected.
#[derive(Debug)]
pub enum Refused {
    /// The server refused the login: asked again, with a password.
    Login,
    /// Anything else, in words.
    Other(anyhow::Error),
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refused::Login => f.write_str("the server refused the login"),
            Refused::Other(error) => write!(f, "{error:#}"),
        }
    }
}

impl std::error::Error for Refused {}

/// Connect the share at the UNC path `unc` (`\\server\share`), as `user`
/// with `password` where given, else as the user is logged in: no drive
/// letter taken, kept for this session alone. Connected already, it is.
///
/// # Errors
/// [`Refused::Login`] when the server refused the login; why otherwise.
#[cfg(windows)]
#[allow(unsafe_code)] // Win32 calls on strings that outlive them.
pub fn connect_unc(unc: &Path, user: Option<&str>, password: Option<&str>) -> Result<(), Refused> {
    use windows_sys::Win32::Foundation::{
        ERROR_ACCESS_DENIED, ERROR_INVALID_PASSWORD, ERROR_LOGON_FAILURE, NO_ERROR,
    };
    use windows_sys::Win32::NetworkManagement::WNet::{
        CONNECT_TEMPORARY, NETRESOURCEW, RESOURCETYPE_DISK, WNetAddConnection2W,
    };
    let wide = |text: &std::ffi::OsStr| -> Vec<u16> {
        use std::os::windows::ffi::OsStrExt;
        text.encode_wide().chain(std::iter::once(0)).collect()
    };
    let mut remote = wide(unc.as_os_str());
    let user = user.map(|u| wide(std::ffi::OsStr::new(u)));
    let password = password.map(|p| wide(std::ffi::OsStr::new(p)));
    let resource = NETRESOURCEW {
        dwScope: 0,
        dwType: RESOURCETYPE_DISK,
        dwDisplayType: 0,
        dwUsage: 0,
        lpLocalName: std::ptr::null_mut(),
        lpRemoteName: remote.as_mut_ptr(),
        lpComment: std::ptr::null_mut(),
        lpProvider: std::ptr::null_mut(),
    };
    // SAFETY: the resource's and the arguments' strings are NUL-ended and
    // live until the call returns; nothing is written back.
    let status = unsafe {
        WNetAddConnection2W(
            &raw const resource,
            password.as_ref().map_or(std::ptr::null(), Vec::as_ptr),
            user.as_ref().map_or(std::ptr::null(), Vec::as_ptr),
            CONNECT_TEMPORARY,
        )
    };
    match status {
        NO_ERROR => Ok(()),
        ERROR_ACCESS_DENIED | ERROR_LOGON_FAILURE | ERROR_INVALID_PASSWORD => Err(Refused::Login),
        other => Err(Refused::Other(
            std::io::Error::from_raw_os_error(other.cast_signed()).into(),
        )),
    }
}

/// Let go of the connection to the share at `unc`, unless a program has a
/// file open there.
///
/// # Errors
/// What the system said.
#[cfg(windows)]
#[allow(unsafe_code)] // A Win32 call on a string that outlives it.
pub fn disconnect_unc(unc: &Path) -> anyhow::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::NO_ERROR;
    use windows_sys::Win32::NetworkManagement::WNet::WNetCancelConnection2W;
    let name: Vec<u16> = unc
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    // SAFETY: a NUL-ended string living until the call returns.
    let status = unsafe { WNetCancelConnection2W(name.as_ptr(), 0, 0) };
    if status == NO_ERROR {
        Ok(())
    } else {
        Err(std::io::Error::from_raw_os_error(status.cast_signed()).into())
    }
}

/// Mount the share at `url` (`smb://server/share`, `afp://…`,
/// `nfs://server/export`, `https://server/dav`) through `NetFS`, as `user`
/// with `password` where given (else as the Keychain or a guest may):
/// where it is mounted (under /Volumes).
///
/// # Errors
/// [`Refused::Login`] when the server refused the login; why otherwise.
#[cfg(target_os = "macos")]
#[allow(unsafe_code)] // NetFS's C call on CoreFoundation objects it borrows.
pub fn mount_url(
    url: &str,
    user: Option<&str>,
    password: Option<&str>,
) -> Result<PathBuf, Refused> {
    use objc2_core_foundation::{CFArray, CFRetained, CFString, CFURL};
    #[link(name = "NetFS", kind = "framework")]
    unsafe extern "C" {
        fn NetFSMountURLSync(
            url: &CFURL,
            mountpath: *const CFURL,
            user: *const CFString,
            passwd: *const CFString,
            open_options: *const std::ffi::c_void,
            mount_options: *const std::ffi::c_void,
            mountpoints: *mut *const CFArray,
        ) -> i32;
    }
    /// `NetFS`'s refused login (`EAUTH`), and a bad password (`EPERM`-like
    /// answers some servers give).
    const EAUTH: i32 = 80;
    let url = CFURL::from_string(None, &CFString::from_str(url), None)
        .ok_or_else(|| Refused::Other(anyhow::anyhow!("“{url}” is no address the system reads")))?;
    let user = user.map(CFString::from_str);
    let password = password.map(CFString::from_str);
    let mut mounted: *const CFArray = std::ptr::null();
    // SAFETY: every object is alive for the call; `NetFS` hands back an
    // array it created (owned here, released by its CFRetained).
    let status = unsafe {
        NetFSMountURLSync(
            &url,
            std::ptr::null(),
            user.as_deref()
                .map_or(std::ptr::null(), std::ptr::from_ref::<CFString>),
            password
                .as_deref()
                .map_or(std::ptr::null(), std::ptr::from_ref::<CFString>),
            std::ptr::null(),
            std::ptr::null(),
            &raw mut mounted,
        )
    };
    if status == EAUTH {
        return Err(Refused::Login);
    }
    if status != 0 {
        return Err(Refused::Other(
            std::io::Error::from_raw_os_error(status).into(),
        ));
    }
    let mounted = std::ptr::NonNull::new(mounted.cast_mut())
        .ok_or_else(|| Refused::Other(anyhow::anyhow!("NetFS said no mount point")))?;
    // SAFETY: a +1 array `NetFS` created, released once here.
    let mounted: CFRetained<CFArray> = unsafe { CFRetained::from_raw(mounted) };
    if mounted.count() < 1 {
        return Err(Refused::Other(anyhow::anyhow!("NetFS said no mount point")));
    }
    // SAFETY: its values are the CFStrings of the mount points.
    let first = unsafe { &*mounted.value_at_index(0).cast::<CFString>() };
    Ok(PathBuf::from(first.to_string()))
}

/// [`connect_unc`] where Windows' own calls are not.
///
/// # Errors
/// Always: not this system's.
#[cfg(not(windows))]
pub fn connect_unc(
    _unc: &Path,
    _user: Option<&str>,
    _password: Option<&str>,
) -> Result<(), Refused> {
    Err(Refused::Other(
        skwir_core::system::not_yet("A Windows share").into(),
    ))
}

/// [`disconnect_unc`] where Windows' own calls are not.
///
/// # Errors
/// Always: not this system's.
#[cfg(not(windows))]
pub fn disconnect_unc(_unc: &Path) -> anyhow::Result<()> {
    Err(skwir_core::system::not_yet("A Windows share").into())
}

/// [`mount_url`] where `NetFS` is not.
///
/// # Errors
/// Always: not this system's.
#[cfg(not(target_os = "macos"))]
pub fn mount_url(
    _url: &str,
    _user: Option<&str>,
    _password: Option<&str>,
) -> Result<PathBuf, Refused> {
    Err(Refused::Other(
        skwir_core::system::not_yet("Mounting a share through NetFS").into(),
    ))
}
