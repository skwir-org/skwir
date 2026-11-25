//! The named pipe an engine is reached by on Windows (`docs/DESIGN.md`,
//! Protocol; `docs/PLAN.md`, phase 4, step 4).
//!
//! - **Its name is its user's:** `\\.\pipe\skwird-<SID>`, or the one
//!   `SKWIRD_PIPE` names (a test's own, as `XDG_RUNTIME_DIR` is on Unix).
//! - **A connection can be given up on:** every read and write is
//!   overlapped, waited for at most the silence given, and cancelled from
//!   another thread by [`Stream::shutdown`].
//! - **The server is checked before anything is said:** the pipe's owner
//!   is `LocalSystem` or the administrators (the system service's,
//!   `\\.\pipe\skwird`), or its process runs as this user; otherwise the
//!   connection is refused. Any user may make a pipe of any name first; a
//!   client that spoke to one another user made would tell it what it
//!   searches for. The client is identified to the server, never
//!   impersonated (`SECURITY_IDENTIFICATION`).
//! - **The engine's side** ([`Server`]): every instance its user's alone
//!   (a security descriptor granting that user's SID alone), the first
//!   made only if no other process holds the name
//!   (`FILE_FLAG_FIRST_PIPE_INSTANCE`), remote clients refused. The system
//!   service's ([`Server::bind_system`]): every user who logs on may
//!   connect, none may make an instance of it, and its owner is the
//!   service's account.

#![allow(unsafe_code)] // Windows' pipes have no safe binding; each block says why it is sound.

use std::ffi::{OsStr, c_void};
use std::io::{Read, Write};
use std::os::windows::ffi::OsStrExt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_BROKEN_PIPE, ERROR_IO_PENDING, ERROR_OPERATION_ABORTED, ERROR_PIPE_BUSY,
    ERROR_PIPE_CONNECTED, ERROR_PIPE_NOT_CONNECTED, GENERIC_READ, GetLastError, HANDLE,
    INVALID_HANDLE_VALUE, LocalFree, WAIT_TIMEOUT,
};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, GetSecurityInfo,
    SE_KERNEL_OBJECT,
};
use windows_sys::Win32::Security::{
    GetTokenInformation, OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID, RevertToSelf,
    SECURITY_ATTRIBUTES, TOKEN_DUPLICATE, TOKEN_QUERY, TOKEN_USER, TokenUser,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAG_OVERLAPPED, FILE_WRITE_DATA,
    OPEN_EXISTING, PIPE_ACCESS_DUPLEX, ReadFile, SECURITY_IDENTIFICATION, SECURITY_SQOS_PRESENT,
    WriteFile,
};
use windows_sys::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, GetNamedPipeServerProcessId, ImpersonateNamedPipeClient,
    PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES,
    PIPE_WAIT, WaitNamedPipeW,
};
use windows_sys::Win32::System::Threading::{
    CreateEventW, GetCurrentProcess, GetCurrentThread, INFINITE, OpenProcess, OpenProcessToken,
    OpenThreadToken, PROCESS_QUERY_LIMITED_INFORMATION, SetEvent, WaitForSingleObject,
};

/// The size of each side's buffer, in bytes.
const BUFFER: u32 = 64 * 1024;

/// `LocalSystem`, which the system service runs as; the administrators,
/// one of whom may run it by hand.
const SYSTEM: &str = "S-1-5-18";
const ADMINISTRATORS: &str = "S-1-5-32-544";

/// A handle, closed once dropped.
struct Handle(HANDLE);

// SAFETY: a handle names a kernel object, usable from any thread; it is
// closed once, when dropped, and never used after.
unsafe impl Send for Handle {}
// SAFETY: as above; overlapped reads and writes from several threads each
// have their own structure and event.
unsafe impl Sync for Handle {}

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: the handle was opened by this process, and is closed here
        // once.
        unsafe { CloseHandle(self.0) };
    }
}

/// An event, made unsignaled and reset by hand, closed once dropped.
fn event() -> std::io::Result<Handle> {
    // SAFETY: no attributes, no name: a new event of this process's own.
    let event = unsafe { CreateEventW(std::ptr::null(), 1, 0, std::ptr::null()) };
    if event.is_null() {
        return Err(std::io::Error::last_os_error());
    }
    Ok(Handle(event))
}

fn wide(name: &OsStr) -> Vec<u16> {
    name.encode_wide().chain([0]).collect()
}

fn millis(wait: Option<Duration>) -> u32 {
    wait.map_or(INFINITE, |w| {
        u32::try_from(w.as_millis()).map_or(INFINITE - 1, |ms| ms.clamp(1, INFINITE - 1))
    })
}

/// The pipe's end: its handle, and whether it was shut down.
struct End {
    handle: Handle,
    shut: AtomicBool,
}

/// A connection over a named pipe, either side's. Clones share it:
/// [`Stream::try_clone`] gives the reader and the writer their own.
#[derive(Clone)]
pub struct Stream {
    end: Arc<End>,
    /// The longest a read or a write is waited for: none, as long as it
    /// takes.
    timeout: Option<Duration>,
}

impl Stream {
    fn of(handle: Handle) -> Stream {
        Stream {
            end: Arc::new(End {
                handle,
                shut: AtomicBool::new(false),
            }),
            timeout: None,
        }
    }

    /// The token of the client at the other end of this, a server's end:
    /// taken by impersonating it for the one call that opens it, then the
    /// server's own again. At the level its client allows, identification
    /// (`SECURITY_IDENTIFICATION`, as [`Stream::connect`] asks): enough to
    /// judge what it may do, never to act as it.
    pub fn client_token(&self) -> std::io::Result<std::os::windows::io::OwnedHandle> {
        use std::os::windows::io::FromRawHandle;
        // SAFETY: this end's pipe, connected.
        if unsafe { ImpersonateNamedPipeClient(self.end.handle.0) } == 0 {
            return Err(std::io::Error::last_os_error());
        }
        let mut token: HANDLE = std::ptr::null_mut();
        // SAFETY: this thread's pseudo-handle; the token given is ours to
        // close. Opened as the server, not as the client it impersonates.
        let opened = unsafe {
            OpenThreadToken(
                GetCurrentThread(),
                TOKEN_QUERY | TOKEN_DUPLICATE,
                1,
                &raw mut token,
            )
        };
        let error = std::io::Error::last_os_error();
        // SAFETY: no argument; whatever came of the open.
        if unsafe { RevertToSelf() } == 0 {
            // The thread would go on as the client: it never may.
            std::process::abort();
        }
        if opened == 0 {
            return Err(error);
        }
        // SAFETY: a token the call opened, ours alone.
        Ok(unsafe { std::os::windows::io::OwnedHandle::from_raw_handle(token) })
    }

    /// The engine at the pipe named `name`, once one of its instances is
    /// free, waited for at most `wait`: refused unless its owner is
    /// `LocalSystem` or the administrators (the system service's, which no
    /// other user can make so), or its process runs as this user.
    pub fn connect(name: &OsStr, wait: Option<Duration>) -> std::io::Result<Stream> {
        let name = wide(name);
        loop {
            // SAFETY: a NUL-terminated name alive for the call, no security
            // attributes, no template; the handle given is ours to close.
            // Asked to read and write the connection and read its owner,
            // no more: what the system's pipe grants every user.
            let handle = unsafe {
                CreateFileW(
                    name.as_ptr(),
                    GENERIC_READ | FILE_WRITE_DATA,
                    0,
                    std::ptr::null(),
                    OPEN_EXISTING,
                    FILE_FLAG_OVERLAPPED | SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
                    std::ptr::null_mut(),
                )
            };
            if handle != INVALID_HANDLE_VALUE {
                let stream = Stream::of(Handle(handle));
                if stream.owned_by_the_system()? || stream.server_runs_as(&user_sid()?)? {
                    return Ok(stream);
                }
                return Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "the pipe is another user's: refused",
                ));
            }
            // SAFETY: no argument.
            let error = unsafe { GetLastError() };
            // Every instance busy: one is waited for, then tried again.
            // SAFETY: a NUL-terminated name alive for the call.
            if error != ERROR_PIPE_BUSY
                || unsafe { WaitNamedPipeW(name.as_ptr(), millis(wait)) } == 0
            {
                return Err(std::io::Error::from_raw_os_error(
                    i32::try_from(error).unwrap_or(i32::MAX),
                ));
            }
        }
    }

    /// Whether the pipe's owner is `LocalSystem` or the administrators: a
    /// system service's, or an administrator's who runs it by hand. A
    /// pipe's maker may name as its owner only itself, or a group it holds
    /// as an owner may be (an administrator's, elevated); `LocalSystem` and
    /// the administrators can name any.
    fn owned_by_the_system(&self) -> std::io::Result<bool> {
        let mut owner: PSID = std::ptr::null_mut();
        let mut held: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
        // SAFETY: this end's handle, opened to read its security; the
        // owner points into the descriptor given, which is ours to free.
        let failed = unsafe {
            GetSecurityInfo(
                self.end.handle.0,
                SE_KERNEL_OBJECT,
                OWNER_SECURITY_INFORMATION,
                &raw mut owner,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &raw mut held,
            )
        };
        if failed != 0 {
            return Err(std::io::Error::from_raw_os_error(
                i32::try_from(failed).unwrap_or(i32::MAX),
            ));
        }
        let held = Descriptor(held);
        let owner = sid_text(owner);
        drop(held);
        Ok(owner.is_ok_and(|o| o == SYSTEM || o == ADMINISTRATORS))
    }

    /// Wait at most `timeout` for each read and write from now on; past it,
    /// the call fails (`TimedOut`). None waits as long as it takes.
    pub fn set_timeout(&mut self, timeout: Option<Duration>) {
        self.timeout = timeout;
    }

    /// Another handle on this connection, for a reader or a writer of its
    /// own.
    pub fn try_clone(&self) -> std::io::Result<Stream> {
        Ok(self.clone())
    }

    /// Stop every read and write on this connection, now and after, from
    /// any thread: what waits returns at once, failing.
    pub fn shutdown(&self) {
        // Before the cancel, in one order with a call's check after it is
        // made ([`Stream::overlapped`]): either the cancel finds the call,
        // or the call sees the stop.
        self.end.shut.store(true, Ordering::SeqCst);
        // SAFETY: every overlapped call on the handle, whichever thread made
        // it, is cancelled; each waits for its cancel before it returns.
        unsafe { CancelIoEx(self.end.handle.0, std::ptr::null()) };
    }

    /// Whether the process at the other end runs as the user `sid` names.
    fn server_runs_as(&self, sid: &str) -> std::io::Result<bool> {
        let mut pid = 0u32;
        // SAFETY: the handle is a pipe's client end; the id is ours.
        if unsafe { GetNamedPipeServerProcessId(self.end.handle.0, &raw mut pid) } == 0 {
            return Err(std::io::Error::last_os_error());
        }
        // SAFETY: the least access that asks of a process's token.
        let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if process.is_null() {
            return Err(std::io::Error::last_os_error());
        }
        let process = Handle(process);
        Ok(token_user(process.0)? == sid)
    }

    /// One overlapped call, waited for at most the timeout, cancelled past
    /// it: the bytes it moved.
    fn overlapped(
        &self,
        call: impl FnOnce(HANDLE, *mut OVERLAPPED) -> i32,
    ) -> std::io::Result<u32> {
        if self.end.shut.load(Ordering::Acquire) {
            return Err(std::io::ErrorKind::ConnectionAborted.into());
        }
        let handle = self.end.handle.0;
        let event = event()?;
        // SAFETY: an all-zero structure is a valid one, before its event.
        let mut ov: OVERLAPPED = unsafe { std::mem::zeroed() };
        ov.hEvent = event.0;
        if call(handle, &raw mut ov) == 0 {
            // SAFETY: no argument.
            let error = unsafe { GetLastError() };
            if error != ERROR_IO_PENDING {
                return Err(failed(error));
            }
            // A stop between the check above and the call cancelled nothing
            // of it, as it was not made yet: seen now, it cancels the call
            // here. One made after this check finds the call.
            if self.end.shut.load(Ordering::SeqCst) {
                // SAFETY: the call's own structure, alive until its result
                // is waited for, below.
                unsafe { CancelIoEx(handle, &raw const ov) };
            }
            // SAFETY: the event is ours, alive for the wait.
            if unsafe { WaitForSingleObject(event.0, millis(self.timeout)) } == WAIT_TIMEOUT {
                let mut moved = 0;
                // SAFETY: the call's own structure, alive until its cancel has
                // landed, which the wait for its result waits for.
                let done = unsafe {
                    CancelIoEx(handle, &raw const ov);
                    GetOverlappedResult(handle, &raw const ov, &raw mut moved, 1)
                };
                // Done before its cancel landed: what it moved is not lost.
                if done != 0 && moved > 0 {
                    return Ok(moved);
                }
                return Err(std::io::ErrorKind::TimedOut.into());
            }
        }
        let mut moved = 0;
        // SAFETY: the call's own structure; its result is waited for.
        if unsafe { GetOverlappedResult(handle, &raw const ov, &raw mut moved, 1) } == 0 {
            // SAFETY: no argument.
            return Err(failed(unsafe { GetLastError() }));
        }
        Ok(moved)
    }
}

/// A failed call's error: a stop is an abort, the other end gone the end.
fn failed(error: u32) -> std::io::Error {
    match error {
        ERROR_OPERATION_ABORTED => std::io::ErrorKind::ConnectionAborted.into(),
        ERROR_BROKEN_PIPE | ERROR_PIPE_NOT_CONNECTED => std::io::ErrorKind::BrokenPipe.into(),
        e => std::io::Error::from_raw_os_error(i32::try_from(e).unwrap_or(i32::MAX)),
    }
}

impl Read for Stream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let len = u32::try_from(buf.len()).unwrap_or(u32::MAX);
        let ptr = buf.as_mut_ptr();
        // SAFETY: the buffer and its length, alive and ours until the call
        // has completed or its cancel has landed.
        let read =
            self.overlapped(|h, ov| unsafe { ReadFile(h, ptr, len, std::ptr::null_mut(), ov) });
        match read {
            Ok(n) => Ok(n as usize),
            // The other end gone: the end of what it says.
            Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => Ok(0),
            Err(e) => Err(e),
        }
    }
}

impl Write for Stream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let len = u32::try_from(buf.len()).unwrap_or(u32::MAX);
        let ptr = buf.as_ptr();
        // SAFETY: the bytes and their length, alive until the call has
        // completed or its cancel has landed.
        self.overlapped(|h, ov| unsafe { WriteFile(h, ptr, len, std::ptr::null_mut(), ov) })
            .map(|n| n as usize)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// The SID of the user `process` runs as, as a string (`S-1-5-21-...`).
fn token_user(process: HANDLE) -> std::io::Result<String> {
    let mut token: HANDLE = std::ptr::null_mut();
    // SAFETY: a process handle of ours; the token given is ours to close.
    if unsafe { OpenProcessToken(process, TOKEN_QUERY, &raw mut token) } == 0 {
        return Err(std::io::Error::last_os_error());
    }
    let token = Handle(token);
    sid_of(token.0)
}

/// The SID of the user of `token`, as text: a client's, as its server's
/// end took it ([`Stream::client_token`]).
pub fn token_sid(token: &std::os::windows::io::OwnedHandle) -> std::io::Result<String> {
    use std::os::windows::io::AsRawHandle;
    sid_of(token.as_raw_handle())
}

/// The SID of the user of the token `token`, as text.
fn sid_of(token: HANDLE) -> std::io::Result<String> {
    let mut buffer = vec![0u64; 64];
    let mut needed = 0u32;
    loop {
        let size = u32::try_from(buffer.len() * 8).unwrap_or(u32::MAX);
        // SAFETY: the buffer, aligned for the structure, and its size.
        let ok = unsafe {
            GetTokenInformation(
                token,
                TokenUser,
                buffer.as_mut_ptr().cast::<c_void>(),
                size,
                &raw mut needed,
            )
        };
        if ok != 0 {
            break;
        }
        if needed as usize <= buffer.len() * 8 {
            return Err(std::io::Error::last_os_error());
        }
        buffer.resize((needed as usize).div_ceil(8), 0);
    }
    // SAFETY: the call wrote a TOKEN_USER at the buffer's start, aligned.
    let user = unsafe { &*buffer.as_ptr().cast::<TOKEN_USER>() };
    // The SID the structure points to, inside the buffer.
    sid_text(user.User.Sid)
}

/// The text of the SID `sid` points to (`S-1-5-...`), valid for the call.
fn sid_text(sid: PSID) -> std::io::Result<String> {
    let mut text: *mut u16 = std::ptr::null_mut();
    // SAFETY: a SID, valid for the call; the string given is ours to free.
    if unsafe { ConvertSidToStringSidW(sid, &raw mut text) } == 0 {
        return Err(std::io::Error::last_os_error());
    }
    // A SID's text is under 200 units.
    // SAFETY: a NUL-terminated string the call made, read to its NUL.
    let len = (0..512)
        .take_while(|&i| unsafe { *text.add(i) } != 0)
        .count();
    // SAFETY: `len` units, read before the string is freed.
    let sid = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(text, len) });
    // SAFETY: the string the call made, freed once.
    unsafe { LocalFree(text.cast()) };
    Ok(sid)
}

/// The SID of the user this process runs as.
pub fn user_sid() -> std::io::Result<String> {
    // SAFETY: a pseudo-handle on this process, never closed.
    token_user(unsafe { GetCurrentProcess() })
}

/// The pipe of this user's engine: the one `SKWIRD_PIPE` names, or
/// `\\.\pipe\skwird-<SID>`.
pub fn user_pipe() -> Option<std::path::PathBuf> {
    if let Some(name) = std::env::var_os("SKWIRD_PIPE").filter(|v| !v.is_empty()) {
        return Some(name.into());
    }
    user_sid()
        .ok()
        .map(|sid| format!(r"\\.\pipe\skwird-{sid}").into())
}

/// A security descriptor, freed once dropped.
struct Descriptor(PSECURITY_DESCRIPTOR);

// SAFETY: a descriptor in memory of this process's, read alone once made.
unsafe impl Send for Descriptor {}
// SAFETY: as above.
unsafe impl Sync for Descriptor {}

impl Drop for Descriptor {
    fn drop(&mut self) {
        // SAFETY: the descriptor the conversion made, freed once.
        unsafe { LocalFree(self.0.cast()) };
    }
}

/// A connection being waited for: its overlapped call, and its instance.
/// The call is let go of first (the order of the fields), while its
/// instance is open.
struct Pending {
    call: Listening,
    pipe: Handle,
    /// Whether a client was there before the call was made: it failed at
    /// once, saying so, and its structure says nothing.
    connected: bool,
}

// SAFETY: the structure is the pending call's alone, used by one thread at
// a time under the server's lock; the handles are usable from any thread.
unsafe impl Send for Pending {}

/// An instance's call waiting for a client: its event, and its structure
/// at a place of its own while it is pending. Let go of, the call is
/// cancelled and waited for until its cancel lands: the system writes to
/// the structure as the call ends, and must never write to it freed.
struct Listening {
    /// The instance's handle, which [`Pending`] holds open.
    pipe: HANDLE,
    event: Handle,
    ov: Box<OVERLAPPED>,
}

impl Drop for Listening {
    fn drop(&mut self) {
        let mut moved = 0;
        // SAFETY: the instance's handle, open until this is let go of; the
        // call's own structure, alive until the wait for its result
        // returns, which waits for its cancel to land. A call done (a
        // client connected, its event set, by the system or by hand)
        // cancels nothing, and the wait ends at once.
        unsafe {
            CancelIoEx(self.pipe, &raw const *self.ov);
            GetOverlappedResult(self.pipe, &raw const *self.ov, &raw mut moved, 1);
        }
    }
}

/// The engine's side of a pipe: its instances, each its user's alone, or
/// the system's ([`Server::bind_system`]).
pub struct Server {
    name: Vec<u16>,
    security: Descriptor,
    pending: Mutex<Option<Pending>>,
}

/// What every user who logs on is granted on the system's pipe: to read
/// and write a connection, and to read its security, its owner among it.
/// Never to make an instance of it (`FILE_CREATE_PIPE_INSTANCE`, the bit
/// a file's appending is), which `GENERIC_WRITE` would grant: any user
/// could then answer the service's clients.
const CONNECT: u32 = 0x0012_008B;

impl Server {
    /// The pipe named `name`, its first instance made, every instance its
    /// user's alone: refused if another process holds the name.
    pub fn bind(name: &OsStr) -> std::io::Result<Server> {
        // Full access to this user's SID alone, nothing inherited.
        Server::bind_as(name, &format!("D:P(A;;GA;;;{})", user_sid()?))
    }

    /// The system service's pipe named `name`, its first instance made:
    /// refused if another process holds the name. Every user who logs on
    /// may connect to it ([`CONNECT`]); `LocalSystem` and the
    /// administrators may make its instances. Its owner is `LocalSystem`
    /// where the service runs as it, the administrators where one runs it
    /// by hand: no other user can make either a pipe's owner, which a
    /// client checks ([`Stream::connect`]).
    pub fn bind_system(name: &OsStr) -> std::io::Result<Server> {
        let owner = if user_sid()? == SYSTEM { "SY" } else { "BA" };
        Server::bind_as(
            name,
            &format!("O:{owner}D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;{CONNECT:#x};;;AU)"),
        )
    }

    /// The pipe named `name`, each instance with the security descriptor
    /// `sddl` says.
    fn bind_as(name: &OsStr, sddl: &str) -> std::io::Result<Server> {
        let sddl = wide(OsStr::new(sddl));
        let mut security: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
        // SAFETY: a NUL-terminated string; the descriptor given is ours to
        // free.
        let ok = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                1,
                &raw mut security,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Err(std::io::Error::last_os_error());
        }
        let server = Server {
            name: wide(name),
            security: Descriptor(security),
            pending: Mutex::new(None),
        };
        let first = server.instance(true)?;
        *server
            .pending
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(first);
        Ok(server)
    }

    /// A new instance, waiting for a client.
    fn instance(&self, first: bool) -> std::io::Result<Pending> {
        let attributes = SECURITY_ATTRIBUTES {
            nLength: u32::try_from(size_of::<SECURITY_ATTRIBUTES>()).unwrap_or(u32::MAX),
            lpSecurityDescriptor: self.security.0,
            bInheritHandle: 0,
        };
        let mut open = PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED;
        if first {
            open |= FILE_FLAG_FIRST_PIPE_INSTANCE;
        }
        // SAFETY: a NUL-terminated name and the attributes, alive for the
        // call; the handle given is ours to close.
        let pipe = unsafe {
            CreateNamedPipeW(
                self.name.as_ptr(),
                open,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                PIPE_UNLIMITED_INSTANCES,
                BUFFER,
                BUFFER,
                0,
                &raw const attributes,
            )
        };
        if pipe == INVALID_HANDLE_VALUE {
            return Err(std::io::Error::last_os_error());
        }
        let pipe = Handle(pipe);
        let event = event()?;
        // SAFETY: an all-zero structure is a valid one, before its event.
        let mut ov: Box<OVERLAPPED> = Box::new(unsafe { std::mem::zeroed() });
        ov.hEvent = event.0;
        // SAFETY: the structure stays at its place until the call is done:
        // it is boxed, and kept with the instance.
        let mut connected = false;
        if unsafe { ConnectNamedPipe(pipe.0, &raw mut *ov) } == 0 {
            // SAFETY: no argument.
            match unsafe { GetLastError() } {
                ERROR_IO_PENDING => {}
                // A client came between the instance and the call: no event
                // will say so, so it is said here; nor will the structure,
                // its call failed at once.
                ERROR_PIPE_CONNECTED => {
                    connected = true;
                    // SAFETY: the event is ours.
                    unsafe { SetEvent(event.0) };
                }
                // Failed at once: no call pends, nothing to cancel.
                e => return Err(failed(e)),
            }
        }
        Ok(Pending {
            call: Listening {
                pipe: pipe.0,
                event,
                ov,
            },
            pipe,
            connected,
        })
    }

    /// The next client, waited for at most `wait`: none if it passed.
    pub fn accept(&self, wait: Option<Duration>) -> std::io::Result<Option<Stream>> {
        let mut pending = self.pending.lock().unwrap_or_else(PoisonError::into_inner);
        if pending.is_none() {
            *pending = Some(self.instance(false)?);
        }
        let p = pending.as_ref().expect("an instance waits");
        // SAFETY: the instance's event, alive while it is pending.
        if unsafe { WaitForSingleObject(p.call.event.0, millis(wait)) } == WAIT_TIMEOUT {
            return Ok(None);
        }
        // The next instance is made before this one is handed on or let go
        // of: the name always has one waiting. A client never finds it gone,
        // to start another engine in place of this one, and no other
        // process can make the name its own meanwhile, which only the first
        // instance's flag refuses (`FILE_FLAG_FIRST_PIPE_INSTANCE`). Should
        // it fail, the next accept makes it.
        let next = self.instance(false).ok();
        let Pending {
            call,
            pipe,
            connected,
        } = std::mem::replace(&mut *pending, next).expect("an instance waits");
        drop(pending);
        let connected = connected || {
            let mut moved = 0;
            // SAFETY: the instance's own call and structure, done.
            let ok = unsafe { GetOverlappedResult(pipe.0, &raw const *call.ov, &raw mut moved, 0) };
            // SAFETY: no argument.
            ok != 0 || unsafe { GetLastError() } == ERROR_PIPE_CONNECTED
        };
        // Done: its drop cancels nothing, waits for nothing.
        drop(call);
        Ok(connected.then(|| Stream::of(pipe)))
    }
}

/// Start `program` with `args`, from `folder`, its environment this
/// process's with `set` over it: with no console window, in a process
/// group of its own, and none of this process's handles. Windows gives a
/// child every inheritable handle its parent holds where asked to inherit
/// any, as std's `Command` asks: the engine, which outlives its client,
/// would keep open what the client was given (the pipe a launcher reads
/// the client's output from, to its end). So nothing is inherited, and
/// the engine opens its log itself (`SKWIRD_LOG`). `program` is the file
/// started, named whole: Windows, given a command line alone, looks for it
/// in the current folder first.
pub(crate) fn start_detached(
    program: &std::path::Path,
    args: &[&str],
    folder: &std::path::Path,
    set: &[(&str, &OsStr)],
) -> std::io::Result<()> {
    use windows_sys::Win32::System::Threading::{
        CREATE_NEW_PROCESS_GROUP, CREATE_NO_WINDOW, CREATE_UNICODE_ENVIRONMENT, CreateProcessW,
        PROCESS_INFORMATION, STARTUPINFOW,
    };
    // Each part quoted: a program's path may hold spaces. Neither it nor
    // an argument here holds a quote, nor ends in a backslash.
    let quoted = |s: &OsStr| -> Vec<u16> {
        std::iter::once(u16::from(b'"'))
            .chain(s.encode_wide())
            .chain([u16::from(b'"')])
            .collect()
    };
    let mut line = quoted(program.as_os_str());
    for arg in args {
        line.push(u16::from(b' '));
        line.extend(quoted(OsStr::new(arg)));
    }
    line.push(0);
    // This process's environment, `set` over it, sorted by name as
    // Windows keeps a block.
    let mut vars: Vec<(std::ffi::OsString, std::ffi::OsString)> = std::env::vars_os()
        .filter(|(k, _)| !set.iter().any(|(s, _)| k.eq_ignore_ascii_case(s)))
        .chain(set.iter().map(|(k, v)| ((*k).into(), (*v).to_owned())))
        .collect();
    vars.sort_by_key(|(k, _)| k.to_string_lossy().to_uppercase());
    let mut block: Vec<u16> = Vec::new();
    for (k, v) in &vars {
        block.extend(k.encode_wide());
        block.push(u16::from(b'='));
        block.extend(v.encode_wide());
        block.push(0);
    }
    block.push(0);
    let folder = wide(folder.as_os_str());
    let application = wide(program.as_os_str());
    // SAFETY: all-zero structures are valid ones, the first sized.
    let mut startup: STARTUPINFOW = unsafe { std::mem::zeroed() };
    startup.cb = u32::try_from(size_of::<STARTUPINFOW>()).unwrap_or(u32::MAX);
    // SAFETY: as above.
    let mut started: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };
    // SAFETY: a NUL-terminated program, a NUL-terminated command line the
    // call may write in, an environment block ended by two NULs, a
    // NUL-terminated folder, the structures alive for the call; nothing
    // inherited. The handles given are ours to close.
    let ok = unsafe {
        CreateProcessW(
            application.as_ptr(),
            line.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            0,
            CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP | CREATE_UNICODE_ENVIRONMENT,
            block.as_ptr().cast::<c_void>(),
            folder.as_ptr(),
            &raw const startup,
            &raw mut started,
        )
    };
    if ok == 0 {
        return Err(std::io::Error::last_os_error());
    }
    drop(Handle(started.hProcess));
    drop(Handle(started.hThread));
    Ok(())
}
