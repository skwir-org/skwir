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
    /// address of Skwir's (a Google account). A device's is GVfs's name for
    /// it (ADR-096): what its volume monitor offers it at, and what lets
    /// go of it.
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
            "mtp" => (Scheme::Mtp, self.get("host")?, "/".to_owned()),
            "gphoto2" => (Scheme::Gphoto2, self.get("host")?, "/".to_owned()),
            "afc" => (Scheme::Afc, self.get("host")?, "/".to_owned()),
            _ => return None,
        };
        if scheme.device().is_some() {
            // Read as its address would be: a name GVfs gave that is no
            // device's as Skwir reads one (an option, a user) has none.
            let mut text = format!("{}://{}", scheme.name(), escaped_host(host));
            if let Some(port) = port {
                use std::fmt::Write as _;
                let _ = write!(text, ":{port}");
            }
            text.push('/');
            return Address::parse(&text).ok();
        }
        // Read back as its address, written: what a folder's name holds
        // goes through the checks of an address typed (its host, its user,
        // no `.` or `..` in its path), as a device's does.
        let built = Address {
            scheme,
            user,
            domain: self.get("domain").map(ToOwned::to_owned),
            host: host.to_owned(),
            port,
            path: path.into_bytes(),
        };
        Address::parse(&built.to_string()).ok()
    }

    /// The device on this machine it is, if one: a phone (MTP, Apple's
    /// AFC) or a camera (gPhoto2), whether or not its name reads as an
    /// address.
    pub fn device(&self) -> Option<crate::connector::Device> {
        use crate::connector::Device;
        match self.kind.as_str() {
            "mtp" | "afc" => Some(Device::Phone),
            "gphoto2" => Some(Device::Camera),
            _ => None,
        }
    }

    /// What the user calls this mount: its address's name, else what GVfs
    /// says it is (`Phone (MTP)`). A device's volume says better, where
    /// its monitor offers it (`Remotes::label_of`).
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

/// A host as an address's authority holds it: what would end the
/// authority `%XX`-escaped, a port's brackets kept.
fn escaped_host(host: &str) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(host.len());
    for b in host.bytes() {
        if b.is_ascii_alphanumeric()
            || b"-._+[],".contains(&b)
            || (b == b':' && host.starts_with('['))
        {
            out.push(char::from(b));
        } else {
            let _ = write!(out, "%{b:02X}");
        }
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
    use crate::connector::Device;

    #[test]
    fn a_gvfs_folder_name_is_an_address() {
        let spec = Spec::parse("sftp:host=example.com,port=2222,user=me").unwrap();
        assert_eq!(
            spec.address().unwrap().to_string(),
            "sftp://me@example.com:2222/"
        );
        assert_eq!(
            spec.label(),
            "me@example.com:2222",
            "its port not the usual one"
        );

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

    /// A device GVfs reads is named by its address (ADR-096): what its
    /// monitor offers it at, and what lets go of it.
    #[test]
    fn a_device_s_folder_name_is_its_address() {
        let phone = Spec::parse("mtp:host=Example_Phone_0123").unwrap();
        assert_eq!(
            phone.address().unwrap().to_string(),
            "mtp://Example_Phone_0123/"
        );
        assert_eq!(phone.label(), "Example Phone 0123 (MTP)");
        // A phone or a camera is on this machine, never a server.
        assert_eq!(phone.device(), Some(Device::Phone));
        let camera = Spec::parse("gphoto2:host=%5Busb%3A001%2C004%5D").unwrap();
        assert_eq!(
            camera.address().unwrap().to_string(),
            "gphoto2://[usb:001,004]/"
        );
        assert_eq!(camera.device(), Some(Device::Camera));
        let documents = Spec::parse("afc:host=00008030-0123,port=3").unwrap();
        assert_eq!(
            documents.address().unwrap().to_string(),
            "afc://00008030-0123:3/"
        );
        // A name no device's address reads as keeps GVfs's word for it.
        let odd = Spec::parse("mtp:host=-Odd%20Phone").unwrap();
        assert!(odd.address().is_none());
        assert_eq!(odd.label(), "-Odd Phone (MTP)");
        assert_eq!(odd.device(), Some(Device::Phone));
        let share = Spec::parse("smb-share:server=nas,share=photos").unwrap();
        assert_eq!(share.device(), None);
        let drive = Spec::parse("google-drive:host=example.com,user=me").unwrap();
        assert!(drive.address().is_none());
        assert_eq!(drive.label(), "me@example.com");
        assert!(Spec::parse("not a spec").is_none());
        assert!(Spec::parse("sftp:host").is_none(), "a key without a value");
    }
}
