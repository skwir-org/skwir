//! GVfs's names for its mounts: each is a folder of its FUSE bridge
//! (`/run/user/1000/gvfs/sftp:host=example.com,user=me`), named by the
//! mount's spec, a type and its keys, each value `%XX`-escaped as GVfs
//! writes it (`g_mount_spec_to_string`). Read here into an address, and
//! back, without asking GVfs.

use crate::address::{Address, Scheme, decoded};

/// A mount spec read from its folder's name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Spec {
    pub kind: String,
    pub keys: Vec<(String, String)>,
}

impl Spec {
    /// `type:key=value,key=value`; `None` for a name that is no spec.
    pub fn parse(name: &str) -> Option<Spec> {
        let (kind, rest) = name.split_once(':')?;
        if kind.is_empty() {
            return None;
        }
        let mut keys = Vec::new();
        for pair in rest.split(',').filter(|p| !p.is_empty()) {
            let (key, value) = pair.split_once('=')?;
            keys.push((
                key.to_owned(),
                String::from_utf8_lossy(&decoded(value)).into_owned(),
            ));
        }
        Some(Spec {
            kind: kind.to_owned(),
            keys,
        })
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.keys
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    /// The address this mount is of, at its top; `None` for what has no
    /// address of Skwir's (a phone over MTP, a Google account).
    pub fn address(&self) -> Option<Address> {
        let port = self.get("port").and_then(|p| p.parse().ok());
        let user = self.get("user").map(ToOwned::to_owned);
        let (scheme, host, path) = match self.kind.as_str() {
            "sftp" => (Scheme::Sftp, self.get("host")?, "/".to_owned()),
            "ftp" => (Scheme::Ftp, self.get("host")?, "/".to_owned()),
            "ftps" => (Scheme::Ftps, self.get("host")?, "/".to_owned()),
            "smb-share" => (
                Scheme::Smb,
                self.get("server")?,
                format!("/{}/", self.get("share")?),
            ),
            "dav" => {
                let scheme = if self.get("ssl") == Some("true") {
                    Scheme::Davs
                } else {
                    Scheme::Dav
                };
                let prefix = self.get("prefix").unwrap_or("/");
                (scheme, self.get("host")?, slashed(prefix))
            }
            "nfs" => (
                Scheme::Nfs,
                self.get("host")?,
                slashed(self.get("prefix").unwrap_or("/")),
            ),
            "afp-volume" => (
                Scheme::Afp,
                self.get("host")?,
                format!("/{}/", self.get("volume")?),
            ),
            _ => return None,
        };
        Some(Address {
            scheme,
            user,
            domain: self.get("domain").map(ToOwned::to_owned),
            host: host.to_owned(),
            port,
            path: path.into_bytes(),
        })
    }

    /// What the user calls this mount: its address's name, else what GVfs
    /// says it is (`Phone (MTP)`).
    pub fn label(&self) -> String {
        if let Some(address) = self.address() {
            return address.label();
        }
        let host = self.get("host").unwrap_or("");
        match self.kind.as_str() {
            "mtp" => format!("{} (MTP)", device_name(host)),
            "gphoto2" => format!("{} (camera)", device_name(host)),
            "afc" => format!("{} (iOS)", device_name(host)),
            "google-drive" => match self.get("user") {
                Some(user) => format!("{user}@{host}"),
                None => host.to_owned(),
            },
            other => match host {
                "" => other.to_owned(),
                host => format!("{host} ({other})"),
            },
        }
    }
}

/// A path that starts and ends with `/`, as a mount's top is written.
fn slashed(path: &str) -> String {
    let mut out = String::with_capacity(path.len() + 2);
    if !path.starts_with('/') {
        out.push('/');
    }
    out.push_str(path);
    if !out.ends_with('/') {
        out.push('/');
    }
    out
}

/// A device GVfs names by its bus (`[usb:003,012]`) or its serial: the
/// part a person would recognise.
fn device_name(host: &str) -> String {
    let name = host.trim_start_matches('[').trim_end_matches(']');
    name.replace('_', " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_gvfs_folder_name_is_an_address() {
        let spec = Spec::parse("sftp:host=example.com,port=2222,user=me").unwrap();
        assert_eq!(
            spec.address().unwrap().to_string(),
            "sftp://me@example.com:2222/"
        );
        assert_eq!(spec.label(), "me@example.com");

        let share = Spec::parse("smb-share:domain=WORK,server=nas,share=photos,user=me").unwrap();
        assert_eq!(
            share.address().unwrap().to_string(),
            "smb://WORK;me@nas/photos/"
        );
        assert_eq!(share.label(), "photos on nas");

        let dav = Spec::parse(
            "dav:host=cloud.example.com,ssl=true,user=me,prefix=%2Fremote.php%2Fwebdav",
        )
        .unwrap();
        assert_eq!(
            dav.address().unwrap().to_string(),
            "davs://me@cloud.example.com/remote.php/webdav/"
        );
    }

    #[test]
    fn what_has_no_address_keeps_a_name() {
        let phone = Spec::parse("mtp:host=Example_Phone_0123").unwrap();
        assert!(phone.address().is_none());
        assert_eq!(phone.label(), "Example Phone 0123 (MTP)");
        assert!(Spec::parse("not a spec").is_none());
        assert!(Spec::parse("sftp:host").is_none(), "a key without a value");
    }
}
