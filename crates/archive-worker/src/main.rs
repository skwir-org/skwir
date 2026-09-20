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
        fn archive_read_open_filename(a: *mut c_void, path: *const c_char, block: usize) -> c_int;
        fn archive_read_next_header(a: *mut c_void, entry: *mut *mut c_void) -> c_int;
        fn archive_entry_pathname_utf8(e: *mut c_void) -> *const c_char;
        fn archive_entry_pathname(e: *mut c_void) -> *const c_char;
        fn archive_entry_size(e: *mut c_void) -> i64;
        fn archive_entry_filetype(e: *mut c_void) -> u32;
        fn archive_entry_is_encrypted(e: *mut c_void) -> c_int;
        fn archive_read_data(a: *mut c_void, buf: *mut c_void, size: usize) -> isize;
        fn archive_error_string(a: *mut c_void) -> *const c_char;
        fn archive_read_free(a: *mut c_void) -> c_int;
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
            if status < 0 {
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
    /// `bibli-archive-worker <archive> <index> <output> --bytes N` streams one.
    /// Every bound arrives from the caller so the UI process and this helper
    /// can never disagree on a limit.
    pub struct Args {
        pub path: std::ffi::OsString,
        pub member: Option<u64>,
        pub output: Option<std::ffi::OsString>,
        pub bytes: Option<usize>,
        pub members: Option<usize>,
        pub names: Option<usize>,
    }
    pub fn parse_args(args: impl IntoIterator<Item = std::ffi::OsString>) -> anyhow::Result<Args> {
        let mut positional = Vec::new();
        let mut bytes = None;
        let mut members = None;
        let mut names = None;
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            let slot = match arg.to_str() {
                Some("--bytes") => &mut bytes,
                Some("--members") => &mut members,
                Some("--names") => &mut names,
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
            anyhow::ensure!(bytes.is_some(), "missing --bytes bound for member read");
        } else {
            anyhow::ensure!(
                members.is_some() && names.is_some(),
                "missing --members/--names bounds for listing"
            );
        }
        Ok(Args {
            path,
            member,
            output,
            bytes,
            members,
            names,
        })
    }
    pub fn run() -> anyhow::Result<()> {
        let Args {
            path,
            member,
            output,
            bytes: byte_limit,
            members: member_limit,
            names: name_limit,
        } = parse_args(std::env::args_os().skip(1))?;
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
            reader.check(archive_read_open_filename(
                reader.0,
                path.as_ptr(),
                64 * 1024,
            ))?;
        }
        let mut entries = Vec::new();
        let mut index = 0u64;
        let mut names = 0usize;
        loop {
            let mut entry = std::ptr::null_mut();
            let status = unsafe { archive_read_next_header(reader.0, &raw mut entry) };
            if status == 1 {
                break;
            }
            reader.check(status)?;
            anyhow::ensure!(!entry.is_null(), "Missing archive header");
            if member == Some(index) {
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
                    anyhow::ensure!(n >= 0, "Archive decompression failed");
                    if n == 0 {
                        break;
                    }
                    let n = usize::try_from(n)?;
                    total += n;
                    anyhow::ensure!(
                        total <= limit,
                        "Member preview exceeds {} limit",
                        super::describe(limit)
                    );
                    file.write_all(&buf[..n])?;
                }
                println!("{{\"bytes\":{total}}}");
                return Ok(());
            }
            if member.is_none() {
                let mut name = unsafe { archive_entry_pathname_utf8(entry) };
                if name.is_null() {
                    name = unsafe { archive_entry_pathname(entry) };
                }
                anyhow::ensure!(!name.is_null(), "Archive member has no name");
                let name = unsafe { CStr::from_ptr(name) }.to_string_lossy();
                names += name.len();
                let (member_limit, name_limit) =
                    (member_limit.unwrap_or(0), name_limit.unwrap_or(0));
                anyhow::ensure!(
                    names <= name_limit && entries.len() < member_limit,
                    "Archive listing exceeds preview limit ({member_limit} entries / {} names)",
                    super::describe(name_limit)
                );
                entries.push(serde_json::json!({"index":index,"name":name,"size":unsafe{archive_entry_size(entry)},"directory":unsafe{archive_entry_filetype(entry)}==0o040_000,"regular":unsafe{archive_entry_filetype(entry)}==0o100_000,"encrypted":unsafe{archive_entry_is_encrypted(entry)}!=0}));
            }
            index += 1;
        }
        anyhow::ensure!(member.is_none(), "Archive member no longer exists");
        println!("{}", serde_json::to_string(&entries)?);
        Ok(())
    }
}
/// Whole binary units for messages, matching `bibli_platform::inspection::limits::describe`.
fn describe(bytes: usize) -> String {
    if bytes > 0 && bytes.is_multiple_of(1024 * 1024) {
        format!("{} MiB", bytes / (1024 * 1024))
    } else if bytes > 0 && bytes.is_multiple_of(1024) {
        format!("{} KiB", bytes / 1024)
    } else {
        format!("{bytes} bytes")
    }
}
fn main() {
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
        let read = parse_args(args(&["a.zip", "3", "out", "--bytes", "4096"])).unwrap();
        assert_eq!(read.member, Some(3));
        assert_eq!(read.output.as_deref().and_then(|o| o.to_str()), Some("out"));
        assert_eq!(read.bytes, Some(4096));
        assert!(parse_args(args(&["a.zip"])).is_err());
        assert!(parse_args(args(&["a.zip", "3", "out"])).is_err());
        assert!(parse_args(args(&["a.zip", "--members", "10"])).is_err());
        assert!(parse_args(args(&["a.zip", "--bytes"])).is_err());
        assert!(parse_args(args(&[])).is_err());
    }
}
