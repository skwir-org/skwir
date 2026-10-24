//! Optional libarchive boundary. A member's name reaches the filesystem
//! only when a whole archive is extracted (`--extract`, ADR-048), through
//! libarchive's own extraction and its guards, in the folder it runs in.
//! An archive is made (`--create`, ADR-059) of what skwir sends on
//! standard input: this helper opens none of the user's files.
// SAFETY: all native handles stay on this thread; strings are copied while the
// current entry is alive, buffers carry their exact lengths, and Reader owns
// the single archive_read_free call. Native code is isolated from the UI.
#[allow(unsafe_code)]
mod native {
    use std::{
        cell::{Cell, RefCell},
        ffi::{CStr, CString, c_char, c_int, c_long, c_uint, c_void},
        io::{BufRead, Read, Write},
    };
    /// libarchive's `mode_t` for a permission: `unsigned short` on
    /// Windows, the system's `mode_t` elsewhere (16 bits on a Mac).
    #[cfg(any(windows, target_vendor = "apple"))]
    type Mode = u16;
    #[cfg(not(any(windows, target_vendor = "apple")))]
    type Mode = u32;
    /// What libarchive calls to learn a passphrase.
    type PassphraseCallback = unsafe extern "C" fn(*mut c_void, *mut c_void) -> *const c_char;
    // libarchive, linked by the build script: the system's, or the one a
    // package carries, static.
    unsafe extern "C" {
        fn archive_read_new() -> *mut c_void;
        fn archive_read_support_filter_all(a: *mut c_void) -> c_int;
        fn archive_read_support_format_all(a: *mut c_void) -> c_int;
        fn archive_read_support_format_raw(a: *mut c_void) -> c_int;
        fn archive_format(a: *mut c_void) -> c_int;
        fn archive_filter_count(a: *mut c_void) -> c_int;
        #[cfg(not(windows))]
        fn archive_read_open_filename(a: *mut c_void, path: *const c_char, block: usize) -> c_int;
        #[cfg(windows)]
        fn archive_read_open_filename_w(a: *mut c_void, path: *const u16, block: usize) -> c_int;
        fn archive_read_set_options(a: *mut c_void, options: *const c_char) -> c_int;
        fn archive_read_next_header(a: *mut c_void, entry: *mut *mut c_void) -> c_int;
        fn archive_entry_pathname_utf8(e: *mut c_void) -> *const c_char;
        fn archive_entry_pathname(e: *mut c_void) -> *const c_char;
        fn archive_entry_size(e: *mut c_void) -> i64;
        fn archive_entry_size_is_set(e: *mut c_void) -> c_int;
        fn archive_entry_filetype(e: *mut c_void) -> u32;
        fn archive_entry_is_encrypted(e: *mut c_void) -> c_int;
        fn archive_read_data(a: *mut c_void, buf: *mut c_void, size: usize) -> isize;
        fn archive_read_data_block(
            a: *mut c_void,
            buf: *mut *const c_void,
            size: *mut usize,
            offset: *mut i64,
        ) -> c_int;
        fn archive_error_string(a: *mut c_void) -> *const c_char;
        fn archive_errno(a: *mut c_void) -> c_int;
        fn archive_read_free(a: *mut c_void) -> c_int;
        fn archive_entry_mtime(e: *mut c_void) -> i64;
        fn archive_entry_mtime_nsec(e: *mut c_void) -> c_long;
        fn archive_entry_mtime_is_set(e: *mut c_void) -> c_int;
        fn archive_entry_symlink(e: *mut c_void) -> *const c_char;
        fn archive_entry_symlink_utf8(e: *mut c_void) -> *const c_char;
        fn archive_entry_hardlink_utf8(e: *mut c_void) -> *const c_char;
        fn archive_filter_bytes(a: *mut c_void, n: c_int) -> i64;
        fn archive_entry_hardlink(e: *mut c_void) -> *const c_char;
        fn archive_entry_set_pathname_utf8(e: *mut c_void, name: *const c_char);
        fn archive_write_disk_new() -> *mut c_void;
        fn archive_write_disk_set_options(a: *mut c_void, flags: c_int) -> c_int;
        fn archive_write_header(a: *mut c_void, e: *mut c_void) -> c_int;
        fn archive_write_data_block(
            a: *mut c_void,
            buf: *const c_void,
            size: usize,
            offset: i64,
        ) -> isize;
        fn archive_write_finish_entry(a: *mut c_void) -> c_int;
        fn archive_write_close(a: *mut c_void) -> c_int;
        fn archive_write_free(a: *mut c_void) -> c_int;
        fn archive_read_set_passphrase_callback(
            a: *mut c_void,
            client: *mut c_void,
            callback: Option<PassphraseCallback>,
        ) -> c_int;
        fn archive_entry_xattr_reset(e: *mut c_void) -> c_int;
        fn archive_entry_xattr_next(
            e: *mut c_void,
            name: *mut *const c_char,
            value: *mut *const c_void,
            size: *mut usize,
        ) -> c_int;
        fn archive_entry_xattr_clear(e: *mut c_void);
        fn archive_entry_xattr_add_entry(
            e: *mut c_void,
            name: *const c_char,
            value: *const c_void,
            size: usize,
        );
        fn archive_write_new() -> *mut c_void;
        fn archive_write_set_format_zip(a: *mut c_void) -> c_int;
        fn archive_write_set_format_7zip(a: *mut c_void) -> c_int;
        fn archive_write_set_format_pax_restricted(a: *mut c_void) -> c_int;
        fn archive_write_add_filter_gzip(a: *mut c_void) -> c_int;
        fn archive_write_add_filter_bzip2(a: *mut c_void) -> c_int;
        fn archive_write_add_filter_xz(a: *mut c_void) -> c_int;
        fn archive_write_add_filter_zstd(a: *mut c_void) -> c_int;
        fn archive_write_set_options(a: *mut c_void, options: *const c_char) -> c_int;
        fn archive_write_set_passphrase(a: *mut c_void, passphrase: *const c_char) -> c_int;
        #[cfg(not(windows))]
        fn archive_write_open_filename(a: *mut c_void, path: *const c_char) -> c_int;
        #[cfg(windows)]
        fn archive_write_open_filename_w(a: *mut c_void, path: *const u16) -> c_int;
        fn archive_write_data(a: *mut c_void, buf: *const c_void, size: usize) -> isize;
        fn archive_entry_new() -> *mut c_void;
        fn archive_entry_free(e: *mut c_void);
        fn archive_entry_set_pathname(e: *mut c_void, name: *const c_char);
        fn archive_entry_set_filetype(e: *mut c_void, kind: c_uint);
        fn archive_entry_set_perm(e: *mut c_void, mode: Mode);
        fn archive_entry_set_size(e: *mut c_void, size: i64);
        fn archive_entry_set_mtime(e: *mut c_void, seconds: i64, nanoseconds: c_long);
        fn archive_entry_set_symlink(e: *mut c_void, target: *const c_char);
        fn archive_entry_set_symlink_utf8(e: *mut c_void, target: *const c_char);
        fn archive_entry_set_hardlink(e: *mut c_void, target: *const c_char);
        fn archive_entry_set_hardlink_utf8(e: *mut c_void, target: *const c_char);
    }
    // From the C library std already links.
    unsafe extern "C" {
        fn setlocale(category: c_int, locale: *const c_char) -> *mut c_char;
    }
    /// `LC_CTYPE`: 0 in glibc and musl, 2 in Apple's, the BSDs' and
    /// Windows' C libraries (where 0 is `LC_ALL`).
    #[cfg(any(
        windows,
        target_vendor = "apple",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd",
        target_os = "dragonfly"
    ))]
    const LC_CTYPE: c_int = 2;
    #[cfg(not(any(
        windows,
        target_vendor = "apple",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd",
        target_os = "dragonfly"
    )))]
    const LC_CTYPE: c_int = 0;
    /// UTF-8, as each C library names it: glibc's and musl's, then Apple's
    /// and the BSDs'; Windows' (its UCRT, from Windows 10's 1803) keeps the
    /// user's language, whose OEM code page libarchive reads a name in when
    /// an archive does not say its charset (`READ_OPTIONS`).
    #[cfg(not(windows))]
    const UTF8_LOCALES: [&CStr; 3] = [c"C.UTF-8", c"C.utf8", c"UTF-8"];
    #[cfg(windows)]
    const UTF8_LOCALES: [&CStr; 1] = [c".UTF-8"];
    /// What an archive is read with. An mtree file is a list of files on
    /// disk: libarchive's reader opens and returns the files it names
    /// (`contents=~/.ssh/id_rsa`) once asked to check them (`checkfs`), as
    /// it is not by default. Not left to a default: an archive is read for
    /// what it holds, never what it names.
    #[cfg(not(windows))]
    const READ_OPTIONS: &CStr = c"mtree:!checkfs";
    /// And what libarchive's defaults get wrong on Windows. A name an
    /// archive does not say the charset of is read in the OEM code page
    /// there, as Windows' own archivers write a ZIP's or a RAR's; but a tar
    /// or a cpio archive comes from Unix's tools, whose names are UTF-8
    /// (GNU tar's, without a PAX header saying so), as they are read
    /// elsewhere.
    #[cfg(windows)]
    const READ_OPTIONS: &CStr = c"mtree:!checkfs,tar:hdrcharset=UTF-8,cpio:hdrcharset=UTF-8";
    /// And a ZIP made on Windows would name its members in the OEM code
    /// page, unflagged, where a name outside it is lost: in UTF-8, flagged
    /// so, as made elsewhere.
    #[cfg(windows)]
    const ZIP_OPTIONS: &CStr = c"zip:hdrcharset=UTF-8";

    const ARCHIVE_OK: c_int = 0;
    const ARCHIVE_EOF: c_int = 1;
    /// `ARCHIVE_FORMAT_RAW`: no archive at all, one stream of data.
    const ARCHIVE_FORMAT_RAW: c_int = 0x90000;
    /// A warning is not a failure where it reports a name or a header:
    /// libarchive reports one when, for instance, a name cannot be
    /// converted to the locale's charset, and the entry is still there to
    /// read. Only `ARCHIVE_FAILED` and `ARCHIVE_FATAL` stop those ([`check`]).
    /// Where it reports a member's bytes (a ZIP's CRC or AES authentication
    /// that does not match, a size that is not the one declared, more data
    /// than the header said), it says what was read is not what was stored:
    /// that stops everything ([`data`]).
    const ARCHIVE_WARN: c_int = -20;

    /// How a whole archive is extracted (`archive.h`): modification times
    /// kept; never a member written outside the folder, by `..`, by an
    /// absolute path, or through a link the archive made before it. No
    /// owner, and no set-id or sticky bits: not asked for.
    const EXTRACT_TIME: c_int = 0x0004;
    /// Extended attributes put back, those a copy keeps (`kept`): a
    /// file's tags come back with it.
    const EXTRACT_XATTR: c_int = 0x0080;
    const EXTRACT_SECURE_SYMLINKS: c_int = 0x0100;
    const EXTRACT_SECURE_NODOTDOT: c_int = 0x0200;
    const EXTRACT_SECURE_NOABSOLUTEPATHS: c_int = 0x1_0000;
    const AE_IFREG: u32 = 0o100_000;
    const AE_IFDIR: u32 = 0o040_000;
    const AE_IFLNK: u32 = 0o120_000;
    /// A unit of work, as the caller hands them out (one byte of standard
    /// input each): an entry, or this many bytes of one.
    const UNIT_BYTES: usize = 1 << 20;
    /// Units between two reports: the caller hands out more as they come.
    const REPORT_EVERY: u64 = 8;
    /// The byte a ticket is.
    const TICKET: u8 = 1;

    /// libarchive converts member names through the C library's charset. A
    /// process starts in the "C" locale, where any non-ASCII name fails to
    /// convert, so ask for UTF-8 first and the environment's locale after.
    fn utf8_locale() {
        for locale in UTF8_LOCALES.into_iter().chain([c""]) {
            if !unsafe { setlocale(LC_CTYPE, locale.as_ptr()) }.is_null() {
                return;
            }
        }
    }

    /// A path as libarchive opens it: its bytes; on Windows, UTF-16 for
    /// its wide calls, which take any path (a narrow one is read in the
    /// locale's code page).
    #[cfg(not(windows))]
    type NativePath = CString;
    #[cfg(windows)]
    type NativePath = Vec<u16>;
    #[cfg(not(windows))]
    use self::{
        archive_read_open_filename as open_read, archive_write_open_filename as open_write,
    };
    #[cfg(windows)]
    use self::{
        archive_read_open_filename_w as open_read, archive_write_open_filename_w as open_write,
    };
    #[cfg(not(windows))]
    fn native_path(path: &std::ffi::OsStr) -> anyhow::Result<NativePath> {
        Ok(CString::new(path.as_encoded_bytes())?)
    }
    #[cfg(windows)]
    fn native_path(path: &std::ffi::OsStr) -> anyhow::Result<NativePath> {
        use std::os::windows::ffi::OsStrExt;
        let wide: Vec<u16> = path.encode_wide().collect();
        anyhow::ensure!(!wide.contains(&0), "a path with a NUL");
        Ok(wide.into_iter().chain([0]).collect())
    }

    struct Reader(*mut c_void);
    impl Drop for Reader {
        fn drop(&mut self) {
            unsafe {
                archive_read_free(self.0);
            }
        }
    }
    impl Reader {
        fn check(&self, status: c_int) -> anyhow::Result<()> {
            check(self.0, status)
        }
    }
    /// A libarchive writer: the disk's, what `--extract` writes with, or
    /// an archive's, what `--create` writes.
    struct Writer(*mut c_void);
    impl Drop for Writer {
        fn drop(&mut self) {
            unsafe {
                archive_write_free(self.0);
            }
        }
    }
    /// `status` of a call on `a`, as an error with libarchive's words when
    /// it failed; a warning is not a failure.
    fn check(a: *mut c_void, status: c_int) -> anyhow::Result<()> {
        if status < ARCHIVE_WARN {
            anyhow::bail!("{}", said(a));
        }
        Ok(())
    }
    /// What libarchive said of the last call on `a`.
    fn said(a: *mut c_void) -> String {
        let p = unsafe { archive_error_string(a) };
        if p.is_null() {
            "Archive error".into()
        } else {
            unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
        }
    }
    /// `status` of a call reading or writing a member's bytes: a warning
    /// stops it as a failure does (`ARCHIVE_WARN`). Encrypted, what does
    /// not match may be a wrong password as well as a damaged archive.
    fn data(a: *mut c_void, status: c_int, name: &str, encrypted: bool) -> anyhow::Result<()> {
        if status >= ARCHIVE_OK {
            return Ok(());
        }
        let error = said(a);
        if encrypted {
            anyhow::bail!("“{name}”: wrong password or damaged archive ({error})");
        }
        anyhow::bail!("“{name}” is damaged: {error}")
    }
    /// Whether the warning of the last call on `a` (a member finished:
    /// its times, its mode, its attributes put back) is the drive refusing
    /// what it does not keep, which a copy does not keep either: a volume
    /// without extended attributes, a link that takes none, a drive that
    /// takes no times. Anything else stops the extraction.
    fn refused_by_the_drive(a: *mut c_void) -> bool {
        let errno = unsafe { archive_errno(a) };
        if errno <= 0 {
            return false;
        }
        #[cfg(unix)]
        {
            use std::io::ErrorKind;
            matches!(
                std::io::Error::from_raw_os_error(errno).kind(),
                ErrorKind::Unsupported | ErrorKind::PermissionDenied
            )
        }
        // libarchive reports the C runtime's numbers there, not the
        // system's: EPERM, EACCES, ENOSYS, ENOTSUP, EOPNOTSUPP.
        #[cfg(not(unix))]
        {
            matches!(errno, 1 | 13 | 40 | 129 | 130)
        }
    }
    /// `skwir-archive-worker <archive> --members N --names N` lists members
    /// (each with its kind, size, time, a link's target and a hard link's
    /// first name);
    /// `skwir-archive-worker <archive> <index> <output> --name NAME --bytes N`
    /// streams the member at `index`, provided it is still called `NAME`, so
    /// an archive rewritten since its listing never yields another member;
    /// `skwir-archive-worker <archive> --extract` extracts it all into the
    /// folder it runs in, taking a byte of standard input before each unit
    /// of work (an entry, or `UNIT_BYTES` of one: none, and it waits) and
    /// reporting `{"read": bytes of the archive read, "units": taken}`
    /// lines on standard output. With `--passwords`, a passphrase an
    /// encrypted member needs is asked of the caller in a
    /// `{"passphrase": attempt}` line and read from standard input: its
    /// bytes in hexadecimal on a line of their own, none to give up.
    /// `skwir-archive-worker <output> --create FORMAT [--encrypt]` writes
    /// an archive at `output` (`zip`, `7z`, `tar`, `tar.gz`, `tar.bz2`,
    /// `tar.xz`, `tar.zst`) of what standard input says ([`create`]).
    /// Every bound arrives from the caller so the UI process and this helper
    /// can never disagree on a limit. `--stem STEM` is the file's own name
    /// less its last extension, which a file only compressed names its one
    /// member after: in a sandbox the archive is read under another name.
    pub struct Args {
        pub path: std::ffi::OsString,
        pub stem: Option<std::ffi::OsString>,
        pub member: Option<u64>,
        pub output: Option<std::ffi::OsString>,
        pub name: Option<std::ffi::OsString>,
        pub bytes: Option<usize>,
        pub members: Option<usize>,
        pub names: Option<usize>,
        /// How long a listing may take: past it, what was listed is told,
        /// as past the other bounds.
        pub seconds: Option<usize>,
        pub extract: bool,
        /// With `--extract`: ask the caller for a passphrase.
        pub passwords: bool,
        /// The format of an archive to make.
        pub create: Option<String>,
        /// With `--create zip`: encrypted, by a passphrase read first.
        pub encrypt: bool,
    }
    pub fn parse_args(args: impl IntoIterator<Item = std::ffi::OsString>) -> anyhow::Result<Args> {
        let mut positional = Vec::new();
        let mut extract = false;
        let mut passwords = false;
        let mut create = None;
        let mut encrypt = false;
        let mut name = None;
        let mut stem = None;
        let mut bytes = None;
        let mut members = None;
        let mut names = None;
        let mut seconds = None;
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            let slot = match arg.to_str() {
                Some("--bytes") => &mut bytes,
                Some("--members") => &mut members,
                Some("--names") => &mut names,
                Some("--seconds") => &mut seconds,
                Some("--name") => {
                    name = Some(
                        args.next()
                            .ok_or_else(|| anyhow::anyhow!("missing value for --name"))?,
                    );
                    continue;
                }
                Some("--stem") => {
                    stem = Some(
                        args.next()
                            .ok_or_else(|| anyhow::anyhow!("missing value for --stem"))?,
                    );
                    continue;
                }
                Some("--extract") => {
                    extract = true;
                    continue;
                }
                Some("--passwords") => {
                    passwords = true;
                    continue;
                }
                Some("--encrypt") => {
                    encrypt = true;
                    continue;
                }
                Some("--create") => {
                    create = Some(
                        args.next()
                            .ok_or_else(|| anyhow::anyhow!("missing value for --create"))?
                            .to_string_lossy()
                            .into_owned(),
                    );
                    continue;
                }
                // One it does not know is refused, never taken for the
                // archive or a member: a skwir newer than this helper
                // asking what it cannot do says so.
                Some(option) if option.starts_with("--") => {
                    anyhow::bail!("unknown option {option}");
                }
                _ => {
                    positional.push(arg);
                    continue;
                }
            };
            let value = args
                .next()
                .ok_or_else(|| anyhow::anyhow!("missing value for {}", arg.to_string_lossy()))?;
            *slot = Some(value.to_string_lossy().parse::<usize>()?);
        }
        let mut positional = positional.into_iter();
        let path = positional
            .next()
            .ok_or_else(|| anyhow::anyhow!("missing archive"))?;
        let member = positional
            .next()
            .map(|s| s.to_string_lossy().parse::<u64>())
            .transpose()?;
        let output = positional.next();
        anyhow::ensure!(!passwords || extract, "--passwords goes with --extract");
        anyhow::ensure!(
            !encrypt || create.as_deref() == Some("zip"),
            "--encrypt goes with --create zip"
        );
        if create.is_some() {
            anyhow::ensure!(
                !extract && member.is_none(),
                "--create takes the archive to write, nothing else"
            );
        } else if extract {
            anyhow::ensure!(
                member.is_none(),
                "--extract takes the whole archive, no member"
            );
        } else if member.is_some() {
            anyhow::ensure!(
                bytes.is_some() && name.is_some(),
                "missing --name or --bytes bound for member read"
            );
        } else {
            anyhow::ensure!(
                members.is_some() && names.is_some(),
                "missing --members/--names bounds for listing"
            );
        }
        Ok(Args {
            path,
            stem,
            member,
            output,
            name,
            bytes,
            members,
            names,
            seconds,
            extract,
            passwords,
            create,
            encrypt,
        })
    }
    /// The member's name as the listing reports it: UTF-8 where libarchive
    /// can say, the raw bytes read lossily otherwise.
    fn entry_name(entry: *mut c_void) -> anyhow::Result<String> {
        let mut name = unsafe { archive_entry_pathname_utf8(entry) };
        if name.is_null() {
            name = unsafe { archive_entry_pathname(entry) };
        }
        anyhow::ensure!(!name.is_null(), "Archive member has no name");
        Ok(unsafe { CStr::from_ptr(name) }
            .to_string_lossy()
            .into_owned())
    }
    /// A string libarchive returns, read lossily; `None` for none.
    fn text(p: *const c_char) -> Option<String> {
        (!p.is_null()).then(|| unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned())
    }
    /// A file that is only compressed (`notes.txt.gz`, `dump.sql.xz`) is read
    /// as an archive of one member, which libarchive calls `data`; it is
    /// named after the file instead, less its compression suffix. Anything
    /// raw that was not decompressed is not an archive at all.
    fn member_name(reader: &Reader, entry: *mut c_void, stem: &str) -> anyhow::Result<String> {
        if unsafe { archive_format(reader.0) } != ARCHIVE_FORMAT_RAW {
            return entry_name(entry);
        }
        // The "none" filter is always there; a decompressor makes two.
        anyhow::ensure!(
            unsafe { archive_filter_count(reader.0) } > 1,
            "Unrecognized archive format"
        );
        Ok(stem.to_owned())
    }
    /// One line of JSON to the caller.
    fn tell(line: &serde_json::Value) -> anyhow::Result<()> {
        let mut out = std::io::stdout().lock();
        writeln!(out, "{line}")?;
        out.flush()?;
        Ok(())
    }

    /// The caller's units of work, a byte of standard input each, the
    /// reports of those taken, and the passphrases it gives when asked
    /// (`--passwords`): a ticket that arrives while one is read is kept
    /// for the next unit. Standard input is locked a read at a time:
    /// libarchive asks for a passphrase in the middle of reading.
    struct Tickets {
        taken: Cell<u64>,
        banked: Cell<u64>,
        /// Passphrases asked for the member being read: each member's
        /// attempts count from 1, so a second one says the first was wrong
        /// for it, not for another member.
        asked: Cell<u32>,
        /// What was given, while libarchive may still read it.
        given: RefCell<Vec<CString>>,
    }
    impl Tickets {
        fn new() -> Self {
            Tickets {
                taken: Cell::new(0),
                banked: Cell::new(0),
                asked: Cell::new(0),
                given: RefCell::new(Vec::new()),
            }
        }
        /// Wait for the next unit's ticket; take it and report every
        /// `REPORT_EVERY`, with the archive's bytes read so far.
        fn take(&self, reader: &Reader) -> anyhow::Result<()> {
            if self.banked.get() > 0 {
                self.banked.set(self.banked.get() - 1);
            } else {
                let mut ticket = [0u8; 1];
                anyhow::ensure!(std::io::stdin().read(&mut ticket)? == 1, "stopped");
                anyhow::ensure!(ticket[0] == TICKET, "unexpected input");
            }
            self.taken.set(self.taken.get() + 1);
            if self.taken.get().is_multiple_of(REPORT_EVERY) {
                self.report(reader)?;
            }
            Ok(())
        }
        fn report(&self, reader: &Reader) -> anyhow::Result<()> {
            let read = unsafe { archive_filter_bytes(reader.0, -1) };
            tell(&serde_json::json!({ "read": read.max(0), "units": self.taken.get() }))
        }
        /// Ask the caller for a passphrase and wait for it: `None` when
        /// it gives none, which libarchive takes as no more to try.
        fn passphrase(&self) -> Option<*const c_char> {
            self.asked.set(self.asked.get() + 1);
            tell(&serde_json::json!({ "passphrase": self.asked.get() })).ok()?;
            let mut line = Vec::new();
            let mut stdin = std::io::stdin();
            loop {
                let mut byte = [0u8; 1];
                if stdin.read(&mut byte).ok()? != 1 {
                    return None;
                }
                match byte[0] {
                    TICKET => self.banked.set(self.banked.get() + 1),
                    b'\n' => break,
                    other => line.push(other),
                }
            }
            let bytes = unhex(std::str::from_utf8(&line).ok()?).ok()?;
            if bytes.is_empty() {
                return None;
            }
            let given = CString::new(bytes).ok()?;
            let pointer = given.as_ptr();
            self.given.borrow_mut().push(given);
            Some(pointer)
        }
    }

    /// libarchive asking for a passphrase: the caller's, through `Tickets`.
    unsafe extern "C" fn asked_passphrase(
        _archive: *mut c_void,
        client: *mut c_void,
    ) -> *const c_char {
        // SAFETY: `client` is the `Tickets` that `extract` registered,
        // which `run` keeps alive until after the reader is freed.
        let tickets = unsafe { &*client.cast::<Tickets>() };
        tickets.passphrase().unwrap_or(std::ptr::null())
    }

    /// Bytes from their hexadecimal digits, two each.
    pub fn unhex(text: &str) -> anyhow::Result<Vec<u8>> {
        anyhow::ensure!(text.len().is_multiple_of(2), "odd hexadecimal");
        (0..text.len())
            .step_by(2)
            .map(|at| {
                text.get(at..at + 2)
                    .and_then(|pair| u8::from_str_radix(pair, 16).ok())
                    .ok_or_else(|| anyhow::anyhow!("not hexadecimal"))
            })
            .collect()
    }

    /// Whether an attribute an archive carries is put back on what is
    /// extracted: those a copy keeps (`skwir_core::fs::xattr::kept`), the
    /// user's on Linux, all but what the system sets itself on a Mac. Of
    /// an archive nobody vouched for, not what tells the Mac which
    /// application opens the file (`com.apple.LaunchServices.*`): the
    /// archive's author would choose it (ADR-059).
    fn kept(name: &[u8]) -> bool {
        if cfg!(target_vendor = "apple") {
            name != b"com.apple.rootless"
                && name != b"com.apple.provenance"
                && !name.starts_with(b"com.apple.LaunchServices.")
        } else {
            name.starts_with(b"user.")
        }
    }

    /// Leave on `entry` only the attributes that are put back (`kept`).
    fn keep_attributes(entry: *mut c_void) {
        let mut kept_ones: Vec<(CString, Vec<u8>)> = Vec::new();
        unsafe { archive_entry_xattr_reset(entry) };
        loop {
            let mut name: *const c_char = std::ptr::null();
            let mut value: *const c_void = std::ptr::null();
            let mut size = 0usize;
            let status = unsafe {
                archive_entry_xattr_next(entry, &raw mut name, &raw mut value, &raw mut size)
            };
            if status != ARCHIVE_OK || name.is_null() {
                break;
            }
            let name = unsafe { CStr::from_ptr(name) }.to_owned();
            if !kept(name.to_bytes()) {
                continue;
            }
            let value = if value.is_null() || size == 0 {
                Vec::new()
            } else {
                unsafe { std::slice::from_raw_parts(value.cast::<u8>(), size) }.to_vec()
            };
            kept_ones.push((name, value));
        }
        unsafe { archive_entry_xattr_clear(entry) };
        for (name, value) in kept_ones {
            unsafe {
                archive_entry_xattr_add_entry(
                    entry,
                    name.as_ptr(),
                    value.as_ptr().cast(),
                    value.len(),
                );
            }
        }
    }

    /// Every entry of `reader` written into the current folder by
    /// libarchive's disk writer, with its guards (`EXTRACT_*`): a folder,
    /// a file, a link (one the archive makes is never written through),
    /// and nothing else, with the attributes a copy keeps (`kept`); one
    /// the guards refuse stops it all, and so does an encrypted member
    /// unless the caller gives its passphrase (`passwords`), and a member
    /// whose bytes are not those stored (`data`). A file only compressed is
    /// written under `stem`.
    fn extract(
        reader: &Reader,
        tickets: &Tickets,
        stem: &str,
        passwords: bool,
    ) -> anyhow::Result<()> {
        let writer = Writer(unsafe { archive_write_disk_new() });
        anyhow::ensure!(!writer.0.is_null(), "libarchive allocation failed");
        check(writer.0, unsafe {
            archive_write_disk_set_options(
                writer.0,
                EXTRACT_TIME
                    | EXTRACT_XATTR
                    | EXTRACT_SECURE_SYMLINKS
                    | EXTRACT_SECURE_NODOTDOT
                    | EXTRACT_SECURE_NOABSOLUTEPATHS,
            )
        })?;
        let stem = CString::new(stem)?;
        loop {
            tickets.take(reader)?;
            let mut entry = std::ptr::null_mut();
            let status = unsafe { archive_read_next_header(reader.0, &raw mut entry) };
            if status == ARCHIVE_EOF {
                break;
            }
            reader.check(status)?;
            anyhow::ensure!(!entry.is_null(), "Missing archive header");
            let kind = unsafe { archive_entry_filetype(entry) };
            let linked = !unsafe { archive_entry_hardlink(entry) }.is_null();
            if !linked && ![AE_IFREG, AE_IFDIR, AE_IFLNK].contains(&kind) {
                // A pipe, a device or a socket: nothing a user extracts.
                continue;
            }
            if unsafe { archive_format(reader.0) } == ARCHIVE_FORMAT_RAW {
                // As listed (`member_name`): only a file decompressed is one.
                anyhow::ensure!(
                    unsafe { archive_filter_count(reader.0) } > 1,
                    "Unrecognized archive format"
                );
                unsafe { archive_entry_set_pathname_utf8(entry, stem.as_ptr()) };
            }
            let name = entry_name(entry)?;
            let encrypted = unsafe { archive_entry_is_encrypted(entry) } != 0;
            anyhow::ensure!(
                passwords || !encrypted,
                "“{name}” is encrypted: open the archive in an archive application"
            );
            // Its passphrase, if it needs one, is asked from the first.
            tickets.asked.set(0);
            keep_attributes(entry);
            check(writer.0, unsafe { archive_write_header(writer.0, entry) })
                .map_err(|e| anyhow::anyhow!("“{name}”: {e}"))?;
            let mut unit = 0usize;
            loop {
                let mut block: *const c_void = std::ptr::null();
                let mut size = 0usize;
                let mut offset = 0i64;
                let status = unsafe {
                    archive_read_data_block(
                        reader.0,
                        &raw mut block,
                        &raw mut size,
                        &raw mut offset,
                    )
                };
                if status == ARCHIVE_EOF {
                    break;
                }
                // A warning here says the bytes read are not those stored.
                data(reader.0, status, &name, encrypted)?;
                if size == 0 {
                    continue;
                }
                let written = unsafe { archive_write_data_block(writer.0, block, size, offset) };
                if written < 0 {
                    // More bytes than its header said: it is cut there.
                    data(writer.0, c_int::try_from(written)?, &name, false)?;
                }
                unit += size;
                while unit >= UNIT_BYTES {
                    unit -= UNIT_BYTES;
                    tickets.take(reader)?;
                }
            }
            // Its times, mode and attributes put back: a warning is one
            // the drive refused, or a failure.
            let status = unsafe { archive_write_finish_entry(writer.0) };
            anyhow::ensure!(
                status >= ARCHIVE_OK || status == ARCHIVE_WARN && refused_by_the_drive(writer.0),
                "“{name}”: {}",
                said(writer.0)
            );
        }
        // Folders get their times and modes last, once filled.
        check(writer.0, unsafe { archive_write_close(writer.0) })?;
        tickets.report(reader)
    }

    /// A libarchive entry, freed once written.
    struct Entry(*mut c_void);
    impl Drop for Entry {
        fn drop(&mut self) {
            unsafe { archive_entry_free(self.0) };
        }
    }

    /// The bytes a header field names in hexadecimal.
    fn hex_field(header: &serde_json::Value, key: &str) -> anyhow::Result<Vec<u8>> {
        unhex(
            header
                .get(key)
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| anyhow::anyhow!("an entry without {key}"))?,
        )
    }

    /// A line of standard input, without its end: `None` at its end.
    fn line(input: &mut impl BufRead) -> anyhow::Result<Option<String>> {
        let mut line = Vec::new();
        if input.read_until(b'\n', &mut line)? == 0 {
            return Ok(None);
        }
        anyhow::ensure!(line.pop() == Some(b'\n'), "stopped");
        Ok(Some(String::from_utf8(line)?))
    }

    /// The archive of `format` written at `output`, of what standard input
    /// says, as skwir walked it (it opens none of the user's files): with
    /// `encrypt`, first a line of the passphrase in hexadecimal (ZIP's
    /// AES-256); then each entry, a line of JSON (`name` in hexadecimal, as
    /// its path in the archive; `kind`, `file`, `dir`, `link` or `hard`, a
    /// second name of a file already written, whose name is `target`;
    /// `size`; `mode`; `mtime`, seconds and nanoseconds; a link's `target`;
    /// `xattrs`, pairs in hexadecimal), and a file's `size` bytes after it;
    /// then `{"end": true}`. Input that ends before says the caller
    /// stopped: nothing written is whole.
    fn create(output: &NativePath, format: &str, encrypt: bool) -> anyhow::Result<()> {
        let writer = Writer(unsafe { archive_write_new() });
        let w = writer.0;
        anyhow::ensure!(!w.is_null(), "libarchive allocation failed");
        unsafe {
            match format {
                "zip" => {
                    check(w, archive_write_set_format_zip(w))?;
                    #[cfg(windows)]
                    check(w, archive_write_set_options(w, ZIP_OPTIONS.as_ptr()))?;
                }
                "7z" => {
                    check(w, archive_write_set_format_7zip(w))?;
                    check(
                        w,
                        archive_write_set_options(w, c"7zip:compression=lzma2".as_ptr()),
                    )?;
                }
                "tar" | "tar.gz" | "tar.bz2" | "tar.xz" | "tar.zst" => {
                    check(w, archive_write_set_format_pax_restricted(w))?;
                    match format {
                        "tar.gz" => check(w, archive_write_add_filter_gzip(w))?,
                        "tar.bz2" => check(w, archive_write_add_filter_bzip2(w))?,
                        "tar.xz" => check(w, archive_write_add_filter_xz(w))?,
                        "tar.zst" => check(w, archive_write_add_filter_zstd(w))?,
                        _ => {}
                    }
                }
                other => anyhow::bail!("unknown format {other}"),
            }
        }
        let mut input = std::io::stdin().lock();
        // Kept until the archive is written: libarchive reads it as it
        // encrypts.
        let mut passphrase = None;
        if encrypt {
            let given = unhex(&line(&mut input)?.ok_or_else(|| anyhow::anyhow!("stopped"))?)?;
            anyhow::ensure!(!given.is_empty(), "an empty password");
            let given = CString::new(given)?;
            unsafe {
                check(
                    w,
                    archive_write_set_options(w, c"zip:encryption=aes256".as_ptr()),
                )?;
                check(w, archive_write_set_passphrase(w, given.as_ptr()))?;
            }
            passphrase = Some(given);
        }
        check(w, unsafe { open_write(w, output.as_ptr()) })?;
        let mut buffer = vec![0u8; 256 * 1024];
        loop {
            let header = line(&mut input)?.ok_or_else(|| anyhow::anyhow!("stopped"))?;
            let header: serde_json::Value = serde_json::from_str(&header)?;
            if header.get("end").and_then(serde_json::Value::as_bool) == Some(true) {
                break;
            }
            let name = hex_field(&header, "name")?;
            let shown = String::from_utf8_lossy(&name).into_owned();
            let number = |key: &str| header.get(key).and_then(serde_json::Value::as_u64);
            let kind = header
                .get("kind")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            let size = if kind == "file" {
                number("size").unwrap_or(0)
            } else {
                0
            };
            let entry = Entry(unsafe { archive_entry_new() });
            anyhow::ensure!(!entry.0.is_null(), "libarchive allocation failed");
            let e = entry.0;
            let utf8 = |bytes: &[u8]| std::str::from_utf8(bytes).is_ok();
            let name_c = CString::new(name.clone())?;
            unsafe {
                if utf8(&name) {
                    archive_entry_set_pathname_utf8(e, name_c.as_ptr());
                } else {
                    archive_entry_set_pathname(e, name_c.as_ptr());
                }
                let mode = number("mode").unwrap_or(0o644) & 0o7777;
                archive_entry_set_perm(e, Mode::try_from(mode).unwrap_or(0o644));
                archive_entry_set_size(e, i64::try_from(size)?);
                if let Some([seconds, nanoseconds]) = header
                    .get("mtime")
                    .and_then(|t| serde_json::from_value::<[i64; 2]>(t.clone()).ok())
                {
                    archive_entry_set_mtime(e, seconds, c_long::try_from(nanoseconds)?);
                }
            }
            match kind {
                "dir" => unsafe { archive_entry_set_filetype(e, AE_IFDIR) },
                "file" => unsafe { archive_entry_set_filetype(e, AE_IFREG) },
                "link" | "hard" => {
                    let target = hex_field(&header, "target")?;
                    let target_c = CString::new(target.clone())?;
                    unsafe {
                        if kind == "link" {
                            archive_entry_set_filetype(e, AE_IFLNK);
                            if utf8(&target) {
                                archive_entry_set_symlink_utf8(e, target_c.as_ptr());
                            } else {
                                archive_entry_set_symlink(e, target_c.as_ptr());
                            }
                        } else {
                            archive_entry_set_filetype(e, AE_IFREG);
                            if utf8(&target) {
                                archive_entry_set_hardlink_utf8(e, target_c.as_ptr());
                            } else {
                                archive_entry_set_hardlink(e, target_c.as_ptr());
                            }
                        }
                    }
                }
                other => anyhow::bail!("“{shown}”: unknown kind {other}"),
            }
            if let Some(pairs) = header.get("xattrs").and_then(serde_json::Value::as_array) {
                for pair in pairs {
                    let (Some(n), Some(v)) = (
                        pair.get(0).and_then(serde_json::Value::as_str),
                        pair.get(1).and_then(serde_json::Value::as_str),
                    ) else {
                        continue;
                    };
                    let (n, v) = (CString::new(unhex(n)?)?, unhex(v)?);
                    unsafe {
                        archive_entry_xattr_add_entry(e, n.as_ptr(), v.as_ptr().cast(), v.len());
                    }
                }
            }
            check(w, unsafe { archive_write_header(w, e) })
                .map_err(|error| anyhow::anyhow!("“{shown}”: {error}"))?;
            let mut left = size;
            while left > 0 {
                let want = usize::try_from(left.min(buffer.len() as u64))?;
                input
                    .read_exact(&mut buffer[..want])
                    .map_err(|_| anyhow::anyhow!("stopped"))?;
                let mut at = 0;
                while at < want {
                    let wrote = unsafe {
                        archive_write_data(w, buffer[at..want].as_ptr().cast(), want - at)
                    };
                    if wrote <= 0 {
                        check(w, c_int::try_from(wrote).unwrap_or(-30))
                            .map_err(|error| anyhow::anyhow!("“{shown}”: {error}"))?;
                        anyhow::bail!("“{shown}” could not be written");
                    }
                    at += usize::try_from(wrote)?;
                }
                left -= want as u64;
            }
            check(w, unsafe { archive_write_finish_entry(w) })
                .map_err(|error| anyhow::anyhow!("“{shown}”: {error}"))?;
        }
        check(w, unsafe { archive_write_close(w) })?;
        drop(writer);
        drop(passphrase);
        Ok(())
    }

    pub fn run() -> anyhow::Result<()> {
        let Args {
            path,
            stem,
            member,
            output,
            name: expected,
            bytes: byte_limit,
            members: member_limit,
            names: name_limit,
            seconds,
            extract: extracting,
            passwords,
            create: creating,
            encrypt,
        } = parse_args(std::env::args_os().skip(1))?;
        let started = std::time::Instant::now();
        let budget = seconds.map(|s| std::time::Duration::from_secs(s as u64));
        utf8_locale();
        // What a file only compressed calls its member: the stem given, else
        // the file's own.
        let stem = stem
            .map(std::path::PathBuf::from)
            .or_else(|| std::path::Path::new(&path).file_stem().map(Into::into))
            .map_or_else(|| "data".into(), |s| s.to_string_lossy().into_owned());
        let path = native_path(&path)?;
        if let Some(format) = creating {
            return create(&path, &format, encrypt);
        }
        // Before the reader, so it outlives it: the reader asks it for
        // passphrases (`extract`).
        let tickets = Tickets::new();
        let reader = Reader(unsafe { archive_read_new() });
        anyhow::ensure!(!reader.0.is_null(), "libarchive allocation failed");
        unsafe {
            reader.check(archive_read_support_filter_all(reader.0))?;
            reader.check(archive_read_support_format_all(reader.0))?;
            // Raw bids last: it is what is left when no archive format is.
            reader.check(archive_read_support_format_raw(reader.0))?;
            // Before it is opened, as libarchive wants it.
            if extracting && passwords {
                reader.check(archive_read_set_passphrase_callback(
                    reader.0,
                    std::ptr::from_ref::<Tickets>(&tickets).cast_mut().cast(),
                    Some(asked_passphrase),
                ))?;
            }
            reader.check(archive_read_set_options(reader.0, READ_OPTIONS.as_ptr()))?;
            reader.check(open_read(reader.0, path.as_ptr(), 64 * 1024))?;
        }
        if extracting {
            return extract(&reader, &tickets, &stem, passwords);
        }
        let mut entries = Vec::new();
        let mut index = 0u64;
        let mut names = 0usize;
        let mut complete = true;
        loop {
            let mut entry = std::ptr::null_mut();
            let status = unsafe { archive_read_next_header(reader.0, &raw mut entry) };
            if status == ARCHIVE_EOF {
                break;
            }
            reader.check(status)?;
            anyhow::ensure!(!entry.is_null(), "Missing archive header");
            if member == Some(index) {
                let name = member_name(&reader, entry, &stem)?;
                anyhow::ensure!(
                    expected.as_deref().and_then(|e| e.to_str()) == Some(name.as_str()),
                    "Archive changed since it was listed; select it again"
                );
                anyhow::ensure!(
                    unsafe { archive_entry_filetype(entry) } == 0o100_000,
                    "Only regular archive members can be previewed"
                );
                anyhow::ensure!(
                    unsafe { archive_entry_is_encrypted(entry) } == 0,
                    "Encrypted member: open in an archive application"
                );
                let output = output
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("missing output"))?;
                let limit = byte_limit.unwrap_or(0);
                let mut file = std::fs::File::create(output)?;
                let mut total = 0usize;
                let mut buf = vec![0u8; 64 * 1024];
                let streamed = (|| -> anyhow::Result<()> {
                    loop {
                        let n = unsafe {
                            archive_read_data(reader.0, buf.as_mut_ptr().cast(), buf.len())
                        };
                        if n < 0 {
                            // A warning too: the bytes are not those stored
                            // (a CRC that does not match).
                            data(reader.0, c_int::try_from(n)?, &name, false)?;
                            anyhow::bail!("Archive decompression failed");
                        }
                        if n == 0 {
                            return Ok(());
                        }
                        let n = usize::try_from(n)?;
                        total += n;
                        // The caller names its bound in its own words; this
                        // is the backstop for a member larger than it
                        // declared.
                        anyhow::ensure!(total <= limit, "Member exceeds the preview limit");
                        file.write_all(&buf[..n])?;
                    }
                })();
                if streamed.is_err() {
                    // Nothing of a member not read whole is shown.
                    drop(file);
                    let _ = std::fs::remove_file(output);
                }
                return streamed;
            }
            if member.is_none() {
                let name = member_name(&reader, entry, &stem)?;
                let kind = unsafe { archive_entry_filetype(entry) };
                // What a link says, and the first name of a hard link: as
                // names, they count against the names' bound.
                let link = (kind == AE_IFLNK)
                    .then(|| {
                        text(unsafe { archive_entry_symlink_utf8(entry) })
                            .or_else(|| text(unsafe { archive_entry_symlink(entry) }))
                    })
                    .flatten();
                let hardlink = text(unsafe { archive_entry_hardlink_utf8(entry) })
                    .or_else(|| text(unsafe { archive_entry_hardlink(entry) }));
                names += name.len()
                    + link.as_ref().map_or(0, String::len)
                    + hardlink.as_ref().map_or(0, String::len);
                // Past either bound the listing ends there: what was
                // listed is shown, with a word that it is not all.
                // So past the time given: a large compressed tar is read
                // whole to be listed, and was not listed at all.
                if names > name_limit.unwrap_or(0)
                    || entries.len() >= member_limit.unwrap_or(0)
                    || budget.is_some_and(|budget| started.elapsed() > budget)
                {
                    complete = false;
                    break;
                }
                let size = if unsafe { archive_entry_size_is_set(entry) } != 0 {
                    unsafe { archive_entry_size(entry) }
                } else {
                    -1
                };
                // Its time as libarchive reads it (a ZIP's local time
                // without a zone in this process's): what extracting it
                // gives the file, to tell an extraction done (ADR-059).
                // A `long`, 32 bits on Windows.
                #[allow(clippy::useless_conversion)]
                let mtime = (unsafe { archive_entry_mtime_is_set(entry) } != 0).then(|| {
                    [
                        unsafe { archive_entry_mtime(entry) },
                        i64::from(unsafe { archive_entry_mtime_nsec(entry) }),
                    ]
                });
                entries.push(serde_json::json!({
                    "index": index,
                    "name": name,
                    "size": size,
                    "directory": kind == AE_IFDIR,
                    "regular": kind == AE_IFREG,
                    "encrypted": unsafe { archive_entry_is_encrypted(entry) } != 0,
                    "mtime": mtime,
                    "link": link,
                    "hardlink": hardlink,
                }));
            }
            index += 1;
        }
        anyhow::ensure!(member.is_none(), "Archive member no longer exists");
        println!(
            "{}",
            serde_json::json!({"members": entries, "complete": complete})
        );
        Ok(())
    }
}
fn main() {
    // `--build`, skwir asking which build of it this helper is, when a
    // call of it failed (`platform::process::explain`).
    if std::env::args_os()
        .nth(1)
        .is_some_and(|arg| arg == "--build")
    {
        println!("{}", env!("SKWIR_BUILD"));
        return;
    }
    // `--probe`, skwir's settings asking whether this helper works here
    // (`platform::process::probe`): being started at all says its
    // libraries are there.
    if std::env::args_os()
        .nth(1)
        .is_some_and(|arg| arg == "--probe")
    {
        return;
    }
    if let Err(error) = native::run() {
        eprintln!("{error:#}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::native::{parse_args, unhex};
    fn args(list: &[&str]) -> Vec<std::ffi::OsString> {
        list.iter().map(std::ffi::OsString::from).collect()
    }
    #[test]
    fn bounds_are_required_and_parsed() {
        let listing = parse_args(args(&["a.zip", "--members", "10", "--names", "2048"])).unwrap();
        assert_eq!(listing.member, None);
        assert_eq!((listing.members, listing.names), (Some(10), Some(2048)));
        let read = parse_args(args(&[
            "a.zip",
            "3",
            "out",
            "--name",
            "dir/caf\u{e9}.txt",
            "--bytes",
            "4096",
        ]))
        .unwrap();
        assert_eq!(read.member, Some(3));
        assert_eq!(read.output.as_deref().and_then(|o| o.to_str()), Some("out"));
        assert_eq!(
            read.name.as_deref().and_then(|o| o.to_str()),
            Some("dir/caf\u{e9}.txt")
        );
        assert_eq!(read.bytes, Some(4096));
        assert!(parse_args(args(&["a.zip"])).is_err());
        assert!(parse_args(args(&["a.zip", "3", "out"])).is_err());
        assert!(parse_args(args(&["a.zip", "3", "out", "--bytes", "4096"])).is_err());
        assert!(parse_args(args(&["a.zip", "--members", "10"])).is_err());
        assert!(parse_args(args(&["a.zip", "--bytes"])).is_err());
        assert!(parse_args(args(&["a.zip", "3", "out", "--name"])).is_err());
        assert!(parse_args(args(&[])).is_err());
        let whole = parse_args(args(&["a.zip", "--extract", "--stem", "a"])).unwrap();
        assert!(whole.extract && whole.member.is_none());
        assert!(parse_args(args(&["a.zip", "3", "--extract"])).is_err());
    }
    #[test]
    fn an_archive_is_made_of_its_format_alone() {
        let made = parse_args(args(&["/work/archive", "--create", "tar.zst"])).unwrap();
        assert_eq!(made.create.as_deref(), Some("tar.zst"));
        assert!(!made.encrypt && !made.extract);
        let locked = parse_args(args(&["out", "--create", "zip", "--encrypt"])).unwrap();
        assert!(locked.encrypt);
        assert!(parse_args(args(&["out", "--create", "7z", "--encrypt"])).is_err());
        assert!(parse_args(args(&["out", "--create", "zip", "--extract"])).is_err());
        assert!(parse_args(args(&["out", "3", "--create", "zip"])).is_err());
        assert!(parse_args(args(&["out", "--create"])).is_err());
        let asked = parse_args(args(&["a.zip", "--extract", "--passwords"])).unwrap();
        assert!(asked.passwords);
        assert!(
            parse_args(args(&[
                "a.zip",
                "--members",
                "1",
                "--names",
                "9",
                "--passwords"
            ]))
            .is_err()
        );
    }

    #[test]
    fn hexadecimal_is_read_two_digits_a_byte() {
        assert_eq!(unhex("").unwrap(), b"");
        assert_eq!(unhex("c3a9 ".trim()).unwrap(), "é".as_bytes());
        assert_eq!(unhex("00ff7F").unwrap(), [0, 255, 127]);
        assert!(unhex("abc").is_err());
        assert!(unhex("zz").is_err());
    }

    #[test]
    fn an_option_it_does_not_know_is_refused_by_its_name() {
        let Err(error) = parse_args(args(&[
            "a.zip",
            "--members",
            "10",
            "--names",
            "2048",
            "--later",
            "27",
        ])) else {
            panic!("an option it does not know was taken");
        };
        assert_eq!(error.to_string(), "unknown option --later");
    }
}
