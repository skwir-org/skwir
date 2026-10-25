//! An encrypted volume's passphrase kept in the desktop's keyring, when
//! the user asked (ADR-088). Opening a locked volume tries the keyring
//! first: Skwir's own passphrase for it, then (Linux) one GVfs kept when
//! Nautilus was told to remember it, read only; the window is asked where
//! none is kept or the one given was refused, offering to remember the
//! answer, which is kept once it unlocked the volume. A volume is known by
//! its container's UUID, as GVfs knows it: a drive that only claims the
//! UUID gets a passphrase that fails here, on this machine.
//!
//! Where the system asks itself and remembers (Windows' BitLocker), none
//! of this runs: `skwir_platform::places::SYSTEM_ASKS`.

use std::sync::Arc;

use skwir_core::app::{App, NotifyLevel, SecretAnswer};
use skwir_core::cancel::CancelToken;

/// Where Skwir's own passphrases are in the keyring
/// (`skwir_platform::secrets`): one record per volume, by its UUID.
const SETTING: &str = "places.passphrases";
const FIELD: &str = "passphrase";

/// The volumes whose passphrase Skwir keeps in the keyring, by their
/// container's UUID: never a passphrase (ADR-088).
pub const REMEMBERED: &str = "places.remembered";

/// How many passphrases the user may type that are refused before the
/// volume is left locked.
const TYPED: usize = 3;

/// Under Remember, while it is on: what remembering the passphrase means.
const KEEPING: &str = "Once remembered, anyone who can open your keyring can unlock this volume.";

/// The keyring as unlocking reads and writes it; a test's stands in.
pub trait Keyring: Send + Sync {
    /// Skwir's own passphrase for the volume `uuid`.
    fn own(&self, uuid: &str) -> Option<String>;
    /// One the desktop keeps for it: GVfs's, Nautilus's.
    fn desktop(&self, uuid: &str) -> Option<String>;
    /// Keep `passphrase` as Skwir's own for `uuid`, `label` what the
    /// keyring's own window shows.
    ///
    /// # Errors
    /// When no keyring answered, or it refused.
    fn keep(&self, uuid: &str, label: &str, passphrase: &str) -> anyhow::Result<()>;
    /// Forget Skwir's own passphrase for `uuid`.
    ///
    /// # Errors
    /// When no keyring answered, or it refused.
    fn forget(&self, uuid: &str) -> anyhow::Result<()>;
}

/// The desktop's keyring (`skwir_platform::secrets`): the Secret Service,
/// the Keychain. Each call blocks until it answers (an unlock it asks for
/// included): on a worker, through [`skwir_platform::secrets::heard`].
pub struct Desktop;

impl Keyring for Desktop {
    fn own(&self, uuid: &str) -> Option<String> {
        match skwir_platform::secrets::lookup(at(uuid)) {
            Ok(found) => found.map(|secret| secret.expose().to_owned()),
            Err(error) => {
                tracing::debug!("the keyring was not read: {error:#}");
                None
            }
        }
    }

    fn desktop(&self, uuid: &str) -> Option<String> {
        match skwir_platform::secrets::lookup_volume(uuid) {
            Ok(found) => found.map(|secret| secret.expose().to_owned()),
            Err(error) => {
                tracing::debug!("the keyring was not read: {error:#}");
                None
            }
        }
    }

    fn keep(&self, uuid: &str, label: &str, passphrase: &str) -> anyhow::Result<()> {
        skwir_platform::secrets::store(at(uuid), label, passphrase)
    }

    fn forget(&self, uuid: &str) -> anyhow::Result<()> {
        skwir_platform::secrets::delete(at(uuid))
    }
}

/// Skwir's own record of the volume `uuid`.
fn at(uuid: &str) -> skwir_platform::secrets::SecretRef<'_> {
    skwir_platform::secrets::SecretRef {
        setting: SETTING,
        record: uuid,
        field: FIELD,
    }
}

/// The volumes whose passphrase Skwir keeps, by UUID.
pub fn remembered(app: &App) -> Vec<String> {
    app.settings.get(REMEMBERED).unwrap_or_default()
}

/// Note in the plugin's data that Skwir keeps a passphrase for `uuid`
/// (`kept`), or no longer: its UUID alone.
pub fn note(app: &App, uuid: &str, kept: bool) {
    let mut remembered = remembered(app);
    if kept == remembered.iter().any(|r| r == uuid) {
        return;
    }
    if kept {
        remembered.push(uuid.to_owned());
    } else {
        remembered.retain(|r| r != uuid);
    }
    app.settings.set(
        REMEMBERED,
        toml::Value::Array(remembered.into_iter().map(toml::Value::String).collect()),
    );
    if let Err(error) = app.settings.save() {
        tracing::warn!("which volumes have a kept passphrase was not saved: {error:#}");
    }
}

/// The locked volume to open: its device, what the question calls it,
/// and its UUID where known (what its passphrase is kept by).
pub struct Locked<'a> {
    pub device: &'a str,
    pub title: &'a str,
    pub uuid: Option<&'a str>,
}

/// What gave the passphrase that unlocked a volume, past Skwir's own.
enum Given {
    Desktop,
    Typed(SecretAnswer),
}

/// Unlock `volume` with the passphrase the keyring keeps for it, else one
/// the user types, asked again while refused ([`TYPED`] times at most),
/// and keep or forget Skwir's own as the module says. `unlock` is the
/// system's (`skwir_platform::places::unlock`, a test's): it answers the
/// cleartext device. `None` when the user cancelled or `cancel` came.
///
/// # Errors
/// What `unlock` answered other than a refusal, or the last refusal.
pub fn unlock_kept_or_asked(
    app: &App,
    keyring: &Arc<dyn Keyring>,
    volume: &Locked<'_>,
    cancel: &CancelToken,
    unlock: impl Fn(&str, &str) -> anyhow::Result<String>,
) -> anyhow::Result<Option<String>> {
    use skwir_platform::places::is_refused;
    let ask_keyring = |ask: fn(&dyn Keyring, &str) -> Option<String>| {
        let (keyring, uuid) = (Arc::clone(keyring), volume.uuid?.to_owned());
        skwir_platform::secrets::heard(cancel, move || ask(&*keyring, &uuid)).flatten()
    };
    // Skwir's own only where it noted one: a volume never remembered
    // does not wake a locked keyring.
    let noted = volume
        .uuid
        .is_some_and(|uuid| remembered(app).iter().any(|r| r == uuid));
    let mut own_refused = false;
    if let Some(own) = noted.then(|| ask_keyring(|k, uuid| k.own(uuid))).flatten() {
        match unlock(volume.device, &own) {
            Ok(cleartext) => return Ok(Some(cleartext)),
            Err(error) if is_refused(&error) => own_refused = true,
            Err(error) => return Err(error),
        }
    }
    let from_desktop = match ask_keyring(|k, uuid| k.desktop(uuid)) {
        Some(desktop) => match unlock(volume.device, &desktop) {
            Ok(cleartext) => Some(cleartext),
            Err(error) if is_refused(&error) => None,
            Err(error) => return Err(error),
        },
        None => None,
    };
    let (cleartext, given) = if let Some(cleartext) = from_desktop {
        (cleartext, Given::Desktop)
    } else {
        let (mut refused, mut typed) = (own_refused, 0);
        loop {
            let Some(answer) = asked(app, volume, refused, cancel) else {
                return Ok(None);
            };
            match unlock(volume.device, &answer.text) {
                Ok(cleartext) => break (cleartext, Given::Typed(answer)),
                Err(error) if is_refused(&error) => {
                    typed += 1;
                    if typed == TYPED {
                        return Err(error);
                    }
                    refused = true;
                }
                Err(error) => return Err(error),
            }
        }
    };
    settle(app, keyring, volume, given, own_refused, cancel);
    Ok(Some(cleartext))
}

/// Keep what the user asked to be remembered, which replaces Skwir's own;
/// forget Skwir's own where it was refused and another served.
fn settle(
    app: &App,
    keyring: &Arc<dyn Keyring>,
    volume: &Locked<'_>,
    given: Given,
    own_refused: bool,
    cancel: &CancelToken,
) {
    let Some(uuid) = volume.uuid else { return };
    let (keyring, owned) = (Arc::clone(keyring), uuid.to_owned());
    match given {
        Given::Typed(answer) if answer.remember => {
            let label = format!("Skwir: {} (encrypted volume)", volume.title);
            let kept = skwir_platform::secrets::heard(cancel, move || {
                keyring.keep(&owned, &label, &answer.text)
            });
            match kept {
                Some(Ok(())) => note(app, uuid, true),
                Some(Err(error)) => app.notify(
                    NotifyLevel::Warning,
                    format!("The passphrase was not remembered: {error:#}"),
                ),
                None => {}
            }
        }
        Given::Typed(_) | Given::Desktop if own_refused => {
            match skwir_platform::secrets::heard(cancel, move || keyring.forget(&owned)) {
                Some(Ok(())) => note(app, uuid, false),
                Some(Err(error)) => {
                    tracing::warn!("a refused passphrase was not forgotten: {error:#}");
                }
                None => {}
            }
        }
        Given::Typed(_) | Given::Desktop => {}
    }
}

/// Ask the window for `volume`'s passphrase, offering Remember where its
/// UUID is known; `refused` says the last one was. `None` for Cancel, no
/// window, or `cancel`.
fn asked(
    app: &App,
    volume: &Locked<'_>,
    refused: bool,
    cancel: &CancelToken,
) -> Option<SecretAnswer> {
    if !app.window_shown() {
        return None;
    }
    let title = if refused {
        format!(
            "Unlock “{}”: that passphrase was not accepted",
            volume.title
        )
    } else {
        format!("Unlock “{}”", volume.title)
    };
    let keeping = volume.uuid.map(|_| KEEPING.to_owned());
    let (id, answer) = app.ask_passphrase(title, "Unlock", keeping);
    let cancelled = cancel.subscribe();
    crossbeam_channel::select! {
        recv(answer) -> answer => answer.ok().flatten(),
        recv(cancelled.receiver()) -> _ => {
            app.withdraw_secret(id);
            None
        }
    }
}
