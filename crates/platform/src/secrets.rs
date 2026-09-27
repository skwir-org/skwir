//! Secrets in the desktop's keyring: the Secret Service (GNOME Keyring,
//! `KeePassXC`, `KWallet`), or the portal's file keyring in a sandbox, through
//! oo7, the client gpui already links. A secret a setting needs (a tile
//! server's API key) is stored here and never in the settings file, which
//! only says it is here (`bibli_core::settings::SECRET_STORED`).
//!
//! Every call blocks until the keyring answers, which may be after the user
//! unlocked it in the keyring's own prompt: call from a worker, never from
//! the UI thread. A secret is never logged, traced or put on a command line;
//! [`SecretText`] has no `Debug`.
use std::collections::HashMap;

/// Where a secret belongs: a field of a record of a setting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SecretRef<'a> {
    pub setting: &'a str,
    pub record: &'a str,
    pub field: &'a str,
}

/// A secret read from the keyring. No `Debug`, no `Display`: it is shown
/// to nobody and only handed to what needs it.
pub struct SecretText(String);

impl SecretText {
    pub fn expose(&self) -> &str {
        &self.0
    }
}

/// The attributes the keyring finds a secret by; without `field`, every
/// secret of a record.
fn attributes<'a>(
    setting: &'a str,
    record: &'a str,
    field: Option<&'a str>,
) -> HashMap<&'a str, &'a str> {
    let mut attributes = HashMap::from([
        ("application", "bibli"),
        ("setting", setting),
        ("record", record),
    ]);
    if let Some(field) = field {
        attributes.insert("field", field);
    }
    attributes
}

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
mod backend {
    use super::{SecretRef, SecretText, attributes};
    use futures_lite::future::block_on;

    async fn keyring() -> anyhow::Result<oo7::Keyring> {
        oo7::Keyring::new()
            .await
            .map_err(|error| anyhow::anyhow!("No keyring answered: {error}"))
    }

    pub fn store(at: SecretRef<'_>, label: &str, secret: &str) -> anyhow::Result<()> {
        block_on(async {
            let keyring = keyring().await?;
            keyring.unlock().await?;
            keyring
                .create_item(
                    label,
                    &attributes(at.setting, at.record, Some(at.field)),
                    secret.as_bytes(),
                    true,
                )
                .await?;
            Ok(())
        })
    }

    pub fn lookup(at: SecretRef<'_>) -> anyhow::Result<Option<SecretText>> {
        block_on(async {
            let keyring = keyring().await?;
            let items = keyring
                .search_items(&attributes(at.setting, at.record, Some(at.field)))
                .await?;
            let Some(item) = items.first() else {
                return Ok(None);
            };
            if item.is_locked().await? {
                item.unlock().await?;
            }
            let secret = item.secret().await?;
            let text = String::from_utf8(secret.to_vec())
                .map_err(|_| anyhow::anyhow!("The keyring's secret is not text"))?;
            Ok(Some(SecretText(text)))
        })
    }

    pub fn delete(setting: &str, record: &str, field: Option<&str>) -> anyhow::Result<()> {
        block_on(async {
            let keyring = keyring().await?;
            keyring.delete(&attributes(setting, record, field)).await?;
            Ok(())
        })
    }
}

#[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
mod backend {
    use super::{SecretRef, SecretText};

    const NONE: &str = "No keyring is supported on this system yet";

    pub fn store(_: SecretRef<'_>, _: &str, _: &str) -> anyhow::Result<()> {
        anyhow::bail!(NONE)
    }
    pub fn lookup(_: SecretRef<'_>) -> anyhow::Result<Option<SecretText>> {
        anyhow::bail!(NONE)
    }
    pub fn delete(_: &str, _: &str, _: Option<&str>) -> anyhow::Result<()> {
        anyhow::bail!(NONE)
    }
}

/// Keep `secret` for `at`, replacing what was kept; `label` is what the
/// keyring's own window shows ("bibli: Topo tiles").
pub fn store(at: SecretRef<'_>, label: &str, secret: &str) -> anyhow::Result<()> {
    backend::store(at, label, secret)
}

/// The secret kept for `at`, if any.
pub fn lookup(at: SecretRef<'_>) -> anyhow::Result<Option<SecretText>> {
    backend::lookup(at)
}

/// Forget the secret kept for `at`.
pub fn delete(at: SecretRef<'_>) -> anyhow::Result<()> {
    backend::delete(at.setting, at.record, Some(at.field))
}

/// Forget every secret of a record: it was removed.
pub fn delete_record(setting: &str, record: &str) -> anyhow::Result<()> {
    backend::delete(setting, record, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_secret_is_found_by_its_setting_record_and_field() {
        let one = attributes("map-tiles.servers", "a1", Some("key"));
        assert_eq!(one.get("application"), Some(&"bibli"));
        assert_eq!(one.get("field"), Some(&"key"));
        // A whole record's secrets: the same attributes, any field.
        let all = attributes("map-tiles.servers", "a1", None);
        assert!(!all.contains_key("field"));
        assert_eq!(all.get("record"), Some(&"a1"));
    }

    /// Against a real Secret Service, never the user's: run only inside a
    /// private D-Bus session with a keyring of its own, as
    /// `tools/test/private-keyring.sh` does, which sets the guard below.
    #[test]
    #[ignore = "needs a private Secret Service (tools/test/private-keyring.sh)"]
    fn a_secret_goes_in_and_comes_out_of_a_keyring() {
        assert_eq!(
            std::env::var("BIBLI_PRIVATE_KEYRING").as_deref(),
            Ok("1"),
            "refusing to touch a keyring that is not the test's own"
        );
        let at = SecretRef {
            setting: "test.servers",
            record: "r1",
            field: "key",
        };
        store(at, "bibli test", "s3cret with spaces").unwrap();
        assert_eq!(lookup(at).unwrap().unwrap().expose(), "s3cret with spaces");
        store(at, "bibli test", "replaced").unwrap();
        assert_eq!(lookup(at).unwrap().unwrap().expose(), "replaced");
        delete_record("test.servers", "r1").unwrap();
        assert!(lookup(at).unwrap().is_none());
    }
}
