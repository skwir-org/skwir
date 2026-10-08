//! Optional libarchive boundary. A member's name reaches the filesystem
//! only when a whole archive is extracted (`--extract`, ADR-048), through
//! libarchive's own extraction and its guards, in the folder it runs in.
// SAFETY: all native handles stay on this thread; strings are copied while the
// current entry is alive, buffers carry their exact lengths, and Reader owns
// the single archive_read_free call. Native code is isolated from the UI.
#[allow(unsafe_code)]
mod native {
    use std::{
        ffi::{CStr, CString, c_char, c_int, c_void},
        io::Write,
    };
    #[link(name = "archive")]
    unsafe extern "C" {
        fn archive_read_new() -> *mut c_void;
        fn archive_read_support_filter_all(a: *mut c_void) -> c_int;
        fn archive_read_support_format_all(a: *mut c_void) -> c_int;
        fn archive_read_support_format_raw(a: *mut c_void) -> c_int;
        fn archive_format(a: *mut c_void) -> c_int;
        fn archive_filter_count(a: *mut c_void) -> c_int;
        fn archive_read_open_filename(a: *mut c_void, path: *const c_char, block: usize) -> c_int;
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
        fn archive_read_free(a: *mut c_void) -> c_int;
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
    }
    // From the C library std already links.
    unsafe extern "C" {
        fn setlocale(category: c_int, locale: *const c_char) -> *mut c_char;
    }
    #[cfg(any(
        target_vendor = "apple",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd",
        target_os = "dragonfly"
    ))]
    const LC_CTYPE: c_int = 2;
    #[cfg(not(any(
        target_vendor = "apple",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd",
        target_os = "dragonfly"
    )))]
    const LC_CTYPE: c_int = 0;

    const ARCHIVE_EOF: c_int = 1;
    /// `ARCHIVE_FORMAT_RAW`: no archive at all, one stream of data.
    const ARCHIVE_FORMAT_RAW: c_int = 0x90000;
    /// A warning is not a failure: libarchive reports one when, for instance,
    /// a name cannot be converted to the locale's charset, and the entry is
    /// still there to read. Only `ARCHIVE_FAILED` and `ARCHIVE_FATAL` stop us.
    const ARCHIVE_WARN: c_int = -20;

    /// How a whole archive is extracted (`archive.h`): modification times
    /// kept; never a member written outside the folder, by `..`, by an
    /// absolute path, or through a link the archive made before it. No
    /// owner, and no set-id or sticky bits: not asked for.
    const EXTRACT_TIME: c_int = 0x0004;
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

    /// libarchive converts member names through the C library's charset. A
    /// process starts in the "C" locale, where any non-ASCII name fails to
    /// convert, so ask for UTF-8 first and the environment's locale after.
    fn utf8_locale() {
        for locale in [c"C.UTF-8", c""] {
            if !unsafe { setlocale(LC_CTYPE, locale.as_ptr()) }.is_null() {
                return;
            }
        }
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
    /// libarchive's disk writer: what `--extract` writes with.
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
            let p = unsafe { archive_error_string(a) };
            let error = if p.is_null() {
                "Archive error".into()
            } else {
                unsafe { CStr::from_ptr(p) }.to_string_lossy()
            };
            anyhow::bail!("{error}");
        }
        Ok(())
    }
    /// `bibli-archive-worker <archive> --members N --names N` lists members;
    /// `bibli-archive-worker <archive> <index> <output> --name NAME --bytes N`
    /// streams the member at `index`, provided it is still called `NAME`, so
    /// an archive rewritten since its listing never yields another member;
    /// `bibli-archive-worker <archive> --extract` extracts it all into the
    /// folder it runs in, taking a byte of standard input before each unit
    /// of work (an entry, or `UNIT_BYTES` of one: none, and it waits) and
    /// reporting `{"read": bytes of the archive read, "units": taken}`
    /// lines on standard output.
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
        pub extract: bool,
    }
    pub fn parse_args(args: impl IntoIterator<Item = std::ffi::OsString>) -> anyhow::Result<Args> {
        let mut positional = Vec::new();
        let mut extract = false;
        let mut name = None;
        let mut stem = None;
        let mut bytes = None;
        let mut members = None;
        let mut names = None;
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            let slot = match arg.to_str() {
                Some("--bytes") => &mut bytes,
                Some("--members") => &mut members,
                Some("--names") => &mut names,
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
        if extract {
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
            extract,
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
    /// The caller's units of work, a byte of standard input each, and the
    /// reports of those taken.
    struct Tickets {
        input: std::io::StdinLock<'static>,
        output: std::io::StdoutLock<'static>,
        taken: u64,
    }
    impl Tickets {
        /// Wait for the next unit's ticket; take it and report every
        /// `REPORT_EVERY`, with the archive's bytes read so far.
        fn take(&mut self, reader: &Reader) -> anyhow::Result<()> {
            let mut ticket = [0u8; 1];
            anyhow::ensure!(
                std::io::Read::read(&mut self.input, &mut ticket)? == 1,
                "stopped"
            );
            self.taken += 1;
            if self.taken.is_multiple_of(REPORT_EVERY) {
                self.report(reader)?;
            }
            Ok(())
        }
        fn report(&mut self, reader: &Reader) -> anyhow::Result<()> {
            let read = unsafe { archive_filter_bytes(reader.0, -1) };
            writeln!(
                self.output,
                "{}",
                serde_json::json!({ "read": read.max(0), "units": self.taken })
            )?;
            self.output.flush()?;
            Ok(())
        }
    }

    /// Every entry of `reader` written into the current folder by
    /// libarchive's disk writer, with its guards (`EXTRACT_*`): a folder,
    /// a file, a link (one the archive makes is never written through),
    /// and nothing else; an encrypted member, or one the guards refuse,
    /// stops it all. A file only compressed is written under `stem`.
    fn extract(reader: &Reader, stem: &str) -> anyhow::Result<()> {
        let writer = Writer(unsafe { archive_write_disk_new() });
        anyhow::ensure!(!writer.0.is_null(), "libarchive allocation failed");
        check(writer.0, unsafe {
            archive_write_disk_set_options(
                writer.0,
                EXTRACT_TIME
                    | EXTRACT_SECURE_SYMLINKS
                    | EXTRACT_SECURE_NODOTDOT
                    | EXTRACT_SECURE_NOABSOLUTEPATHS,
            )
        })?;
        let mut tickets = Tickets {
            input: std::io::stdin().lock(),
            output: std::io::stdout().lock(),
            taken: 0,
        };
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
            anyhow::ensure!(
                unsafe { archive_entry_is_encrypted(entry) } == 0,
                "“{name}” is encrypted: open the archive in an archive application"
            );
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
                reader
                    .check(status)
                    .map_err(|e| anyhow::anyhow!("“{name}”: {e}"))?;
                if size == 0 {
                    continue;
                }
                let written = unsafe { archive_write_data_block(writer.0, block, size, offset) };
                if written < 0 {
                    check(writer.0, c_int::try_from(written)?)
                        .map_err(|e| anyhow::anyhow!("“{name}”: {e}"))?;
                }
                unit += size;
                while unit >= UNIT_BYTES {
                    unit -= UNIT_BYTES;
                    tickets.take(reader)?;
                }
            }
            check(writer.0, unsafe { archive_write_finish_entry(writer.0) })
                .map_err(|e| anyhow::anyhow!("“{name}”: {e}"))?;
        }
        // Folders get their times and modes last, once filled.
        check(writer.0, unsafe { archive_write_close(writer.0) })?;
        tickets.report(reader)
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
            extract: extracting,
        } = parse_args(std::env::args_os().skip(1))?;
        utf8_locale();
        // What a file only compressed calls its member: the stem given, else
        // the file's own.
        let stem = stem
            .map(std::path::PathBuf::from)
            .or_else(|| std::path::Path::new(&path).file_stem().map(Into::into))
            .map_or_else(|| "data".into(), |s| s.to_string_lossy().into_owned());
        #[cfg(unix)]
        let path = {
            use std::os::unix::ffi::OsStrExt;
            CString::new(path.as_bytes())?
        };
        #[cfg(not(unix))]
        let path = CString::new(path.to_string_lossy().as_bytes())?;
        let reader = Reader(unsafe { archive_read_new() });
        anyhow::ensure!(!reader.0.is_null(), "libarchive allocation failed");
        unsafe {
            reader.check(archive_read_support_filter_all(reader.0))?;
            reader.check(archive_read_support_format_all(reader.0))?;
            // Raw bids last: it is what is left when no archive format is.
            reader.check(archive_read_support_format_raw(reader.0))?;
            reader.check(archive_read_open_filename(
                reader.0,
                path.as_ptr(),
                64 * 1024,
            ))?;
        }
        if extracting {
            return extract(&reader, &stem);
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
                anyhow::ensure!(
                    expected.as_deref().and_then(|e| e.to_str())
                        == Some(member_name(&reader, entry, &stem)?.as_str()),
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
                loop {
                    let n =
                        unsafe { archive_read_data(reader.0, buf.as_mut_ptr().cast(), buf.len()) };
                    if n == ARCHIVE_WARN as isize {
                        // The block is kept and handed out by the next call.
                        continue;
                    }
                    if n < 0 {
                        reader.check(c_int::try_from(n)?)?;
                        anyhow::bail!("Archive decompression failed");
                    }
                    if n == 0 {
                        break;
                    }
                    let n = usize::try_from(n)?;
                    total += n;
                    // The caller names its bound in its own words; this is
                    // the backstop for a member larger than it declared.
                    anyhow::ensure!(total <= limit, "Member exceeds the preview limit");
                    file.write_all(&buf[..n])?;
                }
                return Ok(());
            }
            if member.is_none() {
                let name = member_name(&reader, entry, &stem)?;
                names += name.len();
                // Past either bound the listing ends there: what was
                // listed is shown, with a word that it is not all.
                if names > name_limit.unwrap_or(0) || entries.len() >= member_limit.unwrap_or(0) {
                    complete = false;
                    break;
                }
                let kind = unsafe { archive_entry_filetype(entry) };
                let size = if unsafe { archive_entry_size_is_set(entry) } != 0 {
                    unsafe { archive_entry_size(entry) }
                } else {
                    -1
                };
                entries.push(serde_json::json!({
                    "index": index,
                    "name": name,
                    "size": size,
                    "directory": kind == 0o040_000,
                    "regular": kind == 0o100_000,
                    "encrypted": unsafe { archive_entry_is_encrypted(entry) } != 0,
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
    // `--probe`, bibli's settings asking whether this helper works here
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
    use super::native::parse_args;
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
}
