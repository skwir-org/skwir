//! The phone as a folder (ADR-097): kmtpd's storages served through FUSE,
//! one request at a time as kmtpd serves them. Each storage is a folder by
//! its description; a folder is listed by kmtpd each time it is read
//! (`readdir`), and what is looked up in it is found in that listing. A
//! file opened is fetched whole into an unnamed file of the cache (MTP
//! reads no part of a file through kmtpd), read and written there, and
//! sent whole when closed if written: MTP replaces nothing, so what was
//! there is removed first. A move between folders is refused (`EXDEV`:
//! copies are written in place there, ADR-083), as are times and modes
//! (the phone stamps what it is sent). Inode numbers are made up, kept for
//! as long as the mount lives.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs::File;
use std::io;
use std::os::unix::fs::{FileExt, MetadataExt};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use fuser::{
    Errno, FileAttr, FileHandle, FileType, Filesystem, FopenFlags, Generation, INodeNo, LockOwner,
    MountOption, OpenAccMode, OpenFlags, RenameFlags, ReplyAttr, ReplyCreate, ReplyData,
    ReplyDirectory, ReplyEmpty, ReplyEntry, ReplyOpen, ReplyStatfs, ReplyWrite, Request, TimeOrNow,
    WriteFlags,
};
use zbus::blocking::Connection;

use crate::kmtpd::{self, Storage};

/// How long the kernel keeps what it was told of an entry before asking
/// again: what changes on the phone itself is seen once a folder is read
/// again, which this does not hold up.
const TTL: Duration = Duration::from_secs(1);

/// The mount's top, which holds the storages.
const TOP: u64 = 1;

/// The type of the mount in the system's table (`fuse.skwir-kmtp`), by
/// which Skwir knows it as a device's (`fs::reach`).
pub const SUBTYPE: &str = "skwir-kmtp";

/// The system's program that mounts FUSE for a user, on `PATH`.
pub fn fusermount() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    ["fusermount3", "fusermount"].iter().find_map(|name| {
        std::env::split_paths(&path)
            .map(|folder| folder.join(name))
            .find(|program| program.is_file())
    })
}

/// The phone kmtpd holds, as a file system.
pub struct Phone {
    bus: Connection,
    /// Each storage, by the name its folder has.
    storages: Vec<(String, Storage)>,
    /// Where what is read or written is kept, unnamed.
    cache: PathBuf,
    /// Who owns what it shows: the user who mounted it.
    owner: (u32, u32),
    /// When it was mounted: the time of what has none (its top).
    since: SystemTime,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    nodes: HashMap<u64, Node>,
    /// Each entry's number, by its storage and path.
    numbers: HashMap<(usize, String), u64>,
    next: u64,
    open: HashMap<u64, Opened>,
    next_handle: u64,
}

#[derive(Clone, Debug)]
struct Node {
    /// Its storage, `None` for the top.
    storage: Option<usize>,
    /// Its path in its storage (`/` for the storage's top).
    path: String,
    folder: bool,
    size: u64,
    modified: SystemTime,
    /// On the phone already: what is written there replaces it.
    on_phone: bool,
    /// A folder's entries as last listed; `None` until it is.
    entries: Option<Vec<String>>,
}

/// A file open: its copy, and whether it was written since it was sent.
struct Opened {
    number: u64,
    file: File,
    written: bool,
}

/// A path below `folder` in a storage.
fn below(folder: &str, name: &str) -> String {
    if folder == "/" {
        format!("/{name}")
    } else {
        format!("{folder}/{name}")
    }
}

/// What a failure is to the kernel.
fn errno(error: &io::Error) -> Errno {
    match error.kind() {
        io::ErrorKind::NotFound => Errno::ENOENT,
        io::ErrorKind::NotADirectory => Errno::ENOTDIR,
        io::ErrorKind::IsADirectory => Errno::EISDIR,
        io::ErrorKind::AlreadyExists => Errno::EEXIST,
        io::ErrorKind::DirectoryNotEmpty => Errno::ENOTEMPTY,
        io::ErrorKind::CrossesDevices => Errno::EXDEV,
        io::ErrorKind::Unsupported => Errno::ENOTSUP,
        io::ErrorKind::InvalidInput => Errno::EINVAL,
        _ => Errno::EIO,
    }
}

/// Distinct names for `descriptions` (two storages may say the same):
/// the second `name (2)`, and so on.
pub fn distinct(descriptions: &[String]) -> Vec<String> {
    let mut names: Vec<String> = Vec::with_capacity(descriptions.len());
    for description in descriptions {
        let base = if description.trim().is_empty() || description.contains('/') {
            "Storage".to_owned()
        } else {
            description.clone()
        };
        let mut name = base.clone();
        let mut n = 2;
        while names.contains(&name) {
            name = format!("{base} ({n})");
            n += 1;
        }
        names.push(name);
    }
    names
}

impl Phone {
    /// Mount the device kmtpd knows by `udi` at `point`, what it reads and
    /// writes kept in `cache`.
    ///
    /// # Errors
    /// When kmtpd does not hold it, it shows no storage (locked, or not in
    /// file transfer mode), or the system did not mount it.
    pub fn mount(udi: &str, point: &Path, cache: &Path) -> io::Result<fuser::Session<Phone>> {
        let bus = kmtpd::connect()?;
        let device = kmtpd::device(&bus, udi)?
            .ok_or_else(|| io::Error::other("KDE's kmtpd does not hold this device"))?;
        let storages = kmtpd::storages(&bus, &device)?;
        if storages.is_empty() {
            return Err(io::Error::other(
                "the phone shows no storage: unlock it, and choose file transfer on it",
            ));
        }
        std::fs::create_dir_all(cache)?;
        let held = std::fs::metadata(point)?;
        let names = distinct(
            &storages
                .iter()
                .map(|s| s.description.clone())
                .collect::<Vec<_>>(),
        );
        let since = SystemTime::now();
        let mut state = State {
            next: TOP + 1,
            next_handle: 1,
            ..State::default()
        };
        state.nodes.insert(
            TOP,
            Node {
                storage: None,
                path: String::new(),
                folder: true,
                size: 0,
                modified: since,
                on_phone: true,
                entries: Some(names.clone()),
            },
        );
        for index in 0..storages.len() {
            let number = state.number(index, "/");
            state.nodes.insert(
                number,
                Node {
                    storage: Some(index),
                    path: "/".to_owned(),
                    folder: true,
                    size: 0,
                    modified: since,
                    on_phone: true,
                    entries: None,
                },
            );
        }
        let phone = Phone {
            bus,
            storages: names.into_iter().zip(storages).collect(),
            cache: cache.to_path_buf(),
            owner: (held.uid(), held.gid()),
            since,
            state: Mutex::new(state),
        };
        let mut config = fuser::Config::default();
        config.mount_options = vec![
            MountOption::FSName("kmtpd".to_owned()),
            MountOption::Subtype(SUBTYPE.to_owned()),
            MountOption::DefaultPermissions,
            MountOption::NoDev,
            MountOption::NoSuid,
            MountOption::NoExec,
            MountOption::NoAtime,
        ];
        if rustix::process::geteuid().is_root() {
            mount_as_root(phone, point, config)
        } else {
            fuser::Session::new(phone, point, &config)
        }
    }

    fn attr(&self, number: u64, node: &Node) -> FileAttr {
        FileAttr {
            ino: INodeNo(number),
            size: node.size,
            blocks: node.size.div_ceil(512),
            atime: node.modified,
            mtime: node.modified,
            ctime: node.modified,
            crtime: node.modified,
            kind: if node.folder {
                FileType::Directory
            } else {
                FileType::RegularFile
            },
            perm: if node.folder { 0o755 } else { 0o644 },
            nlink: if node.folder { 2 } else { 1 },
            uid: self.owner.0,
            gid: self.owner.1,
            rdev: 0,
            blksize: 4096,
            flags: 0,
        }
    }

    /// An unnamed file of the cache, gone with its last close: its folder
    /// made again first, as the cache may be emptied at any time.
    fn scratch(&self) -> io::Result<File> {
        std::fs::create_dir_all(&self.cache)?;
        tempfile::tempfile_in(&self.cache)
    }

    fn storage(&self, index: usize) -> &Storage {
        &self.storages[index].1
    }

    /// The folder `number`'s entries as kmtpd lists them now, each made a
    /// node; what is no longer there is forgotten.
    fn list(&self, state: &mut State, number: u64) -> io::Result<Vec<String>> {
        let node = state.node(number)?;
        let Some(index) = node.storage else {
            return Ok(self.storages.iter().map(|(name, _)| name.clone()).collect());
        };
        if !node.folder {
            return Err(io::ErrorKind::NotADirectory.into());
        }
        let listed = kmtpd::list(&self.bus, self.storage(index), &node.path)?;
        let mut names = Vec::with_capacity(listed.len());
        for entry in &listed {
            let path = below(&node.path, &entry.3);
            let child = state.number(index, &path);
            let folder = kmtpd::is_folder(entry);
            let entries = state
                .nodes
                .get(&child)
                .filter(|kept| kept.folder == folder)
                .and_then(|kept| kept.entries.clone());
            state.nodes.insert(
                child,
                Node {
                    storage: Some(index),
                    path,
                    folder,
                    size: if folder { 0 } else { entry.4 },
                    modified: UNIX_EPOCH
                        + Duration::from_secs(u64::try_from(entry.5).unwrap_or_default()),
                    on_phone: true,
                    entries,
                },
            );
            names.push(entry.3.clone());
        }
        if let Some(gone) = node.entries {
            for name in gone.iter().filter(|name| !names.contains(name)) {
                state.forget(index, &below(&node.path, name));
            }
        }
        state.node_mut(number)?.entries = Some(names.clone());
        Ok(names)
    }

    /// The entry `name` of the folder `parent`: its number and node, from
    /// the folder's last listing (listed now if never).
    fn find(&self, state: &mut State, parent: u64, name: &OsStr) -> io::Result<(u64, Node)> {
        let name = name.to_str().ok_or(io::ErrorKind::NotFound)?;
        let folder = state.node(parent)?;
        let names = match folder.entries {
            Some(names) => names,
            None => self.list(state, parent)?,
        };
        if !names.iter().any(|n| n == name) {
            return Err(io::ErrorKind::NotFound.into());
        }
        let number = match folder.storage {
            None => {
                let index = self
                    .storages
                    .iter()
                    .position(|(n, _)| n == name)
                    .ok_or(io::ErrorKind::NotFound)?;
                state.number(index, "/")
            }
            Some(index) => state.number(index, &below(&folder.path, name)),
        };
        Ok((number, state.node(number)?))
    }

    /// Where a new entry `name` of `parent` goes: its storage and path. A
    /// name the phone cannot hold, or the top, takes none.
    fn place(state: &State, parent: u64, name: &OsStr) -> io::Result<(usize, Node, String)> {
        let name = name.to_str().ok_or(io::ErrorKind::InvalidInput)?;
        if name.is_empty() || name.contains('/') {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        let folder = state.node(parent)?;
        let index = folder.storage.ok_or(io::ErrorKind::Unsupported)?;
        if !folder.folder {
            return Err(io::ErrorKind::NotADirectory.into());
        }
        let path = below(&folder.path, name);
        Ok((index, folder, path))
    }

    /// Send the open file `handle` to the phone, if written: what was there
    /// removed first, as MTP replaces nothing.
    fn send(&self, state: &mut State, handle: u64) -> io::Result<()> {
        let Some(opened) = state.open.get(&handle) else {
            return Err(io::Error::from_raw_os_error(9));
        };
        if !opened.written {
            return Ok(());
        }
        let number = opened.number;
        let size = opened.file.metadata()?.len();
        let node = state.node(number)?;
        let index = node.storage.ok_or(io::ErrorKind::Unsupported)?;
        if node.on_phone {
            match kmtpd::delete(&self.bus, self.storage(index), &node.path) {
                Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
                _ => {}
            }
            state.node_mut(number)?.on_phone = false;
        }
        let file = &state.open[&handle].file;
        kmtpd::send(&self.bus, self.storage(index), file, &node.path, size)?;
        let node = state.node_mut(number)?;
        node.on_phone = true;
        node.size = size;
        node.modified = SystemTime::now();
        if let Some(opened) = state.open.get_mut(&handle) {
            opened.written = false;
        }
        Ok(())
    }

    /// Keep `file` open for `number`, as written or not.
    fn opened(state: &mut State, number: u64, file: File, written: bool) -> u64 {
        let handle = state.next_handle;
        state.next_handle += 1;
        state.open.insert(
            handle,
            Opened {
                number,
                file,
                written,
            },
        );
        handle
    }
}

impl State {
    fn node(&self, number: u64) -> io::Result<Node> {
        self.nodes
            .get(&number)
            .cloned()
            .ok_or_else(|| io::ErrorKind::NotFound.into())
    }

    fn node_mut(&mut self, number: u64) -> io::Result<&mut Node> {
        self.nodes
            .get_mut(&number)
            .ok_or_else(|| io::ErrorKind::NotFound.into())
    }

    /// The number of the entry at `path` of the storage `index`: the same
    /// for as long as the mount lives, a new one for a new path.
    fn number(&mut self, index: usize, path: &str) -> u64 {
        if let Some(number) = self.numbers.get(&(index, path.to_owned())) {
            return *number;
        }
        let number = self.next;
        self.next += 1;
        self.numbers.insert((index, path.to_owned()), number);
        number
    }

    /// Forget the entry at `path` and what is below it.
    fn forget(&mut self, index: usize, path: &str) {
        let inside = format!("{path}/");
        let gone: Vec<(usize, String)> = self
            .numbers
            .keys()
            .filter(|(i, p)| *i == index && (p == path || p.starts_with(&inside)))
            .cloned()
            .collect();
        for key in gone {
            if let Some(number) = self.numbers.remove(&key) {
                self.nodes.remove(&number);
            }
        }
    }

    /// Drop `name` from the folder `parent`'s entries, or add it.
    fn entry(&mut self, parent: u64, name: &str, present: bool) {
        if let Some(entries) = self
            .nodes
            .get_mut(&parent)
            .and_then(|folder| folder.entries.as_mut())
        {
            entries.retain(|n| n != name);
            if present {
                entries.push(name.to_owned());
            }
        }
    }
}

impl Filesystem for Phone {
    fn lookup(&self, _req: &Request, parent: INodeNo, name: &OsStr, reply: ReplyEntry) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match self.find(&mut state, parent.0, name) {
            Ok((number, node)) => reply.entry(&TTL, &self.attr(number, &node), Generation(0)),
            Err(error) => reply.error(errno(&error)),
        }
    }

    fn getattr(&self, _req: &Request, ino: INodeNo, fh: Option<FileHandle>, reply: ReplyAttr) {
        let state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Ok(mut node) = state.node(ino.0) else {
            reply.error(Errno::ENOENT);
            return;
        };
        // An open file's size is its copy's, written or not.
        if let Some(opened) = fh.and_then(|fh| state.open.get(&fh.0))
            && let Ok(meta) = opened.file.metadata()
        {
            node.size = meta.len();
        }
        if node.storage.is_none() {
            node.modified = self.since;
        }
        reply.attr(&TTL, &self.attr(ino.0, &node));
    }

    fn setattr(
        &self,
        _req: &Request,
        ino: INodeNo,
        mode: Option<u32>,
        uid: Option<u32>,
        gid: Option<u32>,
        size: Option<u64>,
        atime: Option<TimeOrNow>,
        mtime: Option<TimeOrNow>,
        _ctime: Option<SystemTime>,
        fh: Option<FileHandle>,
        _crtime: Option<SystemTime>,
        _chgtime: Option<SystemTime>,
        _bkuptime: Option<SystemTime>,
        _flags: Option<fuser::BsdFileFlags>,
        reply: ReplyAttr,
    ) {
        // The phone keeps no mode or owner, and stamps what it is sent.
        if mode.is_some() || uid.is_some() || gid.is_some() || atime.is_some() || mtime.is_some() {
            reply.error(Errno::ENOTSUP);
            return;
        }
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Ok(mut node) = state.node(ino.0) else {
            reply.error(Errno::ENOENT);
            return;
        };
        if let Some(size) = size {
            if node.folder {
                reply.error(Errno::EISDIR);
                return;
            }
            let result = match fh.map(|fh| fh.0).filter(|fh| state.open.contains_key(fh)) {
                // Open: its copy cut or grown, sent when it is closed.
                Some(handle) => {
                    let opened = state.open.get_mut(&handle).expect("checked");
                    opened.written = true;
                    opened.file.set_len(size)
                }
                None if size == node.size => Ok(()),
                // Emptied unopened (`truncate`): an empty file sent now.
                None if size == 0 => self.scratch().and_then(|file| {
                    let handle = Self::opened(&mut state, ino.0, file, true);
                    let sent = self.send(&mut state, handle);
                    state.open.remove(&handle);
                    sent
                }),
                None => Err(io::ErrorKind::Unsupported.into()),
            };
            if let Err(error) = result {
                reply.error(errno(&error));
                return;
            }
            node.size = size;
        }
        reply.attr(&TTL, &self.attr(ino.0, &node));
    }

    fn mkdir(
        &self,
        _req: &Request,
        parent: INodeNo,
        name: &OsStr,
        _mode: u32,
        _umask: u32,
        reply: ReplyEntry,
    ) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let made = Self::place(&state, parent.0, name).and_then(|(index, _, path)| {
            if self.find(&mut state, parent.0, name).is_ok() {
                return Err(io::ErrorKind::AlreadyExists.into());
            }
            kmtpd::make_folder(&self.bus, self.storage(index), &path)?;
            let number = state.number(index, &path);
            let node = Node {
                storage: Some(index),
                path,
                folder: true,
                size: 0,
                modified: SystemTime::now(),
                on_phone: true,
                entries: Some(Vec::new()),
            };
            state.nodes.insert(number, node.clone());
            state.entry(parent.0, &name.to_string_lossy(), true);
            Ok((number, node))
        });
        match made {
            Ok((number, node)) => reply.entry(&TTL, &self.attr(number, &node), Generation(0)),
            Err(error) => reply.error(errno(&error)),
        }
    }

    fn unlink(&self, _req: &Request, parent: INodeNo, name: &OsStr, reply: ReplyEmpty) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let removed = self.find(&mut state, parent.0, name).and_then(|(_, node)| {
            if node.folder {
                return Err(io::ErrorKind::IsADirectory.into());
            }
            let index = node.storage.ok_or(io::ErrorKind::Unsupported)?;
            if node.on_phone {
                kmtpd::delete(&self.bus, self.storage(index), &node.path)?;
            }
            state.forget(index, &node.path);
            state.entry(parent.0, &name.to_string_lossy(), false);
            Ok(())
        });
        match removed {
            Ok(()) => reply.ok(),
            Err(error) => reply.error(errno(&error)),
        }
    }

    fn rmdir(&self, _req: &Request, parent: INodeNo, name: &OsStr, reply: ReplyEmpty) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let removed = self
            .find(&mut state, parent.0, name)
            .and_then(|(number, node)| {
                if !node.folder {
                    return Err(io::ErrorKind::NotADirectory.into());
                }
                let index = node.storage.ok_or(io::ErrorKind::Unsupported)?;
                if node.path == "/" {
                    return Err(io::ErrorKind::Unsupported.into());
                }
                // The phone removes a folder with what it holds: only an empty
                // one is removed here, as the system's call asks.
                if !self.list(&mut state, number)?.is_empty() {
                    return Err(io::ErrorKind::DirectoryNotEmpty.into());
                }
                kmtpd::delete(&self.bus, self.storage(index), &node.path)?;
                state.forget(index, &node.path);
                state.entry(parent.0, &name.to_string_lossy(), false);
                Ok(())
            });
        match removed {
            Ok(()) => reply.ok(),
            Err(error) => reply.error(errno(&error)),
        }
    }

    fn rename(
        &self,
        _req: &Request,
        parent: INodeNo,
        name: &OsStr,
        newparent: INodeNo,
        newname: &OsStr,
        flags: RenameFlags,
        reply: ReplyEmpty,
    ) {
        // The phone renames within a folder alone: copies are made where
        // they go (ADR-083).
        if parent != newparent {
            reply.error(Errno::EXDEV);
            return;
        }
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let renamed = (|| {
            let (_, node) = self.find(&mut state, parent.0, name)?;
            let (index, _, path) = Self::place(&state, newparent.0, newname)?;
            if path == node.path {
                return Ok(());
            }
            let renamed_to = newname.to_string_lossy().into_owned();
            if let Ok((target_number, target)) = self.find(&mut state, parent.0, newname) {
                if flags.contains(RenameFlags::RENAME_NOREPLACE) {
                    return Err(io::ErrorKind::AlreadyExists.into());
                }
                if target.folder != node.folder {
                    return Err(if target.folder {
                        io::ErrorKind::IsADirectory
                    } else {
                        io::ErrorKind::NotADirectory
                    }
                    .into());
                }
                if target.folder && !self.list(&mut state, target_number)?.is_empty() {
                    return Err(io::ErrorKind::DirectoryNotEmpty.into());
                }
                // MTP replaces nothing: what is replaced goes first.
                kmtpd::delete(&self.bus, self.storage(index), &target.path)?;
                state.forget(index, &target.path);
                state.entry(parent.0, &renamed_to, false);
            } else if !flags.is_empty() && !flags.contains(RenameFlags::RENAME_NOREPLACE) {
                return Err(io::ErrorKind::InvalidInput.into());
            }
            if node.on_phone {
                kmtpd::rename(&self.bus, self.storage(index), &node.path, &renamed_to)?;
            }
            // Its number follows it; what is below it is listed anew.
            let number = state
                .numbers
                .remove(&(index, node.path.clone()))
                .ok_or(io::ErrorKind::NotFound)?;
            state.forget(index, &node.path);
            state.numbers.insert((index, path.clone()), number);
            let mut moved = node.clone();
            moved.path = path;
            if moved.folder {
                moved.entries = None;
            }
            state.nodes.insert(number, moved);
            state.entry(parent.0, &name.to_string_lossy(), false);
            state.entry(parent.0, &renamed_to, true);
            Ok(())
        })();
        match renamed {
            Ok(()) => reply.ok(),
            Err(error) => reply.error(errno(&error)),
        }
    }

    fn open(&self, _req: &Request, ino: INodeNo, flags: OpenFlags, reply: ReplyOpen) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let opened = (|| {
            let node = state.node(ino.0)?;
            if node.folder {
                return Err(io::ErrorKind::IsADirectory.into());
            }
            let index = node.storage.ok_or(io::ErrorKind::Unsupported)?;
            let writes = flags.acc_mode() != OpenAccMode::O_RDONLY;
            let emptied = writes && flags.0 & rustix::fs::OFlags::TRUNC.bits().cast_signed() != 0;
            let file = self.scratch()?;
            if !emptied && node.on_phone {
                kmtpd::fetch(&self.bus, self.storage(index), &node.path, &file, node.size)?;
            }
            Ok(Self::opened(&mut state, ino.0, file, emptied))
        })();
        match opened {
            Ok(handle) => reply.opened(FileHandle(handle), FopenFlags::empty()),
            Err(error) => reply.error(errno(&error)),
        }
    }

    fn create(
        &self,
        _req: &Request,
        parent: INodeNo,
        name: &OsStr,
        _mode: u32,
        _umask: u32,
        _flags: i32,
        reply: ReplyCreate,
    ) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let made = (|| {
            let (index, _, path) = Self::place(&state, parent.0, name)?;
            // What is there already is replaced when this is sent.
            let on_phone = self
                .find(&mut state, parent.0, name)
                .is_ok_and(|(_, node)| node.on_phone);
            let number = state.number(index, &path);
            let node = Node {
                storage: Some(index),
                path,
                folder: false,
                size: 0,
                modified: SystemTime::now(),
                on_phone,
                entries: None,
            };
            state.nodes.insert(number, node.clone());
            state.entry(parent.0, &name.to_string_lossy(), true);
            let file = self.scratch()?;
            let handle = Self::opened(&mut state, number, file, true);
            Ok((number, node, handle))
        })();
        match made {
            Ok((number, node, handle)) => reply.created(
                &TTL,
                &self.attr(number, &node),
                Generation(0),
                FileHandle(handle),
                FopenFlags::empty(),
            ),
            Err(error) => reply.error(errno(&error)),
        }
    }

    fn read(
        &self,
        _req: &Request,
        _ino: INodeNo,
        fh: FileHandle,
        offset: u64,
        size: u32,
        _flags: OpenFlags,
        _lock_owner: Option<LockOwner>,
        reply: ReplyData,
    ) {
        let state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(opened) = state.open.get(&fh.0) else {
            reply.error(Errno::EBADF);
            return;
        };
        let mut buffer = vec![0; size as usize];
        let mut filled = 0;
        while filled < buffer.len() {
            match opened
                .file
                .read_at(&mut buffer[filled..], offset + filled as u64)
            {
                Ok(0) => break,
                Ok(n) => filled += n,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => {
                    reply.error(errno(&error));
                    return;
                }
            }
        }
        reply.data(&buffer[..filled]);
    }

    fn write(
        &self,
        _req: &Request,
        _ino: INodeNo,
        fh: FileHandle,
        offset: u64,
        data: &[u8],
        _write_flags: WriteFlags,
        _flags: OpenFlags,
        _lock_owner: Option<LockOwner>,
        reply: ReplyWrite,
    ) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(opened) = state.open.get_mut(&fh.0) else {
            reply.error(Errno::EBADF);
            return;
        };
        match opened.file.write_all_at(data, offset) {
            Ok(()) => {
                opened.written = true;
                reply.written(u32::try_from(data.len()).unwrap_or(u32::MAX));
            }
            Err(error) => reply.error(errno(&error)),
        }
    }

    /// Closed: what was written is sent, and the close says how it went.
    fn flush(
        &self,
        _req: &Request,
        _ino: INodeNo,
        fh: FileHandle,
        _lock_owner: LockOwner,
        reply: ReplyEmpty,
    ) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match self.send(&mut state, fh.0) {
            Ok(()) => reply.ok(),
            Err(error) => reply.error(errno(&error)),
        }
    }

    fn fsync(
        &self,
        _req: &Request,
        _ino: INodeNo,
        fh: FileHandle,
        _datasync: bool,
        reply: ReplyEmpty,
    ) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match self.send(&mut state, fh.0) {
            Ok(()) => reply.ok(),
            Err(error) => reply.error(errno(&error)),
        }
    }

    fn release(
        &self,
        _req: &Request,
        _ino: INodeNo,
        fh: FileHandle,
        _flags: OpenFlags,
        _lock_owner: Option<LockOwner>,
        _flush: bool,
        reply: ReplyEmpty,
    ) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // Sent at its flush already, unless that failed: tried once more.
        let sent = self.send(&mut state, fh.0);
        state.open.remove(&fh.0);
        match sent {
            Ok(()) => reply.ok(),
            Err(error) => reply.error(errno(&error)),
        }
    }

    fn readdir(
        &self,
        _req: &Request,
        ino: INodeNo,
        _fh: FileHandle,
        offset: u64,
        mut reply: ReplyDirectory,
    ) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // Listed anew each time the folder is read from its start.
        let names = match state.node(ino.0) {
            Ok(node) if offset == 0 || node.entries.is_none() => self.list(&mut state, ino.0),
            Ok(node) => Ok(node.entries.unwrap_or_default()),
            Err(error) => Err(error),
        };
        let names = match names {
            Ok(names) => names,
            Err(error) => {
                reply.error(errno(&error));
                return;
            }
        };
        let Ok(folder) = state.node(ino.0) else {
            reply.error(Errno::ENOENT);
            return;
        };
        let mut entries: Vec<(u64, FileType, String)> = vec![
            (ino.0, FileType::Directory, ".".to_owned()),
            (TOP, FileType::Directory, "..".to_owned()),
        ];
        for name in names {
            let number = match folder.storage {
                None => self
                    .storages
                    .iter()
                    .position(|(n, _)| *n == name)
                    .map(|index| state.number(index, "/")),
                Some(index) => Some(state.number(index, &below(&folder.path, &name))),
            };
            let Some(number) = number else { continue };
            let kind = match state.nodes.get(&number) {
                Some(node) if !node.folder => FileType::RegularFile,
                _ => FileType::Directory,
            };
            entries.push((number, kind, name));
        }
        for (at, (number, kind, name)) in entries.iter().enumerate().skip(offset as usize) {
            if reply.add(INodeNo(*number), at as u64 + 1, *kind, name) {
                break;
            }
        }
        reply.ok();
    }

    fn statfs(&self, _req: &Request, ino: INodeNo, reply: ReplyStatfs) {
        let state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let storage = state.nodes.get(&ino.0).and_then(|node| node.storage);
        drop(state);
        let mut whole = 0u64;
        let mut free = 0u64;
        for (index, (_, kept)) in self.storages.iter().enumerate() {
            if storage.is_some_and(|s| s != index) {
                continue;
            }
            match kmtpd::space(&self.bus, kept) {
                Ok((w, f)) => {
                    whole += w;
                    free += f;
                }
                Err(error) => {
                    reply.error(errno(&error));
                    return;
                }
            }
        }
        let block = 4096u64;
        reply.statfs(
            whole / block,
            free / block,
            free / block,
            0,
            0,
            block as u32,
            255,
            block as u32,
        );
    }
}

/// Serve `session` until it is let go of, or the device leaves kmtpd, or
/// `kiod6` the bus (then let go of here).
///
/// # Errors
/// What the session met.
pub fn serve(mut session: fuser::Session<Phone>, udi: String, point: PathBuf) -> io::Result<()> {
    let mut unmounter = session.unmount_callable();
    std::thread::Builder::new()
        .name("kmtpd-watch".into())
        .spawn(move || {
            watch(&udi);
            // What it mounted itself as root, it lets go of itself.
            if rustix::process::geteuid().is_root() {
                let _ = rustix::mount::unmount(&point, rustix::mount::UnmountFlags::DETACH);
            } else {
                let _ = unmounter.unmount();
            }
        })?;
    session.run()
}

/// Mount `phone` at `point` as root (in a user namespace of its own, as
/// tests run it): fuser mounts as root with the type `fuse` alone, its
/// subtype only the mount's source, where the system's program (and
/// libfuse) name it `fuse.skwir-kmtp`, by which Skwir knows it
/// (`fs::reach`). Mounted so here, FUSE's device then served by fuser.
fn mount_as_root(
    phone: Phone,
    point: &Path,
    config: fuser::Config,
) -> io::Result<fuser::Session<Phone>> {
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::PermissionsExt;
    let device = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/fuse")?;
    let data = format!(
        "fd={},rootmode={:o},user_id={},group_id={},default_permissions",
        device.as_raw_fd(),
        std::fs::metadata(point)?.permissions().mode(),
        rustix::process::getuid().as_raw(),
        rustix::process::getgid().as_raw(),
    );
    let data = std::ffi::CString::new(data).map_err(|_| io::ErrorKind::InvalidInput)?;
    rustix::mount::mount(
        "kmtpd",
        point,
        format!("fuse.{SUBTYPE}").as_str(),
        rustix::mount::MountFlags::NODEV
            | rustix::mount::MountFlags::NOSUID
            | rustix::mount::MountFlags::NOEXEC
            | rustix::mount::MountFlags::NOATIME,
        data.as_c_str(),
    )?;
    fuser::Session::from_fd(
        phone,
        std::os::fd::OwnedFd::from(device),
        fuser::SessionACL::Owner,
        config,
    )
}

/// Return once the device `udi` is gone from kmtpd, or kmtpd from the bus.
fn watch(udi: &str) {
    let Ok(bus) = kmtpd::connect() else { return };
    let Ok(dbus) = zbus::blocking::fdo::DBusProxy::new(&bus) else {
        return;
    };
    let rules = [
        zbus::MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .sender("org.freedesktop.DBus")
            .and_then(|rule| rule.member("NameOwnerChanged"))
            .and_then(|rule| rule.arg(0, kmtpd::SERVICE))
            .map(zbus::match_rule::Builder::build),
        zbus::MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .interface("org.kde.kmtp.Daemon")
            .and_then(|rule| rule.member("devicesChanged"))
            .map(zbus::match_rule::Builder::build),
    ];
    for rule in rules {
        let Ok(rule) = rule else { return };
        if dbus.add_match_rule(rule).is_err() {
            return;
        }
    }
    // Gone already, before the rules listened.
    if !matches!(kmtpd::device(&bus, udi), Ok(Some(_))) {
        return;
    }
    for message in zbus::blocking::MessageIterator::from(&bus) {
        let Ok(message) = message else { return };
        let header = message.header();
        if header.message_type() != zbus::message::Type::Signal {
            continue;
        }
        match header.member().map(|m| m.as_str().to_owned()).as_deref() {
            Some("NameOwnerChanged") => {
                if message
                    .body()
                    .deserialize::<(String, String, String)>()
                    .is_ok_and(|(name, _, owner)| name == kmtpd::SERVICE && owner.is_empty())
                {
                    return;
                }
            }
            Some("devicesChanged") if !matches!(kmtpd::device(&bus, udi), Ok(Some(_))) => {
                return;
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storages_have_distinct_names() {
        let names = distinct(&[
            "Internal storage".to_owned(),
            "Internal storage".to_owned(),
            String::new(),
            "a/b".to_owned(),
        ]);
        assert_eq!(
            names,
            [
                "Internal storage",
                "Internal storage (2)",
                "Storage",
                "Storage (2)"
            ]
        );
    }

    #[test]
    fn a_path_below_a_storage_s_top() {
        assert_eq!(below("/", "DCIM"), "/DCIM");
        assert_eq!(below("/DCIM", "It's 1.jpg"), "/DCIM/It's 1.jpg");
    }

    #[test]
    fn what_is_forgotten_is_what_is_below() {
        let mut state = State {
            next: 2,
            ..State::default()
        };
        let a = state.number(0, "/a");
        let ab = state.number(0, "/a/b");
        let ac = state.number(0, "/ac");
        let other = state.number(1, "/a");
        for n in [a, ab, ac, other] {
            state.nodes.insert(
                n,
                Node {
                    storage: Some(0),
                    path: String::new(),
                    folder: false,
                    size: 0,
                    modified: UNIX_EPOCH,
                    on_phone: true,
                    entries: None,
                },
            );
        }
        state.forget(0, "/a");
        assert!(!state.nodes.contains_key(&a) && !state.nodes.contains_key(&ab));
        assert!(state.nodes.contains_key(&ac), "a name that starts the same");
        assert!(state.nodes.contains_key(&other), "another storage's");
        assert_ne!(state.number(0, "/a"), a, "a new number for a new entry");
    }
}
