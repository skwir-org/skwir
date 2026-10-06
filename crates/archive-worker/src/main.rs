//! Optional libarchive boundary. Never extract archive paths to the filesystem.
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
        fn archive_error_string(a: *mut c_void) -> *const c_char;
        fn archive_read_free(a: *mut c_void) -> c_int;
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
            if status < ARCHIVE_WARN {
                let p = unsafe { archive_error_string(self.0) };
                let error = if p.is_null() {
                    "Archive error".into()
                } else {
                    unsafe { CStr::from_ptr(p) }.to_string_lossy()
                };
                anyhow::bail!("{error}");
            }
            Ok(())
        }
    }
    /// `bibli-archive-worker <archive> --members N --names N` lists members;
    /// `bibli-archive-worker <archive> <index> <output> --name NAME --bytes N`
    /// streams the member at `index`, provided it is still called `NAME`, so
    /// an archive rewritten since its listing never yields another member.
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
    }
    pub fn parse_args(args: impl IntoIterator<Item = std::ffi::OsString>) -> anyhow::Result<Args> {
        let mut positional = Vec::new();
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
        if member.is_some() {
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
    }
}
