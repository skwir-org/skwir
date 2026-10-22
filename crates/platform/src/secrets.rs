//! Secrets in the desktop's keyring: the Secret Service (GNOME Keyring,
//! `KeePassXC`, `KWallet`), or the portal's file keyring in a sandbox, through
//! oo7, the client gpui already links. A secret a setting needs (a tile
//! server's API key) is stored here and never in the settings file, which
//! only says it is here (`skwir_core::settings::SECRET_STORED`).
//!
//! Every call blocks until the keyring answers, which may be after the user
//! unlocked it in the keyring's own prompt: call from a worker, never from
//! the UI thread. A secret is never logged, traced or put on a command line;
//! [`SecretText`] has no `Debug`.
#[cfg(any(target_os = "linux", target_os = "freebsd", test))]
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
#[cfg(any(target_os = "linux", target_os = "freebsd", test))]
fn attributes<'a>(
    setting: &'a str,
    record: &'a str,
    field: Option<&'a str>,
) -> HashMap<&'a str, &'a str> {
    let mut attributes = HashMap::from([
        ("application", "skwir"),
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
    use std::collections::HashMap;

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

    pub fn lookup_network(named: &[(&str, &str)]) -> anyhow::Result<Option<SecretText>> {
        block_on(async {
            let keyring = keyring().await?;
            let wanted: HashMap<&str, &str> = named.iter().copied().collect();
            let mut found = Vec::new();
            for item in keyring.search_items(&wanted).await? {
                let count = item.attributes().await.map_or(usize::MAX, |a| a.len());
                found.push((count, item));
            }
            // The least specific first, as GVfs takes it: `ftp://host`'s
            // before `ftp://host:2121`'s for an address that names no port.
            found.sort_by_key(|(count, _)| *count);
            let Some((_, item)) = found.into_iter().next() else {
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
}

/// Windows: the Credential Manager's generic credentials, one per field,
/// named `skwir/<setting>/<record>/<field>` (run under Wine's).
#[cfg(windows)]
mod backend {
    use super::{SecretRef, SecretText};
    use crate::windows::credentials;

    fn name(setting: &str, record: &str, field: &str) -> String {
        format!("skwir/{setting}/{record}/{field}")
    }

    pub fn store(at: SecretRef<'_>, label: &str, secret: &str) -> anyhow::Result<()> {
        credentials::write(
            &name(at.setting, at.record, at.field),
            label,
            secret.as_bytes(),
        )?;
        Ok(())
    }

    pub fn lookup(at: SecretRef<'_>) -> anyhow::Result<Option<SecretText>> {
        let Some(bytes) = credentials::read(&name(at.setting, at.record, at.field))? else {
            return Ok(None);
        };
        let text = String::from_utf8(bytes)
            .map_err(|_| anyhow::anyhow!("The keyring's secret is not text"))?;
        Ok(Some(SecretText(text)))
    }

    pub fn delete(setting: &str, record: &str, field: Option<&str>) -> anyhow::Result<()> {
        credentials::delete(&name(setting, record, field.unwrap_or("*")))?;
        Ok(())
    }
}

/// macOS: the Keychain's generic passwords, service `skwir`, one per
/// field, its account `<setting>/<record>/<field>`. Compiled, never run.
#[cfg(target_os = "macos")]
mod backend {
    use security_framework::item::{ItemClass, ItemSearchOptions, Limit};
    use security_framework::passwords;

    use super::{SecretRef, SecretText};

    const SERVICE: &str = "skwir";
    /// `errSecItemNotFound`.
    const NOT_FOUND: i32 = -25300;

    fn account(setting: &str, record: &str, field: &str) -> String {
        format!("{setting}/{record}/{field}")
    }

    pub fn store(at: SecretRef<'_>, _label: &str, secret: &str) -> anyhow::Result<()> {
        passwords::set_generic_password(
            SERVICE,
            &account(at.setting, at.record, at.field),
            secret.as_bytes(),
        )?;
        Ok(())
    }

    pub fn lookup(at: SecretRef<'_>) -> anyhow::Result<Option<SecretText>> {
        match passwords::get_generic_password(SERVICE, &account(at.setting, at.record, at.field)) {
            Ok(bytes) => {
                let text = String::from_utf8(bytes)
                    .map_err(|_| anyhow::anyhow!("The keyring's secret is not text"))?;
                Ok(Some(SecretText(text)))
            }
            Err(e) if e.code() == NOT_FOUND => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    fn forget(name: &str) -> anyhow::Result<()> {
        match passwords::delete_generic_password(SERVICE, name) {
            Err(e) if e.code() != NOT_FOUND => Err(e.into()),
            _ => Ok(()),
        }
    }

    pub fn delete(setting: &str, record: &str, field: Option<&str>) -> anyhow::Result<()> {
        if let Some(field) = field {
            return forget(&account(setting, record, field));
        }
        let prefix = account(setting, record, "");
        let found = ItemSearchOptions::new()
            .class(ItemClass::generic_password())
            .service(SERVICE)
            .load_attributes(true)
            .limit(Limit::All)
            .search();
        let results = match found {
            Ok(results) => results,
            Err(e) if e.code() == NOT_FOUND => return Ok(()),
            Err(e) => return Err(e.into()),
        };
        for result in results {
            if let Some(name) = result
                .simplify_dict()
                .and_then(|attributes| attributes.get("acct").cloned())
                .filter(|name| name.starts_with(&prefix))
            {
                forget(&name)?;
            }
        }
        Ok(())
    }
}

#[cfg(not(any(
    target_os = "linux",
    target_os = "freebsd",
    windows,
    target_os = "macos"
)))]
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
/// keyring's own window shows ("skwir: Topo tiles").
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

/// A password the desktop keeps for a server under libsecret's network
/// schema (`org.gnome.keyring.NetworkPassword`: what GVfs keeps when
/// Nautilus is told to remember one), found by `named` (`protocol`,
/// `server`, `user`, `domain`, `port`): the least specific of those that
/// hold them all. Read only: Skwir never changes another program's.
/// Linux and FreeBSD's Secret Service; elsewhere none.
pub fn lookup_network(named: &[(&str, &str)]) -> anyhow::Result<Option<SecretText>> {
    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    {
        backend::lookup_network(named)
    }
    #[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
    {
        let _ = named;
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_secret_is_found_by_its_setting_record_and_field() {
        let one = attributes("map-tiles.servers", "a1", Some("key"));
        assert_eq!(one.get("application"), Some(&"skwir"));
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
            std::env::var("SKWIR_PRIVATE_KEYRING").as_deref(),
            Ok("1"),
            "refusing to touch a keyring that is not the test's own"
        );
        let at = SecretRef {
            setting: "test.servers",
            record: "r1",
            field: "key",
        };
        store(at, "skwir test", "s3cret with spaces").unwrap();
        assert_eq!(lookup(at).unwrap().unwrap().expose(), "s3cret with spaces");
        store(at, "skwir test", "replaced").unwrap();
        assert_eq!(lookup(at).unwrap().unwrap().expose(), "replaced");
        delete_record("test.servers", "r1").unwrap();
        assert!(lookup(at).unwrap().is_none());
    }

    /// A password GVfs kept (libsecret's network schema) is found by its
    /// server, the least specific first; written here as GVfs writes it.
    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    #[test]
    #[ignore = "needs a private Secret Service (tools/test/private-keyring.sh)"]
    fn a_password_gvfs_kept_is_found_by_its_server() {
        assert_eq!(
            std::env::var("SKWIR_PRIVATE_KEYRING").as_deref(),
            Ok("1"),
            "refusing to touch a keyring that is not the test's own"
        );
        let keep = |named: &[(&str, &str)], secret: &str| {
            futures_lite::future::block_on(async {
                let keyring = oo7::Keyring::new().await.unwrap();
                let mut all: HashMap<&str, &str> = named.iter().copied().collect();
                all.insert("xdg:schema", "org.gnome.keyring.NetworkPassword");
                // Not replacing: an item with more attributes matches the
                // fewer, and would go.
                keyring
                    .create_item("me@nas", &all, secret.as_bytes(), false)
                    .await
                    .unwrap();
            });
        };
        let nas = [("protocol", "smb"), ("server", "nas.test"), ("user", "me")];
        keep(
            &[nas.as_slice(), &[("port", "4450")]].concat(),
            "on its port",
        );
        keep(&nas, "the server's");
        assert_eq!(
            lookup_network(&nas).unwrap().unwrap().expose(),
            "the server's",
            "the least specific"
        );
        assert_eq!(
            lookup_network(&[nas.as_slice(), &[("port", "4450")]].concat())
                .unwrap()
                .unwrap()
                .expose(),
            "on its port"
        );
        assert!(
            lookup_network(&[("protocol", "smb"), ("server", "nas.test"), ("user", "you")])
                .unwrap()
                .is_none(),
            "another user's is not"
        );
    }
}
