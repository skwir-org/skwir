//! What a plugin from outside may reach while it answers one call: the
//! files of that call, by handle, read only with the `read` capability;
//! and how its answer reaches the host ([`Transport`]).

use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use bibli_core::CancelToken;
use bibli_core::permissions::{Capabilities, Capability};
use bibli_plugin_api::protocol::{File, READ_LIMIT};
use serde_json::Value;

/// How a plugin's answers reach bibli: a program's pipe, a module's call.
pub(crate) trait Transport: Send + Sync {
    /// Ask `method` with `params`, the plugin reading through `call`.
    fn call(&self, method: &str, params: Value, call: &Call) -> anyhow::Result<Value>;
    /// Let go of what runs it: the program ends, the module is dropped.
    fn stop(&self);
}

/// One call's files and what the plugin may do with them.
#[derive(Clone)]
pub(crate) struct Call {
    files: Vec<PathBuf>,
    read: bool,
    cancel: CancelToken,
}

impl Call {
    /// A call about `files` (handle 1 is the first), for a plugin granted
    /// `granted`, given up when `cancel` fires.
    pub fn new(files: Vec<PathBuf>, granted: Capabilities, cancel: CancelToken) -> Call {
        Call {
            files,
            read: granted.has(Capability::Read),
            cancel,
        }
    }

    /// The `n`-th file (from 0) as the plugin is told of it.
    pub fn describe(&self, n: usize, mime: Option<String>) -> File {
        let path = &self.files[n];
        File {
            handle: u32::try_from(n + 1).unwrap_or(u32::MAX),
            name: path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default(),
            path: path.to_string_lossy().into_owned(),
            mime,
            size: std::fs::metadata(path).ok().map(|meta| meta.len()),
        }
    }

    pub fn path(&self, handle: u32) -> Option<&Path> {
        let n = usize::try_from(handle).ok()?.checked_sub(1)?;
        self.files.get(n).map(PathBuf::as_path)
    }

    /// What gives the call up.
    pub fn token(&self) -> &CancelToken {
        &self.cancel
    }

    pub fn cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }

    /// The bytes a plugin asks for, `length` at most (bounded) from
    /// `offset`, of a file of the call; refused for a plugin that may
    /// not read, a handle of no file of the call, a call given up, or a
    /// file that is no regular file.
    pub fn read(&self, handle: u32, offset: u64, length: u32) -> Result<Vec<u8>, String> {
        if !self.read {
            return Err("this plugin may not read what files hold".into());
        }
        if self.cancelled() {
            return Err("cancelled".into());
        }
        let path = self
            .path(handle)
            .ok_or_else(|| format!("no file {handle} in this call"))?;
        let mut file =
            bibli_core::fs::file::open_regular(path).map_err(|error| error.to_string())?;
        file.seek(SeekFrom::Start(offset))
            .map_err(|error| error.to_string())?;
        let mut bytes = Vec::new();
        file.take(u64::from(length.min(READ_LIMIT)))
            .read_to_end(&mut bytes)
            .map_err(|error| error.to_string())?;
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_call_reads_its_own_files_and_only_with_leave() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("bytes.bin");
        std::fs::write(&file, b"0123456789").unwrap();
        let readable = Capabilities::of(&[Capability::Read]);
        let call = Call::new(vec![file.clone()], readable, CancelToken::new());
        assert_eq!(call.read(1, 2, 3), Ok(b"234".to_vec()));
        assert_eq!(call.read(1, 8, 100), Ok(b"89".to_vec()), "fewer at the end");
        assert!(call.read(2, 0, 1).is_err(), "no such file in the call");
        assert!(call.read(0, 0, 1).is_err());
        let described = call.describe(0, Some("application/octet-stream".into()));
        assert_eq!((described.handle, described.size), (1, Some(10)));
        let blind = Call::new(vec![file], Capabilities::NONE, CancelToken::new());
        assert!(blind.read(1, 0, 1).unwrap_err().contains("may not read"));
        let cancel = CancelToken::new();
        let given_up = Call::new(vec![tmp.path().join("bytes.bin")], readable, cancel.clone());
        cancel.cancel();
        assert_eq!(given_up.read(1, 0, 1), Err("cancelled".into()));
    }
}
