//! What Windows says of a file through a handle (ADR-035, ADR-036): the
//! queries `fs::Dir` and `fs::stat` make there, in one place with the
//! `unsafe` they need. Written on Linux, compiled for Windows, run under
//! Wine; never yet on Windows itself.

// This module is the system's calls on Windows, which take raw pointers
// and handles: each `unsafe` block says why its call is sound, and
// nothing unsafe leaves the module.
#![allow(unsafe_code)]

use std::fs::{File, OpenOptions};
use std::io;
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::AsRawHandle;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use windows_sys::Win32::Foundation::{
    ERROR_INVALID_PARAMETER, ERROR_NO_MORE_FILES, ERROR_NOT_SUPPORTED,
};
use windows_sys::Win32::Storage::FileSystem as fs;
use windows_sys::Win32::System::SystemServices::{
    IO_REPARSE_TAG_MOUNT_POINT, IO_REPARSE_TAG_SYMLINK,
};

use crate::entry::{EntryKind, FileId};

const SHARE_ALL: u32 = fs::FILE_SHARE_READ | fs::FILE_SHARE_WRITE | fs::FILE_SHARE_DELETE;

/// Open `path` to read what is said of it, and with `list` what a folder
/// holds, shared with everyone: a folder held open can still be renamed
/// or removed, as on Unix. `follow` false opens a link itself.
pub fn open(path: &Path, list: bool, follow: bool) -> io::Result<File> {
    let mut flags = fs::FILE_FLAG_BACKUP_SEMANTICS;
    if !follow {
        flags |= fs::FILE_FLAG_OPEN_REPARSE_POINT;
    }
    let mut access = fs::FILE_READ_ATTRIBUTES | fs::SYNCHRONIZE;
    if list {
        access |= fs::FILE_LIST_DIRECTORY;
    }
    OpenOptions::new()
        .access_mode(access)
        .share_mode(SHARE_ALL)
        .custom_flags(flags)
        .open(path)
}

/// A new handle to the folder `file` holds open, wherever it is now, with
/// a position of its own: a listing read through it starts at its
/// beginning whatever another reading of the folder does. Opened again
/// by its final path (`ReOpenFile` is refused for folders under Wine).
pub fn reopen(file: &File) -> io::Result<File> {
    let path = path_now(file).ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))?;
    open(&path, true, true)
}

/// Fill a `T` with `class`'s record of `file`.
///
/// # Safety
/// `T` must be the struct `GetFileInformationByHandleEx` fills for
/// `class`: every caller below names a pair the documentation pairs.
unsafe fn query<T: Default>(file: &File, class: fs::FILE_INFO_BY_HANDLE_CLASS) -> io::Result<T> {
    let mut info = T::default();
    let size = u32::try_from(size_of::<T>()).expect("a record's size");
    // SAFETY: `info` is a `T`, the record `class` fills (the caller's
    // promise), and its size is the one given.
    let ok = unsafe {
        fs::GetFileInformationByHandleEx(file.as_raw_handle(), class, (&raw mut info).cast(), size)
    };
    if ok == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(info)
    }
}

/// A 128-bit file ID folded into a [`FileId`]'s 64 bits (ADR-036):
/// NTFS's fill the low half and are kept as they are; `ReFS`'s use all 128,
/// whose halves are folded together.
pub fn fold(id: [u8; 16]) -> u64 {
    let low = u64::from_le_bytes(id[..8].try_into().expect("eight bytes"));
    let high = u64::from_le_bytes(id[8..].try_into().expect("eight bytes"));
    low ^ high.rotate_left(32)
}

/// What a stat says of a file there.
pub struct Info {
    pub id: FileId,
    pub kind: EntryKind,
    pub size: u64,
    pub links: u64,
    pub modified: Option<SystemTime>,
    pub accessed: Option<SystemTime>,
    pub created: Option<SystemTime>,
    /// When it last changed, its bytes or what is said of it.
    pub changed: Option<SystemTime>,
    pub hidden: bool,
    /// Removed while held open: it goes when the last handle closes.
    pub deleting: bool,
}

/// The identity of what `file` holds open: the volume's serial and the
/// file's ID, both from the 128-bit record where the filesystem keeps
/// one (NTFS, `ReFS`), else from the older 32- and 64-bit one (FAT).
pub fn identity(file: &File) -> io::Result<FileId> {
    // SAFETY: FileIdInfo fills a FILE_ID_INFO.
    match unsafe { query::<fs::FILE_ID_INFO>(file, fs::FileIdInfo) } {
        Ok(info) => Ok(FileId {
            dev: info.VolumeSerialNumber,
            ino: fold(info.FileId.Identifier),
        }),
        Err(e) if unsupported(&e) => {
            let mut info = fs::BY_HANDLE_FILE_INFORMATION::default();
            // SAFETY: `info` is the record GetFileInformationByHandle fills.
            if unsafe { fs::GetFileInformationByHandle(file.as_raw_handle(), &raw mut info) } == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(FileId {
                dev: u64::from(info.dwVolumeSerialNumber),
                ino: (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
            })
        }
        Err(e) => Err(e),
    }
}

/// Everything a stat says of what `file` holds open.
pub fn info(file: &File) -> io::Result<Info> {
    // SAFETY: each class below fills the record it is paired with.
    let basic: fs::FILE_BASIC_INFO = unsafe { query(file, fs::FileBasicInfo)? };
    let standard: fs::FILE_STANDARD_INFO = unsafe { query(file, fs::FileStandardInfo)? };
    let tag: fs::FILE_ATTRIBUTE_TAG_INFO = unsafe { query(file, fs::FileAttributeTagInfo)? };
    Ok(Info {
        id: identity(file)?,
        kind: kind_of(tag.FileAttributes, tag.ReparseTag),
        size: u64::try_from(standard.EndOfFile).unwrap_or(0),
        links: u64::from(standard.NumberOfLinks),
        modified: time(basic.LastWriteTime),
        accessed: time(basic.LastAccessTime),
        created: time(basic.CreationTime),
        changed: time(basic.ChangeTime),
        hidden: basic.FileAttributes & fs::FILE_ATTRIBUTE_HIDDEN != 0,
        deleting: standard.DeletePending,
    })
}

/// What an entry is, by its attributes and its reparse tag: a symbolic
/// link or a junction is a link; another reparse point (a `OneDrive`
/// placeholder, deduplicated data) is what its attributes say.
pub fn kind_of(attributes: u32, tag: u32) -> EntryKind {
    if attributes & fs::FILE_ATTRIBUTE_REPARSE_POINT != 0
        && matches!(tag, IO_REPARSE_TAG_SYMLINK | IO_REPARSE_TAG_MOUNT_POINT)
    {
        EntryKind::Symlink
    } else if attributes & fs::FILE_ATTRIBUTE_DIRECTORY != 0 {
        EntryKind::Directory
    } else {
        EntryKind::File
    }
}

/// A `FILETIME` (100 ns since 1601) as a time; `None` for 0, "not
/// known", and before 1970.
pub fn time(filetime: i64) -> Option<SystemTime> {
    /// 1601 to 1970, in 100 ns.
    const EPOCH: i64 = 116_444_736_000_000_000;
    let since = u64::try_from(filetime.checked_sub(EPOCH)?).ok()?;
    (filetime != 0).then(|| {
        SystemTime::UNIX_EPOCH
            + Duration::from_secs(since / 10_000_000)
            + Duration::from_nanos((since % 10_000_000) * 100)
    })
}

/// Where what `file` holds open is now, wherever it moved: its final
/// path, a drive's (`C:\…`) where it has one.
pub fn path_now(file: &File) -> Option<PathBuf> {
    let mut buffer = vec![0u16; 512];
    loop {
        let size = u32::try_from(buffer.len()).ok()?;
        // SAFETY: `buffer` holds `size` UTF-16 units.
        let n = unsafe {
            fs::GetFinalPathNameByHandleW(
                file.as_raw_handle(),
                buffer.as_mut_ptr(),
                size,
                fs::FILE_NAME_NORMALIZED | fs::VOLUME_NAME_DOS,
            )
        } as usize;
        if n == 0 {
            return None;
        }
        if n < buffer.len() {
            buffer.truncate(n);
            let path: std::ffi::OsString = std::os::windows::ffi::OsStringExt::from_wide(&buffer);
            return Some(crate::os_path::plain(PathBuf::from(path)));
        }
        buffer.resize(n + 1, 0);
    }
}

/// Whether `error` says the filesystem does not keep what was asked.
fn unsupported(error: &io::Error) -> bool {
    error.raw_os_error().is_some_and(|code| {
        u32::try_from(code)
            .is_ok_and(|code| matches!(code, ERROR_INVALID_PARAMETER | ERROR_NOT_SUPPORTED))
    })
}

/// One entry of a folder's listing.
pub struct Listed {
    pub name: std::ffi::OsString,
    pub ino: u64,
    pub kind: EntryKind,
    pub hidden: bool,
}

/// The entries of the folder `file` holds open, read in batches of what
/// fits in a 64 KiB buffer, `.` and `..` left out: the 128-bit IDs where
/// the filesystem keeps them, else the 64-bit ones.
pub struct Listing {
    file: File,
    buffer: Vec<u64>,
    /// Where the next record starts in `buffer`, when one is left.
    next: Option<usize>,
    /// Which record the filesystem gives: the 128-bit or the 64-bit one.
    extended: bool,
    started: bool,
    done: bool,
}

impl Listing {
    pub fn new(file: File) -> Self {
        Self {
            file,
            buffer: vec![0; 8 << 10],
            next: None,
            extended: true,
            started: false,
            done: false,
        }
    }

    /// Fill the buffer with the next batch: false at the end.
    fn fill(&mut self) -> io::Result<bool> {
        loop {
            let class = match (self.extended, self.started) {
                (true, false) => fs::FileIdExtdDirectoryRestartInfo,
                (true, true) => fs::FileIdExtdDirectoryInfo,
                (false, false) => fs::FileIdBothDirectoryRestartInfo,
                (false, true) => fs::FileIdBothDirectoryInfo,
            };
            let size = u32::try_from(self.buffer.len() * 8).expect("64 KiB");
            // SAFETY: `buffer` holds `size` bytes, aligned for the records.
            let ok = unsafe {
                fs::GetFileInformationByHandleEx(
                    self.file.as_raw_handle(),
                    class,
                    self.buffer.as_mut_ptr().cast(),
                    size,
                )
            };
            if ok != 0 {
                self.started = true;
                self.next = Some(0);
                return Ok(true);
            }
            let error = io::Error::last_os_error();
            if error.raw_os_error().and_then(|c| u32::try_from(c).ok()) == Some(ERROR_NO_MORE_FILES)
            {
                return Ok(false);
            }
            if self.extended && !self.started && unsupported(&error) {
                self.extended = false;
                continue;
            }
            return Err(error);
        }
    }

    /// The record at `at` in the buffer, and where the next one is.
    fn record(&self, at: usize) -> (Listed, Option<usize>) {
        use fs::{FILE_ID_BOTH_DIR_INFO, FILE_ID_EXTD_DIR_INFO};
        use std::mem::offset_of;
        let bytes: &[u8] = {
            let len = self.buffer.len() * 8;
            // SAFETY: the buffer's `u64`s are `len` initialized bytes.
            unsafe { std::slice::from_raw_parts(self.buffer.as_ptr().cast::<u8>(), len) }
        };
        // What a record says, read where it is whatever its alignment.
        let (next, name_at, name_len, ino, attributes, tag) = if self.extended {
            assert!(at + size_of::<FILE_ID_EXTD_DIR_INFO>() <= bytes.len());
            // SAFETY: the system wrote a whole record at `at`, inside the
            // buffer (asserted above).
            let r: FILE_ID_EXTD_DIR_INFO =
                unsafe { std::ptr::read_unaligned(bytes.as_ptr().add(at).cast()) };
            (
                r.NextEntryOffset,
                offset_of!(FILE_ID_EXTD_DIR_INFO, FileName),
                r.FileNameLength,
                fold(r.FileId.Identifier),
                r.FileAttributes,
                r.ReparsePointTag,
            )
        } else {
            assert!(at + size_of::<FILE_ID_BOTH_DIR_INFO>() <= bytes.len());
            // SAFETY: as above.
            let r: FILE_ID_BOTH_DIR_INFO =
                unsafe { std::ptr::read_unaligned(bytes.as_ptr().add(at).cast()) };
            // The older record says a reparse point's tag in its EA size.
            let tag = if r.FileAttributes & fs::FILE_ATTRIBUTE_REPARSE_POINT != 0 {
                r.EaSize
            } else {
                0
            };
            (
                r.NextEntryOffset,
                offset_of!(FILE_ID_BOTH_DIR_INFO, FileName),
                r.FileNameLength,
                r.FileId.cast_unsigned(),
                r.FileAttributes,
                tag,
            )
        };
        let start = at + name_at;
        let name = bytes
            .get(start..start + name_len as usize)
            .unwrap_or_default()
            .as_chunks::<2>()
            .0
            .iter()
            .map(|unit| u16::from_le_bytes(*unit))
            .collect::<Vec<u16>>();
        (
            Listed {
                name: std::os::windows::ffi::OsStringExt::from_wide(&name),
                ino,
                kind: kind_of(attributes, tag),
                hidden: attributes & fs::FILE_ATTRIBUTE_HIDDEN != 0,
            },
            (next != 0).then_some(at + next as usize),
        )
    }
}

impl Iterator for Listing {
    type Item = io::Result<Listed>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if self.done {
                return None;
            }
            let Some(at) = self.next else {
                match self.fill() {
                    Ok(true) => continue,
                    Ok(false) => {
                        self.done = true;
                        return None;
                    }
                    Err(e) => {
                        self.done = true;
                        return Some(Err(e));
                    }
                }
            };
            let (listed, next) = self.record(at);
            self.next = next;
            if listed.name == "." || listed.name == ".." {
                continue;
            }
            return Some(Ok(listed));
        }
    }
}

/// The bytes free to the caller on the volume `path` is on.
pub(crate) fn free_space(path: &Path) -> Option<u64> {
    let mut free = 0u64;
    // SAFETY: a NUL-ended path and a place for the one number asked; the
    // others are not asked (null).
    let ok = unsafe {
        fs::GetDiskFreeSpaceExW(
            wide(path).as_ptr(),
            &raw mut free,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    (ok != 0).then_some(free)
}

/// `path` as Windows takes it: UTF-16, ending in a NUL.
fn wide(path: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    path.as_os_str().encode_wide().chain([0]).collect()
}

/// Rename `from` to `to`, refusing to replace what is there
/// (`AlreadyExists`) and to move to another volume (`CrossesDevices`):
/// `MoveFileExW` without its replace and copy flags.
pub fn rename_noreplace(from: &Path, to: &Path) -> io::Result<()> {
    let (from, to) = (wide(from), wide(to));
    // SAFETY: both are NUL-terminated UTF-16 strings that outlive the call.
    if unsafe { fs::MoveFileExW(from.as_ptr(), to.as_ptr(), 0) } == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Copy the file at `from` to `to`, which must not exist yet, as Windows
/// copies a file (`CopyFileExW`): its bytes with its alternate streams
/// (the mark a downloaded file carries), its attributes and its times.
/// `progress` is told how many bytes are written so far; when it says
/// no, the copy stops, Windows removes what it wrote, and it fails with
/// `Interrupted`.
pub fn copy_file(from: &Path, to: &Path, progress: &mut dyn FnMut(u64) -> bool) -> io::Result<()> {
    /// What the routine is handed, through the copy's `lpData`.
    struct Data<'a> {
        progress: &'a mut dyn FnMut(u64) -> bool,
    }

    unsafe extern "system" fn routine(
        _total: i64,
        transferred: i64,
        _stream_total: i64,
        _stream_transferred: i64,
        _stream: u32,
        _reason: u32,
        _from: windows_sys::Win32::Foundation::HANDLE,
        _to: windows_sys::Win32::Foundation::HANDLE,
        data: *const core::ffi::c_void,
    ) -> u32 {
        // SAFETY: `data` is the `Data` `copy_file` passed, alive and not
        // otherwise used while the copy runs on this thread.
        let data = unsafe { &mut *data.cast::<Data<'_>>().cast_mut() };
        if (data.progress)(u64::try_from(transferred).unwrap_or(0)) {
            fs::PROGRESS_CONTINUE
        } else {
            fs::PROGRESS_CANCEL
        }
    }

    let mut data = Data { progress };
    let (from, to) = (wide(from), wide(to));
    // SAFETY: both paths are NUL-terminated UTF-16 that outlive the call,
    // and `data` is what `routine` reads, alive until the copy returns.
    let ok = unsafe {
        fs::CopyFileExW(
            from.as_ptr(),
            to.as_ptr(),
            Some(routine),
            (&raw mut data).cast(),
            std::ptr::null_mut(),
            fs::COPY_FILE_FAIL_IF_EXISTS,
        )
    };
    if ok != 0 {
        return Ok(());
    }
    let error = io::Error::last_os_error();
    if error.raw_os_error().and_then(|c| u32::try_from(c).ok())
        == Some(windows_sys::Win32::Foundation::ERROR_REQUEST_ABORTED)
    {
        Err(io::Error::from(io::ErrorKind::Interrupted))
    } else {
        Err(error)
    }
}
