//! What a preview read of its file (ADR-101): read once, in the file's
//! lane, and shared by every provider and step that shows it, so a file on
//! a mount that keeps nothing it reads (rclone's `minimal`) is fetched
//! once, not once per step. Its first bytes always; all of it up to a
//! bound; past the bound, its last bytes and, on a remote, a few blocks
//! between. Never a memory map (a file shortened while mapped is SIGBUS; a
//! page of a server that stops answering blocks its thread past
//! cancellation), never a descriptor kept between reads (Eject fails
//! "busy"; Windows refuses another program's save by replacement).

use std::borrow::Cow;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::SystemTime;

use parking_lot::Mutex;

use crate::CancelToken;

/// The first bytes held of every file: what decides its type, whether it
/// is text, and what a provider's `accepts` reads.
pub const HEAD: usize = 64 * 1024;
/// The last bytes held of a file not held whole: a zip's directory, a
/// PDF's trailer, a film's index.
pub const TAIL: usize = 64 * 1024;
/// A file a quick preview holds whole: the most it reads whole
/// (`limits::BINARY_INPUT`).
pub const WHOLE: u64 = 16 * 1024 * 1024;
/// A file Full preview holds whole (`limits::WHOLE_BINARY_INPUT`, ADR-068).
pub const WHOLE_FULL: u64 = 128 * 1024 * 1024;
/// What is read at once past the ends of a remote's file not held whole.
const BLOCK: u64 = 1024 * 1024;
/// Blocks kept of it: the most recently read.
const BLOCKS: usize = 8;
/// What a whole file is read by, cancellation asked between.
const CHUNK: usize = 1024 * 1024;

/// What the file was when it was first opened. A later open that finds
/// another length or time modified is another version of it, and refused:
/// what is held belongs to one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Stamp {
    len: u64,
    modified: Option<SystemTime>,
}

impl Stamp {
    fn of(file: &File) -> io::Result<Stamp> {
        let meta = file.metadata()?;
        Ok(Stamp {
            len: meta.len(),
            modified: meta.modified().ok(),
        })
    }
}

/// What a preview holds of one version of a file.
pub struct Held {
    path: PathBuf,
    stamp: Stamp,
    /// Whether the file is on a remote, where every read is a request.
    remote: bool,
    /// How large a file is kept whole: `WHOLE`, or `WHOLE_FULL` under Full
    /// preview. A larger one asked whole is read and handed over, not kept.
    keeps: u64,
    head: Arc<[u8]>,
    whole: OnceLock<Arc<[u8]>>,
    tail: OnceLock<Arc<[u8]>>,
    /// Blocks read past the ends of a remote's file, the latest last.
    blocks: Mutex<Vec<(u64, Arc<[u8]>)>>,
    /// Taken while the file is read for what is kept, so two steps asking
    /// at once fetch it once.
    reading: Mutex<()>,
    /// Where a copy for a program is written, for a remote's file
    /// ([`Held::path_for_program`]): the preview's own folder.
    scratch: Option<PathBuf>,
    /// That copy, removed with what is held.
    copy: OnceLock<(tempfile::TempDir, PathBuf)>,
}

impl std::fmt::Debug for Held {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Held")
            .field("path", &self.path)
            .field("len", &self.stamp.len)
            .field("whole", &self.whole.get().is_some())
            .finish_non_exhaustive()
    }
}

fn check(cancel: &CancelToken) -> io::Result<()> {
    if cancel.is_cancelled() {
        return Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "preview cancelled",
        ));
    }
    Ok(())
}

impl Held {
    /// The file at `path`, its first bytes read (all of it when it is no
    /// longer): one open. On a worker, in the file's lane. A larger file
    /// is kept whole once asked, up to `keeps`.
    ///
    /// # Errors
    /// As `fs::file::open_regular` (`InvalidInput` for what is not a
    /// regular file: a pipe, a device), or the read's.
    pub fn open(path: &Path, keeps: u64) -> io::Result<Held> {
        let mut file = crate::fs::file::open_regular(path)?;
        let stamp = Stamp::of(&file)?;
        let mut head = Vec::with_capacity(usize::try_from(stamp.len).unwrap_or(HEAD).min(HEAD));
        (&mut file).take(HEAD as u64).read_to_end(&mut head)?;
        let short = head.len() < HEAD;
        let opened = Held {
            path: path.to_path_buf(),
            stamp,
            remote: crate::fs::reach::of(path).is_some(),
            keeps,
            head: head.into(),
            whole: OnceLock::new(),
            tail: OnceLock::new(),
            blocks: Mutex::new(Vec::new()),
            reading: Mutex::new(()),
            scratch: None,
            copy: OnceLock::new(),
        };
        // It ended before its head did: that is all of it.
        if short {
            let _ = opened.whole.set(Arc::clone(&opened.head));
        }
        Ok(opened)
    }

    /// `bytes` as the whole of a file named `path`, which is never read:
    /// a member a container wrote in memory, a test.
    pub fn of_bytes(path: impl Into<PathBuf>, bytes: impl Into<Arc<[u8]>>) -> Held {
        let bytes: Arc<[u8]> = bytes.into();
        let head: Arc<[u8]> = bytes[..bytes.len().min(HEAD)].into();
        let whole = OnceLock::new();
        let _ = whole.set(Arc::clone(&bytes));
        Held {
            path: path.into(),
            stamp: Stamp {
                len: bytes.len() as u64,
                modified: None,
            },
            remote: false,
            keeps: WHOLE_FULL,
            head,
            whole,
            tail: OnceLock::new(),
            blocks: Mutex::new(Vec::new()),
            reading: Mutex::new(()),
            scratch: None,
            copy: OnceLock::new(),
        }
    }

    /// `bytes` as the whole of a remote's file named `path`, never read,
    /// whose copy for a program is written into `scratch`
    /// ([`Held::path_for_program`]): a test's, for what hands a remote's
    /// file to a program (`SQLite`, a helper) from what is held.
    pub fn of_bytes_on_remote(
        path: impl Into<PathBuf>,
        bytes: impl Into<Arc<[u8]>>,
        scratch: PathBuf,
    ) -> Held {
        let mut held = Held::of_bytes(path, bytes);
        held.remote = true;
        held.scratch = Some(scratch);
        held
    }

    /// The file's own path: what names it (its extension, its language).
    /// Never read again by a provider: what is held is. A program is
    /// handed [`Held::path_for_program`].
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// What a program that reads the file by its path (Pandoc,
    /// `LibreOffice`, a helper) is handed: the file's own path on this
    /// machine's disks; for a remote's file held whole, a copy written
    /// from what is held into the preview's folder, once, removed with
    /// it, so the program does not fetch it again. A remote's file not
    /// held whole (or with nowhere to copy it) is handed by its path: the
    /// program reads it once, its own way.
    ///
    /// # Errors
    /// When the copy cannot be written.
    pub fn path_for_program(&self, cancel: &CancelToken) -> io::Result<Cow<'_, Path>> {
        if !self.remote {
            return Ok(Cow::Borrowed(&self.path));
        }
        if let Some((_, copy)) = self.copy.get() {
            return Ok(Cow::Borrowed(copy));
        }
        let Some(scratch) = &self.scratch else {
            return Ok(Cow::Borrowed(&self.path));
        };
        let Some(whole) = self.whole(self.keeps, cancel)? else {
            return Ok(Cow::Borrowed(&self.path));
        };
        let _reading = self.reading.lock();
        if let Some((_, copy)) = self.copy.get() {
            return Ok(Cow::Borrowed(copy));
        }
        std::fs::create_dir_all(scratch)?;
        let folder = tempfile::Builder::new()
            .prefix("held-")
            .tempdir_in(scratch)?;
        let name = self
            .path
            .file_name()
            .unwrap_or(std::ffi::OsStr::new("file"));
        let copy = folder.path().join(name);
        std::fs::write(&copy, &whole[..])?;
        let _ = self.copy.set((folder, copy));
        Ok(Cow::Borrowed(
            self.copy.get().map_or(&self.path, |(_, copy)| copy),
        ))
    }

    /// Its length when it was opened.
    pub fn len(&self) -> u64 {
        self.stamp.len
    }

    /// The version of the file held: its length and time modified as it
    /// was opened. What a cache keeps of the file is keyed by it, with
    /// its path: the version actually read, the path asked nothing.
    pub fn version(&self) -> (u64, Option<SystemTime>) {
        (self.stamp.len, self.stamp.modified)
    }

    pub fn is_empty(&self) -> bool {
        self.stamp.len == 0
    }

    /// How large a file it keeps whole: `WHOLE`, or `WHOLE_FULL` under
    /// Full preview.
    pub fn keeps(&self) -> u64 {
        self.keeps
    }

    /// Whether it is on a remote, where every read is a request.
    pub fn is_remote(&self) -> bool {
        self.remote
    }

    /// Its first bytes, at most `HEAD`: all of it for a short file.
    pub fn head(&self) -> &[u8] {
        &self.head
    }

    /// All of it, if it was read whole.
    pub fn held_whole(&self) -> Option<&Arc<[u8]>> {
        self.whole.get()
    }

    /// The bytes it keeps in memory now.
    pub fn kept(&self) -> usize {
        let whole = self.whole.get().map_or(0, |w| w.len());
        let head = if whole > 0 { 0 } else { self.head.len() };
        let tail = self.tail.get().map_or(0, |t| t.len());
        let blocks: usize = self.blocks.lock().iter().map(|(_, b)| b.len()).sum();
        whole + head + tail + blocks
    }

    /// The file opened again for a read: refused when it is another
    /// version than the one held.
    fn reopen(&self) -> io::Result<File> {
        let file = crate::fs::file::open_regular(&self.path)?;
        if Stamp::of(&file)? != self.stamp {
            return Err(io::Error::other(format!(
                "{} changed while it was shown",
                self.path.display()
            )));
        }
        Ok(file)
    }

    /// All of it, when it is no longer than `limit`: read once (the rest
    /// after its head) and kept, up to what this preview keeps. `None`
    /// past `limit`, unread.
    ///
    /// # Errors
    /// The read's, a version other than the one held, or a cancel.
    pub fn whole(&self, limit: u64, cancel: &CancelToken) -> io::Result<Option<Arc<[u8]>>> {
        if let Some(whole) = self.whole.get() {
            return Ok((whole.len() as u64 <= limit).then(|| Arc::clone(whole)));
        }
        if self.stamp.len > limit {
            return Ok(None);
        }
        let _reading = self.reading.lock();
        if let Some(whole) = self.whole.get() {
            return Ok(Some(Arc::clone(whole)));
        }
        check(cancel)?;
        let mut file = self.reopen()?;
        file.seek(SeekFrom::Start(self.head.len() as u64))?;
        let len = usize::try_from(self.stamp.len).map_err(io::Error::other)?;
        let mut bytes = Vec::with_capacity(len);
        bytes.extend_from_slice(&self.head);
        let mut chunk = vec![0; CHUNK];
        loop {
            check(cancel)?;
            let n = file.read(&mut chunk)?;
            if n == 0 {
                break;
            }
            bytes.extend_from_slice(&chunk[..n]);
            if bytes.len() as u64 > limit {
                // It grew since it was opened: another version.
                return Err(io::Error::other(format!(
                    "{} changed while it was shown",
                    self.path.display()
                )));
            }
        }
        let bytes: Arc<[u8]> = bytes.into();
        if bytes.len() as u64 <= self.keeps {
            let _ = self.whole.set(Arc::clone(&bytes));
        }
        Ok(Some(bytes))
    }

    /// Its last bytes, at most `TAIL`: read once and kept.
    ///
    /// # Errors
    /// The read's, or a version other than the one held.
    pub fn tail(&self) -> io::Result<Arc<[u8]>> {
        if let Some(whole) = self.whole.get() {
            let from = whole.len().saturating_sub(TAIL);
            return Ok(whole[from..].into());
        }
        if let Some(tail) = self.tail.get() {
            return Ok(Arc::clone(tail));
        }
        let _reading = self.reading.lock();
        if let Some(tail) = self.tail.get() {
            return Ok(Arc::clone(tail));
        }
        let from = self.stamp.len.saturating_sub(TAIL as u64);
        let mut file = self.reopen()?;
        file.seek(SeekFrom::Start(from))?;
        let mut tail = Vec::with_capacity(TAIL);
        file.take(TAIL as u64).read_to_end(&mut tail)?;
        let tail: Arc<[u8]> = tail.into();
        let _ = self.tail.set(Arc::clone(&tail));
        Ok(tail)
    }

    /// Up to `buf.len()` bytes from `offset`, as `pread` reads them: from
    /// memory where held; on a remote, through the blocks kept; else from
    /// the file, opened for the read. Fewer than asked only at its end, or
    /// where what is held ends (read again from there).
    ///
    /// # Errors
    /// The read's, or a version other than the one held.
    pub fn read_at(&self, offset: u64, buf: &mut [u8]) -> io::Result<usize> {
        if let Some(n) = self.in_memory(offset, buf) {
            return Ok(n);
        }
        if self.remote {
            return self.through_blocks(offset, buf);
        }
        let mut file = self.reopen()?;
        file.seek(SeekFrom::Start(offset))?;
        read_full(&mut file, buf)
    }

    /// What memory holds from `offset`: the whole file, its head or its
    /// tail. `None` where none of them does.
    fn in_memory(&self, offset: u64, buf: &mut [u8]) -> Option<usize> {
        let copy = |bytes: &[u8], start: u64, buf: &mut [u8]| {
            let at = usize::try_from(offset - start).ok()?;
            let rest = bytes.get(at..)?;
            let n = rest.len().min(buf.len());
            buf[..n].copy_from_slice(&rest[..n]);
            Some(n)
        };
        if let Some(whole) = self.whole.get() {
            return Some(copy(whole, 0, buf).unwrap_or(0));
        }
        if offset < self.head.len() as u64 {
            return copy(&self.head, 0, buf);
        }
        let tail = self.tail.get()?;
        let start = self.stamp.len - tail.len() as u64;
        (offset >= start).then(|| copy(tail, start, buf)).flatten()
    }

    /// A remote's file past its ends, a block at a time, the latest kept.
    fn through_blocks(&self, offset: u64, buf: &mut [u8]) -> io::Result<usize> {
        if offset >= self.stamp.len {
            return Ok(0);
        }
        let start = offset - offset % BLOCK;
        let found = {
            let mut blocks = self.blocks.lock();
            blocks.iter().position(|(at, _)| *at == start).map(|i| {
                let block = blocks.remove(i);
                blocks.push(block.clone());
                block.1
            })
        };
        let block = if let Some(block) = found {
            block
        } else {
            let mut file = self.reopen()?;
            file.seek(SeekFrom::Start(start))?;
            let mut block = Vec::with_capacity(BLOCK as usize);
            file.take(BLOCK).read_to_end(&mut block)?;
            let block: Arc<[u8]> = block.into();
            let mut blocks = self.blocks.lock();
            if blocks.len() >= BLOCKS {
                blocks.remove(0);
            }
            blocks.push((start, Arc::clone(&block)));
            block
        };
        let at = usize::try_from(offset - start).map_err(io::Error::other)?;
        let rest = block.get(at..).unwrap_or_default();
        let n = rest.len().min(buf.len());
        buf[..n].copy_from_slice(&rest[..n]);
        Ok(n)
    }

    /// A reader of it from its start, `Read` and `Seek`, for a library
    /// that takes one (a zip, a picture's decoder): from memory where held;
    /// else it opens the file once for its own life, which ends with the
    /// step that asked for it.
    pub fn reader(self: &Arc<Held>) -> Reader {
        Reader {
            held: Arc::clone(self),
            at: 0,
            file: None,
        }
    }
}

/// Read until `buf` is full or the file ends.
fn read_full(file: &mut impl Read, buf: &mut [u8]) -> io::Result<usize> {
    let mut n = 0;
    while n < buf.len() {
        match file.read(&mut buf[n..]) {
            Ok(0) => break,
            Ok(k) => n += k,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(n)
}

/// A reader of what is held ([`Held::reader`]).
pub struct Reader {
    held: Arc<Held>,
    at: u64,
    /// The file, opened once for the reads memory does not answer on a
    /// disk, and where it was left.
    file: Option<(File, u64)>,
}

impl Read for Reader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = if let Some(n) = self.held.in_memory(self.at, buf) {
            n
        } else if self.held.remote {
            self.held.through_blocks(self.at, buf)?
        } else {
            let (mut file, left) = match self.file.take() {
                Some(open) => open,
                None => (self.held.reopen()?, 0),
            };
            if left != self.at {
                file.seek(SeekFrom::Start(self.at))?;
            }
            let n = file.read(buf)?;
            self.file = Some((file, self.at + n as u64));
            n
        };
        self.at += n as u64;
        Ok(n)
    }
}

impl Seek for Reader {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        let at = match to {
            SeekFrom::Start(at) => Some(at),
            SeekFrom::End(by) => self.held.stamp.len.checked_add_signed(by),
            SeekFrom::Current(by) => self.at.checked_add_signed(by),
        };
        self.at = at.ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "a seek before the start")
        })?;
        Ok(self.at)
    }
}

/// Where a reader takes a file's bytes from: its path, opened for each
/// read as before (a thumbnail's, a field's), or what a preview holds of
/// it ([`Held`]), which reads the file once. The platform's readers take
/// either, so a provider hands over what it holds and nothing else reads
/// the file again.
pub trait FileSource: Sync {
    /// What names the file: its extension, its language.
    fn path(&self) -> &Path;
    /// Its length.
    ///
    /// # Errors
    /// The file's metadata's.
    fn length(&self) -> io::Result<u64>;
    /// Its first bytes, at most `HEAD`.
    ///
    /// # Errors
    /// The read's.
    fn first_bytes(&self) -> io::Result<Cow<'_, [u8]>>;
    /// Up to `limit` bytes from `offset`.
    ///
    /// # Errors
    /// The read's, or a cancel.
    fn read_range(
        &self,
        offset: u64,
        limit: usize,
        cancel: &CancelToken,
    ) -> io::Result<Cow<'_, [u8]>>;
    /// All of it, when it is no longer than `limit`; `None` past it.
    ///
    /// # Errors
    /// The read's, or a cancel.
    fn read_whole(&self, limit: u64, cancel: &CancelToken) -> io::Result<Option<Cow<'_, [u8]>>>;
    /// A reader from its start, of its own: it may outlive the call (a
    /// zip kept open).
    ///
    /// # Errors
    /// The open's.
    fn open_reader(&self) -> io::Result<Box<dyn ReadSeek + Send>>;
    /// What a program that reads the file by its path is handed: its path,
    /// or what a preview holds of a remote's file, copied
    /// ([`Held::path_for_program`]).
    ///
    /// # Errors
    /// When a copy cannot be written.
    fn path_for_program(&self, _cancel: &CancelToken) -> io::Result<Cow<'_, Path>> {
        Ok(Cow::Borrowed(self.path()))
    }
}

/// What a library that reads and seeks takes.
pub trait ReadSeek: Read + Seek {}
impl<T: Read + Seek> ReadSeek for T {}

impl FileSource for Arc<Held> {
    fn path(&self) -> &Path {
        &self.path
    }
    fn length(&self) -> io::Result<u64> {
        Ok(self.stamp.len)
    }
    fn first_bytes(&self) -> io::Result<Cow<'_, [u8]>> {
        Ok(Cow::Borrowed(&self.head))
    }
    fn read_range(
        &self,
        offset: u64,
        limit: usize,
        cancel: &CancelToken,
    ) -> io::Result<Cow<'_, [u8]>> {
        check(cancel)?;
        let end = offset.saturating_add(limit as u64).min(self.stamp.len);
        if let Some(whole) = self.whole.get() {
            let from = usize::try_from(offset.min(end)).map_err(io::Error::other)?;
            let to = usize::try_from(end).map_err(io::Error::other)?;
            return Ok(Cow::Borrowed(&whole[from..to]));
        }
        if end <= self.head.len() as u64 {
            let from = usize::try_from(offset.min(end)).map_err(io::Error::other)?;
            let to = usize::try_from(end).map_err(io::Error::other)?;
            return Ok(Cow::Borrowed(&self.head[from..to]));
        }
        let mut out =
            vec![0; usize::try_from(end.saturating_sub(offset)).map_err(io::Error::other)?];
        let mut n = 0;
        while n < out.len() {
            check(cancel)?;
            let k = self.read_at(offset + n as u64, &mut out[n..])?;
            if k == 0 {
                break;
            }
            n += k;
        }
        out.truncate(n);
        Ok(Cow::Owned(out))
    }
    fn read_whole(&self, limit: u64, cancel: &CancelToken) -> io::Result<Option<Cow<'_, [u8]>>> {
        if let Some(whole) = self.whole.get() {
            return Ok((whole.len() as u64 <= limit).then_some(Cow::Borrowed(&whole[..])));
        }
        Ok(
            Held::whole(self, limit, cancel)?.map(|bytes| match self.whole.get() {
                Some(kept) => Cow::Borrowed(&kept[..]),
                None => Cow::Owned(bytes.to_vec()),
            }),
        )
    }
    fn open_reader(&self) -> io::Result<Box<dyn ReadSeek + Send>> {
        Ok(Box::new(Held::reader(self)))
    }
    fn path_for_program(&self, cancel: &CancelToken) -> io::Result<Cow<'_, Path>> {
        Held::path_for_program(self, cancel)
    }
}

impl FileSource for Path {
    fn path(&self) -> &Path {
        self
    }
    fn length(&self) -> io::Result<u64> {
        Ok(std::fs::metadata(self)?.len())
    }
    fn first_bytes(&self) -> io::Result<Cow<'_, [u8]>> {
        let mut head = Vec::new();
        crate::fs::file::open_regular(self)?
            .take(HEAD as u64)
            .read_to_end(&mut head)?;
        Ok(Cow::Owned(head))
    }
    fn read_range(
        &self,
        offset: u64,
        limit: usize,
        cancel: &CancelToken,
    ) -> io::Result<Cow<'_, [u8]>> {
        check(cancel)?;
        let mut file = crate::fs::file::open_regular(self)?;
        file.seek(SeekFrom::Start(offset))?;
        let mut out = Vec::new();
        file.take(limit as u64).read_to_end(&mut out)?;
        check(cancel)?;
        Ok(Cow::Owned(out))
    }
    fn read_whole(&self, limit: u64, cancel: &CancelToken) -> io::Result<Option<Cow<'_, [u8]>>> {
        check(cancel)?;
        let file = crate::fs::file::open_regular(self)?;
        if file.metadata()?.len() > limit {
            return Ok(None);
        }
        let mut out = Vec::new();
        file.take(limit.saturating_add(1)).read_to_end(&mut out)?;
        check(cancel)?;
        Ok((out.len() as u64 <= limit).then_some(Cow::Owned(out)))
    }
    fn open_reader(&self) -> io::Result<Box<dyn ReadSeek + Send>> {
        Ok(Box::new(crate::fs::file::open_regular(self)?))
    }
}

impl FileSource for PathBuf {
    fn path(&self) -> &Path {
        self
    }
    fn length(&self) -> io::Result<u64> {
        self.as_path().length()
    }
    fn first_bytes(&self) -> io::Result<Cow<'_, [u8]>> {
        self.as_path().first_bytes()
    }
    fn read_range(
        &self,
        offset: u64,
        limit: usize,
        cancel: &CancelToken,
    ) -> io::Result<Cow<'_, [u8]>> {
        self.as_path().read_range(offset, limit, cancel)
    }
    fn read_whole(&self, limit: u64, cancel: &CancelToken) -> io::Result<Option<Cow<'_, [u8]>>> {
        self.as_path().read_whole(limit, cancel)
    }
    fn open_reader(&self) -> io::Result<Box<dyn ReadSeek + Send>> {
        self.as_path().open_reader()
    }
}

impl FileSource for Cow<'_, Path> {
    fn path(&self) -> &Path {
        self
    }
    fn length(&self) -> io::Result<u64> {
        Path::length(self)
    }
    fn first_bytes(&self) -> io::Result<Cow<'_, [u8]>> {
        Path::first_bytes(self)
    }
    fn read_range(
        &self,
        offset: u64,
        limit: usize,
        cancel: &CancelToken,
    ) -> io::Result<Cow<'_, [u8]>> {
        Path::read_range(self, offset, limit, cancel)
    }
    fn read_whole(&self, limit: u64, cancel: &CancelToken) -> io::Result<Option<Cow<'_, [u8]>>> {
        Path::read_whole(self, limit, cancel)
    }
    fn open_reader(&self) -> io::Result<Box<dyn ReadSeek + Send>> {
        Path::open_reader(self)
    }
}

impl Drop for Held {
    fn drop(&mut self) {
        let large = self
            .whole
            .get()
            .is_some_and(|whole| whole.len() >= BLOCK as usize);
        // Let go of what it holds first: a worker read it, and keeps what
        // another thread frees of it until it releases.
        drop(self.whole.take());
        if large {
            crate::scheduler::release_on_workers();
        }
        // The copy a program read goes on a thread of its own, never the
        // one letting go (the window's, maybe), and again a moment later
        // while it is refused: on Windows, while the program that read it
        // still has it open.
        if let Some((folder, _)) = self.copy.take() {
            let removed = std::thread::Builder::new()
                .name("skwir-held-copy".into())
                .spawn(move || remove_copy(folder));
            if let Err(error) = removed {
                tracing::debug!("the copy held for a program stays: {error}");
            }
        }
    }
}

/// Remove `folder`, a copy for a program, trying again a few times.
fn remove_copy(folder: tempfile::TempDir) {
    let path = folder.keep();
    for _ in 0..5 {
        match std::fs::remove_dir_all(&path) {
            Ok(()) => return,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return,
            Err(_) => std::thread::sleep(std::time::Duration::from_secs(1)),
        }
    }
    tracing::debug!(
        "the copy held for a program at {} stays until the cache is swept",
        path.display()
    );
}

/// Where a preview request keeps what was read of its file, shared by its
/// clones (the job, the pictures asked for later, the reader): read on the
/// first ask, for the entry asked about, and what each provider's
/// `accepts` answered for it, so each is asked once.
#[derive(Clone)]
pub struct HeldSlot(Arc<Slot>);

struct Slot {
    keeps: u64,
    /// Where a remote's file is copied for a program.
    scratch: Option<PathBuf>,
    held: Mutex<Option<(crate::op::EntryRef, Arc<Held>)>>,
    /// What providers answered (`accepts`), by the entry and the type asked
    /// about, the provider and the registry's revision.
    accepted: Mutex<Vec<(Asked, bool)>>,
}

/// What an `accepts` was asked about.
#[derive(PartialEq)]
struct Asked {
    entry: crate::op::EntryRef,
    mime: Option<String>,
    provider: &'static str,
    revision: u64,
}

impl Default for HeldSlot {
    fn default() -> Self {
        HeldSlot::keeping(WHOLE)
    }
}

impl std::fmt::Debug for HeldSlot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let held = self.0.held.lock();
        f.debug_tuple("HeldSlot")
            .field(&held.as_ref().map(|(_, held)| held))
            .finish()
    }
}

impl HeldSlot {
    /// A slot whose file is kept whole up to `keeps` (`WHOLE`, or
    /// `WHOLE_FULL` under Full preview).
    pub fn keeping(keeps: u64) -> HeldSlot {
        HeldSlot::copying(keeps, None)
    }

    /// A slot whose file is kept whole up to `keeps`, a remote's copied
    /// into `scratch` for a program (the preview's own folder).
    pub fn copying(keeps: u64, scratch: Option<PathBuf>) -> HeldSlot {
        HeldSlot(Arc::new(Slot {
            keeps,
            scratch,
            held: Mutex::new(None),
            accepted: Mutex::new(Vec::new()),
        }))
    }

    /// An empty slot like this one: for another file a request is asked
    /// about (`PreviewRequest::retarget`).
    pub fn emptied(&self) -> HeldSlot {
        HeldSlot::copying(self.0.keeps, self.0.scratch.clone())
    }

    /// A slot that already holds `held`, for `entry`: what the preview
    /// shown before kept, or a test's bytes.
    pub fn holding(entry: crate::op::EntryRef, held: Arc<Held>) -> HeldSlot {
        let slot = HeldSlot::copying(held.keeps, held.scratch.clone());
        *slot.0.held.lock() = Some((entry, held));
        slot
    }

    /// How large a file it keeps whole.
    pub fn keeps(&self) -> u64 {
        self.0.keeps
    }

    /// What it holds now, if it was read.
    pub fn get(&self) -> Option<Arc<Held>> {
        self.0
            .held
            .lock()
            .as_ref()
            .map(|(_, held)| Arc::clone(held))
    }

    /// What is held of `entry`'s file, read now if it was not (a version
    /// of another entry is let go of).
    pub(crate) fn of(&self, entry: &crate::op::EntryRef) -> io::Result<Arc<Held>> {
        let mut held = self.0.held.lock();
        if let Some((of, held)) = held.as_ref()
            && of == entry
        {
            return Ok(Arc::clone(held));
        }
        let path = entry.resolve().map_err(|error| {
            let kind = match error {
                crate::op::ResolveError::Gone(_) => io::ErrorKind::NotFound,
                crate::op::ResolveError::Replaced { .. } => io::ErrorKind::Other,
            };
            io::Error::new(kind, error.to_string())
        })?;
        let mut read = Held::open(&path, self.0.keeps)?;
        read.scratch.clone_from(&self.0.scratch);
        let read = Arc::new(read);
        *held = Some((entry.clone(), Arc::clone(&read)));
        Ok(read)
    }

    /// What `provider` answered for `entry` asked about as `mime`, asked
    /// once.
    pub(crate) fn accepted(
        &self,
        entry: &crate::op::EntryRef,
        mime: Option<&str>,
        provider: &'static str,
        revision: u64,
        ask: impl FnOnce() -> bool,
    ) -> bool {
        let asked = Asked {
            entry: entry.clone(),
            mime: mime.map(str::to_owned),
            provider,
            revision,
        };
        let found = self
            .0
            .accepted
            .lock()
            .iter()
            .find(|(of, _)| *of == asked)
            .map(|(_, answer)| *answer);
        if let Some(answer) = found {
            return answer;
        }
        // Asked unlocked: `accepts` reads what is held, which takes the
        // slot's other lock.
        let answer = ask();
        self.0.accepted.lock().push((asked, answer));
        answer
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }

    #[test]
    fn a_short_file_is_held_whole_at_once_and_a_long_one_when_asked() {
        let dir = tempfile::tempdir().unwrap();
        let cancel = CancelToken::new();
        let short = Arc::new(Held::open(&file(dir.path(), "l'été.txt", b"short"), WHOLE).unwrap());
        assert_eq!(short.head(), b"short");
        assert_eq!(short.held_whole().map(|w| &w[..]), Some(&b"short"[..]));
        let long: Vec<u8> = (0..HEAD * 3).map(|n| (n % 251) as u8).collect();
        let path = file(dir.path(), "a long one.bin", &long);
        let held = Arc::new(Held::open(&path, WHOLE).unwrap());
        assert_eq!(held.len(), long.len() as u64);
        assert_eq!(held.head(), &long[..HEAD]);
        assert!(held.held_whole().is_none(), "only its head, until asked");
        assert!(held.whole(100, &cancel).unwrap().is_none(), "past a limit");
        assert_eq!(&held.whole(WHOLE, &cancel).unwrap().unwrap()[..], &long[..]);
        assert!(held.held_whole().is_some(), "kept once read");
        // Read from memory now: the file gone, every read answers.
        std::fs::remove_file(&path).unwrap();
        let mut buf = vec![0; 10];
        assert_eq!(held.read_at(HEAD as u64 * 2, &mut buf).unwrap(), 10);
        assert_eq!(buf, long[HEAD * 2..HEAD * 2 + 10]);
        let mut all = Vec::new();
        held.reader().read_to_end(&mut all).unwrap();
        assert_eq!(all, long);
        assert_eq!(&held.tail().unwrap()[..], &long[long.len() - TAIL..]);
    }

    #[test]
    fn another_version_of_the_file_is_refused_never_mixed_in() {
        let dir = tempfile::tempdir().unwrap();
        let long = vec![b'a'; HEAD * 2];
        let path = file(dir.path(), "changing.log", &long);
        let held = Arc::new(Held::open(&path, WHOLE).unwrap());
        // Written anew, longer: what is past the head is another version's.
        std::fs::write(&path, vec![b'b'; HEAD * 3]).unwrap();
        let error = held.whole(WHOLE, &CancelToken::new()).unwrap_err();
        assert!(
            error.to_string().contains("changed while it was shown"),
            "{error}"
        );
        let mut buf = vec![0; 4];
        assert!(held.read_at(HEAD as u64 + 1, &mut buf).is_err());
        // What was read of the version held still answers.
        assert_eq!(held.read_at(0, &mut buf).unwrap(), 4);
        assert_eq!(buf, b"aaaa");
    }

    #[test]
    fn a_reader_seeks_through_memory_and_the_file_alike() {
        let dir = tempfile::tempdir().unwrap();
        let bytes: Vec<u8> = (0..HEAD * 2 + 7).map(|n| (n % 13) as u8).collect();
        let held = Arc::new(Held::open(&file(dir.path(), "seek.bin", &bytes), WHOLE).unwrap());
        let mut reader = held.reader();
        let mut buf = [0u8; 5];
        reader.seek(SeekFrom::End(-5)).unwrap();
        reader.read_exact(&mut buf).unwrap();
        assert_eq!(buf, bytes[bytes.len() - 5..]);
        reader.seek(SeekFrom::Start(3)).unwrap();
        reader.read_exact(&mut buf).unwrap();
        assert_eq!(buf, bytes[3..8]);
        let step = i64::try_from(HEAD).unwrap();
        reader.seek(SeekFrom::Current(step)).unwrap();
        reader.read_exact(&mut buf).unwrap();
        assert_eq!(buf, bytes[HEAD + 8..HEAD + 13]);
        assert!(reader.seek(SeekFrom::Current(-step * 4)).is_err());
    }

    // A remote is pretended on Linux alone.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_remote_file_is_read_by_blocks_and_copied_for_a_program_once() {
        let dir = tempfile::tempdir().unwrap();
        let point = dir.path().join("server");
        std::fs::create_dir(&point).unwrap();
        crate::fs::reach::pretend_remote(&point);
        let cancel = CancelToken::new();
        let large: Vec<u8> = (0..(BLOCK as usize) * 2 + HEAD)
            .map(|n| (n % 241) as u8)
            .collect();
        let path = file(&point, "large.bin", &large);
        let held = Arc::new(Held::open(&path, HEAD as u64).unwrap());
        assert!(held.is_remote());
        let mut buf = vec![0; 16];
        let at = BLOCK + 100;
        assert_eq!(held.read_at(at, &mut buf).unwrap(), 16);
        assert_eq!(buf, large[at as usize..at as usize + 16]);
        assert_eq!(
            held.kept(),
            HEAD + BLOCK as usize,
            "its head and the block read"
        );
        // Past what it keeps whole: a program reads it by its own path.
        assert_eq!(held.path_for_program(&cancel).unwrap(), path.as_path());
        let scratch = dir.path().join("scratch");
        let small = Held::of_bytes_on_remote(point.join("gone.txt"), &b"held"[..], scratch.clone());
        let copy = small.path_for_program(&cancel).unwrap().into_owned();
        assert!(copy.starts_with(&scratch));
        assert_eq!(std::fs::read(&copy).unwrap(), b"held");
        assert_eq!(
            small.path_for_program(&cancel).unwrap(),
            copy.as_path(),
            "once"
        );
        drop(small);
        // On a thread of its own.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while copy.exists() {
            assert!(
                std::time::Instant::now() < deadline,
                "removed with what is held"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    #[test]
    fn a_request_reads_its_file_once_for_every_clone_and_provider() {
        let dir = tempfile::tempdir().unwrap();
        let path = file(dir.path(), "once.txt", b"once");
        let entry = crate::op::EntryRef::from_path(&path).unwrap();
        let slot = HeldSlot::default();
        let first = slot.of(&entry).unwrap();
        assert!(Arc::ptr_eq(&slot.clone().of(&entry).unwrap(), &first));
        let mut asked = 0;
        for _ in 0..3 {
            assert!(slot.accepted(&entry, Some("text/plain"), "geo", 1, || {
                asked += 1;
                true
            }));
        }
        assert_eq!(asked, 1, "each provider asked once");
        // Another revision of the providers, or another type: asked again.
        slot.accepted(&entry, Some("text/plain"), "geo", 2, || {
            asked += 1;
            false
        });
        slot.accepted(&entry, Some("text/csv"), "geo", 2, || {
            asked += 1;
            false
        });
        assert_eq!(asked, 3);
    }
}
