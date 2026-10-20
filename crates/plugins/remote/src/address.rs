//! A remote's address: `sftp://me@host:2222/home/me`, `smb://nas/photos`,
//! `rclone://gdrive/Photos` (ADR-077). What the user types, bookmarks and
//! keeps; what a connector turns into a mount. Never a password: one
//! written in an address is refused, not kept.

use std::fmt::{self, Write as _};

use skwir_core::location::Location;

/// The protocols an address may name, as a connector maps them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Scheme {
    Sftp,
    Smb,
    Dav,
    Davs,
    Ftp,
    Ftps,
    Nfs,
    Afp,
    /// A remote of rclone's configuration, by its name (`rclone://gdrive/`).
    Rclone,
}

impl Scheme {
    /// Every scheme, each by its own name: what the plugin opens.
    pub const ALL: [Scheme; 9] = [
        Scheme::Sftp,
        Scheme::Smb,
        Scheme::Dav,
        Scheme::Davs,
        Scheme::Ftp,
        Scheme::Ftps,
        Scheme::Nfs,
        Scheme::Afp,
        Scheme::Rclone,
    ];

    /// Names another program gives a scheme here: `ssh://` is SFTP for
    /// GVfs, `cifs://` SMB, `webdav://` WebDAV.
    pub const ALIASES: [(&'static str, Scheme); 4] = [
        ("ssh", Scheme::Sftp),
        ("cifs", Scheme::Smb),
        ("webdav", Scheme::Dav),
        ("webdavs", Scheme::Davs),
    ];

    pub fn name(self) -> &'static str {
        match self {
            Scheme::Sftp => "sftp",
            Scheme::Smb => "smb",
            Scheme::Dav => "dav",
            Scheme::Davs => "davs",
            Scheme::Ftp => "ftp",
            Scheme::Ftps => "ftps",
            Scheme::Nfs => "nfs",
            Scheme::Afp => "afp",
            Scheme::Rclone => "rclone",
        }
    }

    pub fn from_name(name: &str) -> Option<Scheme> {
        let name = name.to_ascii_lowercase();
        Scheme::ALL
            .into_iter()
            .find(|s| s.name() == name)
            .or_else(|| {
                Scheme::ALIASES
                    .into_iter()
                    .find(|(alias, _)| *alias == name)
                    .map(|(_, s)| s)
            })
    }

    /// The protocol in the user's words: "SFTP", "SMB".
    pub fn title(self) -> &'static str {
        match self {
            Scheme::Sftp => "SFTP",
            Scheme::Smb => "SMB",
            Scheme::Dav | Scheme::Davs => "WebDAV",
            Scheme::Ftp | Scheme::Ftps => "FTP",
            Scheme::Nfs => "NFS",
            Scheme::Afp => "AFP",
            Scheme::Rclone => "rclone",
        }
    }
}

/// A parsed address. The path is the server's, its bytes decoded: an SMB
/// address's first part is the share.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Address {
    pub scheme: Scheme,
    /// Who to connect as; `None` for the tool's default (ssh's
    /// configuration, the user's name). For SMB, `domain;user` as GVfs
    /// writes it is split: the domain is [`Address::domain`].
    pub user: Option<String>,
    pub domain: Option<String>,
    /// The server; for rclone, the remote's name in its configuration.
    pub host: String,
    pub port: Option<u16>,
    /// The path on the server, decoded, always starting with `/` (`/` for
    /// the server's top, or a user's home for SFTP as ssh starts there).
    pub path: Vec<u8>,
}

/// Why text is no address.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NotAnAddress(pub String);

impl fmt::Display for NotAnAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for NotAnAddress {}

impl Address {
    /// Read `text`: `scheme://[user@]host[:port][/path]`. The path's
    /// `%XX` are decoded (a space is written as it is or as `%20`).
    ///
    /// # Errors
    /// When the scheme is not one, the host is missing, or the address
    /// holds a password: Skwir keeps none, and the tool that connects asks.
    pub fn parse(text: &str) -> Result<Address, NotAnAddress> {
        let (scheme, rest) = text
            .split_once(':')
            .ok_or_else(|| NotAnAddress(format!("“{text}” names no protocol")))?;
        let scheme = Scheme::from_name(scheme)
            .ok_or_else(|| NotAnAddress(format!("“{scheme}:” is not a protocol Skwir opens")))?;
        Address::parse_rest(scheme, rest)
    }

    /// [`Address::parse`] for what follows the scheme and its colon: a
    /// location's `rest`.
    ///
    /// # Errors
    /// As [`Address::parse`].
    pub fn parse_rest(scheme: Scheme, rest: &str) -> Result<Address, NotAnAddress> {
        let rest = rest.strip_prefix("//").ok_or_else(|| {
            NotAnAddress(format!(
                "“{}:{rest}” needs // before the server",
                scheme.name()
            ))
        })?;
        let (authority, path) = match rest.find('/') {
            Some(at) => (&rest[..at], &rest[at..]),
            None => (rest, "/"),
        };
        let (userinfo, hostport) = match authority.rfind('@') {
            Some(at) => (Some(&authority[..at]), &authority[at + 1..]),
            None => (None, authority),
        };
        let (mut user, mut domain) = (None, None);
        if let Some(info) = userinfo {
            if info.contains(':') {
                return Err(NotAnAddress(
                    "an address holds no password: the server asks for it".to_owned(),
                ));
            }
            let info = decoded_text(info);
            match info.split_once(';') {
                Some((d, u)) if scheme == Scheme::Smb => {
                    domain = (!d.is_empty()).then(|| d.to_owned());
                    user = (!u.is_empty()).then(|| u.to_owned());
                }
                _ => user = (!info.is_empty()).then_some(info),
            }
        }
        let (host, port) = split_port(hostport)?;
        if host.is_empty() {
            return Err(NotAnAddress(format!(
                "“{}://{rest}” names no server",
                scheme.name()
            )));
        }
        let mut path = decoded(path);
        if path.is_empty() {
            path.push(b'/');
        }
        Ok(Address {
            scheme,
            user,
            domain,
            host: host.to_owned(),
            port,
            path,
        })
    }

    /// The address of a location of one of the plugin's schemes.
    ///
    /// # Errors
    /// As [`Address::parse`].
    pub fn of_location(location: &Location) -> Result<Address, NotAnAddress> {
        let scheme = Scheme::from_name(&location.scheme).ok_or_else(|| {
            NotAnAddress(format!(
                "“{}:” is not a protocol Skwir opens",
                location.scheme
            ))
        })?;
        Address::parse_rest(scheme, &location.rest)
    }

    /// This address as a location, which opens it (`sftp:` and the rest).
    pub fn location(&self) -> Location {
        let text = self.to_string();
        let rest = text
            .split_once(':')
            .map_or(text.as_str(), |(_, rest)| rest)
            .to_owned();
        Location::new(self.scheme.name(), rest)
    }

    /// The server's address, at its top: what one mount of it holds,
    /// for the protocols whose mount is of the whole server (SFTP, FTP,
    /// WebDAV, rclone's remote). An SMB share and an NFS export are
    /// mounted one at a time: their first part stays.
    pub fn top(&self) -> Address {
        let mut top = self.clone();
        top.path = match self.scheme {
            Scheme::Smb | Scheme::Afp => {
                let share = self.first_part();
                if share.is_empty() {
                    b"/".to_vec()
                } else {
                    [b"/".as_slice(), share, b"/"].concat()
                }
            }
            _ => b"/".to_vec(),
        };
        top
    }

    /// The first part of the path: an SMB share, an AFP volume.
    pub fn first_part(&self) -> &[u8] {
        let path = self.path.strip_prefix(b"/").unwrap_or(&self.path);
        path.split(|b| *b == b'/').next().unwrap_or_default()
    }

    /// Whether `other` is on the same server, as the same user: a mount of
    /// one holds what the other names, below its own path.
    pub fn same_server(&self, other: &Address) -> bool {
        self.scheme == other.scheme
            && self.host.eq_ignore_ascii_case(&other.host)
            && self.effective_port() == other.effective_port()
            && (self.user.is_none() || other.user.is_none() || self.user == other.user)
            && self.domain == other.domain
    }

    /// Where `other` is below `self`, which a mount holds: the path
    /// relative to it, `Some(b"")` for itself. `None` when `other` is
    /// elsewhere.
    pub fn below<'a>(&self, other: &'a Address) -> Option<&'a [u8]> {
        if !self.same_server(other) {
            return None;
        }
        let base = self.path.strip_suffix(b"/").unwrap_or(&self.path);
        let rest = other.path.strip_prefix(base)?;
        if rest.is_empty() {
            return Some(rest);
        }
        rest.strip_prefix(b"/")
    }

    /// The port when the address names the protocol's usual one too, so
    /// `sftp://h` and `sftp://h:22` are one server.
    pub fn effective_port(&self) -> Option<u16> {
        let usual = match self.scheme {
            Scheme::Sftp => Some(22),
            Scheme::Smb => Some(445),
            Scheme::Dav => Some(80),
            Scheme::Davs => Some(443),
            Scheme::Ftp => Some(21),
            Scheme::Ftps => Some(990),
            Scheme::Nfs => Some(2049),
            Scheme::Afp => Some(548),
            Scheme::Rclone => None,
        };
        self.port.filter(|p| Some(*p) != usual)
    }

    /// What the user calls it: `me@host`, `photos on nas`, `gdrive`.
    pub fn label(&self) -> String {
        let share = String::from_utf8_lossy(self.first_part()).into_owned();
        match self.scheme {
            Scheme::Smb | Scheme::Afp if !share.is_empty() => {
                format!("{share} on {}", self.host)
            }
            Scheme::Nfs if self.path != b"/" => {
                let path = String::from_utf8_lossy(&self.path);
                let name = path.trim_end_matches('/').rsplit('/').next().unwrap_or("");
                format!("{name} on {}", self.host)
            }
            Scheme::Rclone => self.host.clone(),
            _ => match &self.user {
                Some(user) => format!("{user}@{}", self.host),
                None => self.host.clone(),
            },
        }
    }

    /// The address as a URI other programs read (`gio mount`): its path's
    /// bytes `%XX`-escaped but for what a path may hold as it is.
    pub fn uri(&self) -> String {
        let text = self.to_string();
        let start = text.len() - skwir_core::location::escape(&self.path).len();
        let mut uri = text[..start].to_owned();
        for &b in &self.path {
            if b.is_ascii_alphanumeric() || b"-._~/!$&'()*+,;=:@".contains(&b) {
                uri.push(char::from(b));
            } else {
                let _ = write!(uri, "%{b:02X}");
            }
        }
        uri
    }

    /// The host without the brackets an IPv6 literal is written in.
    pub fn bare_host(&self) -> &str {
        self.host
            .strip_prefix('[')
            .and_then(|h| h.strip_suffix(']'))
            .unwrap_or(&self.host)
    }
}

impl fmt::Display for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}://", self.scheme.name())?;
        match (&self.domain, &self.user) {
            (Some(domain), Some(user)) => write!(f, "{};{}@", encoded(domain), encoded(user))?,
            (Some(domain), None) => write!(f, "{};@", encoded(domain))?,
            (None, Some(user)) => write!(f, "{}@", encoded(user))?,
            (None, None) => {}
        }
        f.write_str(&self.host)?;
        if let Some(port) = self.port {
            write!(f, ":{port}")?;
        }
        f.write_str(&skwir_core::location::escape(&self.path))
    }
}

/// `host[:port]`, an IPv6 literal in brackets.
fn split_port(hostport: &str) -> Result<(&str, Option<u16>), NotAnAddress> {
    let (host, port) = if hostport.starts_with('[') {
        match hostport.find(']') {
            Some(end) => (&hostport[..=end], hostport[end + 1..].strip_prefix(':')),
            None => return Err(NotAnAddress(format!("“{hostport}” ends no IPv6 address"))),
        }
    } else {
        match hostport.rsplit_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (hostport, None),
        }
    };
    let port = match port {
        Some("") | None => None,
        Some(port) => Some(
            port.parse::<u16>()
                .map_err(|_| NotAnAddress(format!("“{port}” is no port")))?,
        ),
    };
    Ok((host, port))
}

/// `%XX` decoded, other bytes as they are.
pub fn decoded(text: &str) -> Vec<u8> {
    skwir_core::location::unescape(text)
}

fn decoded_text(text: &str) -> String {
    String::from_utf8_lossy(&decoded(text)).into_owned()
}

/// A user or domain written into an address: what would end it escaped.
fn encoded(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if matches!(c, '@' | ':' | ';' | '/' | '%' | '?' | '#') || c.is_control() {
            let mut buf = [0u8; 4];
            for b in c.encode_utf8(&mut buf).bytes() {
                let _ = write!(out, "%{b:02X}");
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_address_is_read_and_written_back() {
        let a = Address::parse("sftp://me@example.com:2222/home/me/My Files").unwrap();
        assert_eq!(a.scheme, Scheme::Sftp);
        assert_eq!(a.user.as_deref(), Some("me"));
        assert_eq!(a.host, "example.com");
        assert_eq!(a.port, Some(2222));
        assert_eq!(a.path, b"/home/me/My Files");
        assert_eq!(a.to_string(), "sftp://me@example.com:2222/home/me/My Files");
        assert_eq!(Address::parse(&a.to_string()).unwrap(), a);
        let spaced = Address::parse("sftp://example.com/My%20Files").unwrap();
        assert_eq!(spaced.path, b"/My Files", "a bookmark's %20");
    }

    #[test]
    fn other_programs_names_of_a_scheme_are_taken() {
        assert_eq!(Address::parse("ssh://h/").unwrap().scheme, Scheme::Sftp);
        assert_eq!(Address::parse("cifs://h/s").unwrap().scheme, Scheme::Smb);
        assert_eq!(Address::parse("webdavs://h/").unwrap().scheme, Scheme::Davs);
        assert!(Address::parse("http://h/").is_err());
        assert!(Address::parse("sftp:h").is_err(), "no //");
        assert!(Address::parse("sftp:///path").is_err(), "no server");
    }

    #[test]
    fn a_password_in_an_address_is_refused() {
        let error = Address::parse("sftp://me:secret@h/").unwrap_err();
        assert!(!error.0.contains("secret"), "{error}");
    }

    #[test]
    fn an_smb_user_may_say_its_domain() {
        let a = Address::parse("smb://WORK;me@nas/photos/2024").unwrap();
        assert_eq!(
            (a.domain.as_deref(), a.user.as_deref()),
            (Some("WORK"), Some("me"))
        );
        assert_eq!(a.first_part(), b"photos");
        assert_eq!(a.top().path, b"/photos/");
        assert_eq!(a.label(), "photos on nas");
        assert_eq!(a.to_string(), "smb://WORK;me@nas/photos/2024");
    }

    #[test]
    fn its_uri_escapes_what_a_uri_path_cannot_hold() {
        let a = Address::parse("sftp://me@h/home/me/Two Words/é").unwrap();
        assert_eq!(a.uri(), "sftp://me@h/home/me/Two%20Words/%C3%A9");
    }

    #[test]
    fn ipv6_hosts_keep_their_brackets() {
        let a = Address::parse("sftp://[fe80::1]:2200/srv").unwrap();
        assert_eq!((a.host.as_str(), a.port), ("[fe80::1]", Some(2200)));
        assert_eq!(a.bare_host(), "fe80::1");
        assert_eq!(a.to_string(), "sftp://[fe80::1]:2200/srv");
    }

    #[test]
    fn what_a_mount_holds_is_below_its_address() {
        let mount = Address::parse("sftp://me@h/").unwrap();
        let there = Address::parse("sftp://me@h/home/me/docs").unwrap();
        assert_eq!(mount.below(&there), Some(b"home/me/docs".as_slice()));
        assert_eq!(mount.below(&mount), Some(b"".as_slice()));
        let usual_port = Address::parse("sftp://me@h:22/x").unwrap();
        assert_eq!(mount.below(&usual_port), Some(b"x".as_slice()));
        let other = Address::parse("sftp://you@h/x").unwrap();
        assert_eq!(mount.below(&other), None, "another user");
        let share = Address::parse("smb://nas/photos/").unwrap();
        assert_eq!(
            share.below(&Address::parse("smb://nas/photos/2024").unwrap()),
            Some(b"2024".as_slice())
        );
        assert_eq!(
            share.below(&Address::parse("smb://nas/photosynth").unwrap()),
            None,
            "a share whose name starts the same"
        );
    }

    #[test]
    fn an_address_is_a_location_of_its_scheme() {
        let a = Address::parse("rclone://gdrive/Photos/2024").unwrap();
        let location = a.location();
        assert_eq!(&*location.scheme, "rclone");
        assert_eq!(location.to_string(), "rclone://gdrive/Photos/2024");
        assert_eq!(Address::of_location(&location).unwrap(), a);
        assert_eq!(a.label(), "gdrive");
    }
}
