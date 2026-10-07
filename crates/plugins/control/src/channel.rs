//! Where the window listens, and how one reaches it (ADR-043): a Unix
//! socket in a folder of the user's alone, or on Windows a named pipe in
//! the user's name. One bibli listens; a second finds it taken.

use std::path::PathBuf;

/// The channel's address: a socket's path, or a pipe's name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Address(pub PathBuf);

/// Where the user's bibli listens: `$XDG_RUNTIME_DIR/bibli/control.sock`
/// (Linux), the user's own `$TMPDIR` on macOS, else a folder of the user's
/// in the temporary folder; `\\.\pipe\bibli-control-<user>` on Windows.
pub fn address() -> Option<Address> {
    #[cfg(unix)]
    {
        let folder = match bibli_core::xdg::runtime_dir() {
            Some(runtime) => runtime.join("bibli"),
            None if cfg!(target_os = "macos") => std::env::temp_dir().join("bibli"),
            None => {
                std::env::temp_dir().join(format!("bibli-{}", rustix::process::getuid().as_raw()))
            }
        };
        Some(Address(folder.join("control.sock")))
    }
    #[cfg(windows)]
    {
        let user = std::env::var("USERNAME").ok()?;
        let user: String = user
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
            .collect();
        Some(Address(PathBuf::from(format!(
            r"\\.\pipe\bibli-control-{user}"
        ))))
    }
}

/// Why no window answers.
#[derive(Debug)]
pub struct NotListening;

impl std::fmt::Display for NotListening {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(
            "bibli is not open, or Agents and scripts is off: open bibli and turn it on in Settings, Plugins",
        )
    }
}

impl std::error::Error for NotListening {}

#[cfg(unix)]
mod imp {
    use std::os::unix::fs::{DirBuilderExt as _, MetadataExt as _, PermissionsExt as _};
    use std::path::Path;

    use tokio::net::{UnixListener, UnixStream};

    use super::{Address, NotListening};

    pub type Stream = UnixStream;

    /// The listening socket, removed with it.
    pub struct Listener {
        listener: UnixListener,
        path: std::path::PathBuf,
    }

    impl Listener {
        pub async fn accept(&mut self) -> std::io::Result<Stream> {
            self.listener.accept().await.map(|(stream, _)| stream)
        }
    }

    impl Drop for Listener {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.path);
        }
    }

    /// Listen at `address`, in a folder that is the user's alone. `None`
    /// when another bibli listens there already. A socket left by a bibli
    /// that ended without removing it is replaced.
    pub fn listen(address: &Address) -> anyhow::Result<Option<Listener>> {
        let path = &address.0;
        let folder = path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("the channel's folder"))?;
        own_folder(folder)?;
        if path.exists() {
            match std::os::unix::net::UnixStream::connect(path) {
                Ok(_) => return Ok(None),
                Err(_) => std::fs::remove_file(path)?,
            }
        }
        let listener = UnixListener::bind(path)?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        Ok(Some(Listener {
            listener,
            path: path.clone(),
        }))
    }

    /// `folder`, made if missing, and refused unless it is the user's alone
    /// (owned by the user, open to nobody else).
    // A permission mask reads as one: no group or other bits.
    #[allow(clippy::verbose_bit_mask)]
    fn own_folder(folder: &Path) -> anyhow::Result<()> {
        if !folder.exists() {
            std::fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(folder)?;
        }
        let metadata = std::fs::symlink_metadata(folder)?;
        let uid = rustix::process::getuid().as_raw();
        anyhow::ensure!(
            metadata.is_dir() && metadata.uid() == uid && metadata.mode() & 0o077 == 0,
            "the channel's folder is not the user's alone"
        );
        Ok(())
    }

    pub async fn connect(address: &Address) -> Result<Stream, NotListening> {
        UnixStream::connect(&address.0)
            .await
            .map_err(|_| NotListening)
    }
}

#[cfg(windows)]
mod imp {
    use tokio::net::windows::named_pipe::{
        ClientOptions, NamedPipeClient, NamedPipeServer, ServerOptions,
    };

    use super::{Address, NotListening};

    pub type Stream = NamedPipeServer;

    /// The pipe's next instance, waiting for a client.
    pub struct Listener {
        name: std::ffi::OsString,
        next: NamedPipeServer,
    }

    impl Listener {
        pub async fn accept(&mut self) -> std::io::Result<Stream> {
            self.next.connect().await?;
            let next = ServerOptions::new()
                .reject_remote_clients(true)
                .create(&self.name)?;
            Ok(std::mem::replace(&mut self.next, next))
        }
    }

    /// Listen at `address`. `None` when another bibli has the pipe.
    pub fn listen(address: &Address) -> anyhow::Result<Option<Listener>> {
        let name = address.0.clone().into_os_string();
        match ServerOptions::new()
            .first_pipe_instance(true)
            .reject_remote_clients(true)
            .create(&name)
        {
            Ok(next) => Ok(Some(Listener { name, next })),
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    pub async fn connect(address: &Address) -> Result<NamedPipeClient, NotListening> {
        ClientOptions::new()
            .open(address.0.as_os_str())
            .map_err(|_| NotListening)
    }
}

pub use imp::{Listener, connect, listen};
