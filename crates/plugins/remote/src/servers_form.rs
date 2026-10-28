//! A server added by its address (ADR-086, amended 2026-10-24): SFTP,
//! SMB, WebDAV, FTP, NFS, AFP, picked in Add Server's list beside rclone's
//! kinds of storage. Its parts asked in a form become an address, kept
//! under the name given in Skwir's list or KDE's network folders
//! (`servers`), shown in the Network section; then it is opened, its
//! password asked at this first connection, which the window offers to
//! keep in the keyring (ADR-082). Edit asks the same form again, filled in.

use skwir_core::app::App;
use skwir_core::cancel::CancelToken;
use skwir_core::form::{Answer, Choice, Field, Form, Kind, Values};
use skwir_core::workspace::PaneId;

use crate::address::{Address, Scheme};
use crate::servers::Kept;

/// What a kind of server picked by its address is named in Add Server's
/// list: this, then the scheme's name.
pub const KIND_PREFIX: &str = "address:";

/// The heading of the kinds added by their address in Add Server's list.
const SECTION: &str = "Servers, by their address";

/// The schemes a server is added by, in the list's order.
const SCHEMES: [Scheme; 8] = [
    Scheme::Sftp,
    Scheme::Smb,
    Scheme::Dav,
    Scheme::Davs,
    Scheme::Ftp,
    Scheme::Ftps,
    Scheme::Nfs,
    Scheme::Afp,
];

/// Ask what kind of server to add, among those added by their address
/// alone (where rclone is not installed), then add it: its name once
/// kept, `None` when the user gave up.
///
/// # Errors
/// As [`add`].
pub fn add_any(app: &App, pane: PaneId, cancel: &CancelToken) -> anyhow::Result<Option<String>> {
    loop {
        let pick = Form::new("Add Server", "Next")
            .intro("A server by its address. With rclone installed, seventy kinds of storage more.")
            .field(
                Field::new(
                    "picked",
                    "Kind of server",
                    Kind::Pick { choices: choices() },
                )
                .required(),
            );
        let Some(Answer::Accepted(picked)) = app.fill(pick, cancel) else {
            return Ok(None);
        };
        let Some(scheme) = scheme_of(picked.get("picked")) else {
            return Ok(None);
        };
        match add(
            app,
            scheme,
            &std::collections::BTreeMap::new(),
            true,
            pane,
            cancel,
        )? {
            Added::Done(name) => return Ok(Some(name)),
            Added::Back => {}
            Added::Cancelled => return Ok(None),
        }
    }
}

/// The kinds of server added by their address, as Add Server lists them.
pub fn choices() -> Vec<Choice> {
    SCHEMES
        .iter()
        .map(|&scheme| {
            let (title, detail) = match scheme {
                Scheme::Sftp => ("SFTP Server", "A computer reached over ssh"),
                Scheme::Smb => (
                    "Windows Share (SMB)",
                    "A NAS's or a computer's shared folder",
                ),
                Scheme::Dav => ("WebDAV Server", "Nextcloud, ownCloud and others, over HTTP"),
                Scheme::Davs => ("WebDAV Server (HTTPS)", "Nextcloud, ownCloud and others"),
                Scheme::Ftp => ("FTP Server", "A server's files over FTP"),
                Scheme::Ftps => ("FTP Server (TLS)", "A server's files over FTP with TLS"),
                Scheme::Nfs => ("NFS Export", "A Unix server's shared folder"),
                Scheme::Afp => ("AFP Volume", "An older Mac's or NAS's shared volume"),
                Scheme::Rclone | Scheme::S3 | Scheme::Mtp | Scheme::Gphoto2 | Scheme::Afc => {
                    ("", "")
                }
            };
            Choice::new(format!("{KIND_PREFIX}{}", scheme.name()), title)
                .detail(detail)
                .section(SECTION)
        })
        .collect()
}

/// The scheme a kind picked names, where it is a server's address.
pub fn scheme_of(kind: &str) -> Option<Scheme> {
    let name = kind.strip_prefix(KIND_PREFIX)?;
    Scheme::from_name(name).filter(|s| SCHEMES.contains(s))
}

/// The form of a server of `scheme`, to be kept as `kept` says: its
/// address's parts, then the name the Network section calls it by.
pub fn form(scheme: Scheme, kept: Kept, back: bool) -> Form {
    let (folder, folder_help) = match scheme {
        Scheme::Smb => (
            "Share",
            "The shared folder's name, and a folder of it after a /",
        ),
        Scheme::Afp => (
            "Volume",
            "The shared volume's name, and a folder of it after a /",
        ),
        Scheme::Nfs => (
            "Export",
            "The exported folder's path on the server: /srv/photos",
        ),
        _ => (
            "Folder",
            "Where to open, on the server: empty for its top (SFTP: your home)",
        ),
    };
    let kept = match kept {
        Kept::Kde => "Kept in KDE's network folders, where Dolphin lists it too.",
        _ => "Kept in Skwir's list of servers.",
    };
    let mut form = Form::new(format!("Add {}", title(scheme)), "Add")
        .intro(format!(
            "{kept} Its password is asked when it first connects."
        ))
        .field(
            Field::text("host", "Server")
                .help("Its name or address: nas.local, files.example.com, 192.168.1.20")
                .required(),
        )
        .field(Field::text("path", folder).help(folder_help));
    if scheme == Scheme::Nfs {
        let last = form.fields.len() - 1;
        form.fields[last].required = true;
    } else {
        form = form.field(Field::text("user", "User").help(
            "The name you log in with; empty for your own (SFTP: as your ssh configuration says)",
        ));
    }
    if scheme == Scheme::Smb {
        form = form.field(
            Field::text("domain", "Domain")
                .help("The Windows domain or workgroup, where the server wants one")
                .advanced(),
        );
    }
    form = form
        .field(Field::new("port", "Port", Kind::Integer { min: 1, max: 65535 }).advanced())
        .field(
            Field::text("title", "Name")
                .help("What the sidebar calls it; its address's own name where empty"),
        );
    if back { form.with_back() } else { form }
}

/// A scheme in the user's words, for a form's title.
fn title(scheme: Scheme) -> &'static str {
    match scheme {
        Scheme::Smb => "Windows Share",
        Scheme::Nfs => "NFS Export",
        Scheme::Afp => "AFP Volume",
        Scheme::Davs => "WebDAV Server (HTTPS)",
        Scheme::Ftps => "FTP Server (TLS)",
        _ => match scheme.title() {
            "SFTP" => "SFTP Server",
            "WebDAV" => "WebDAV Server",
            "FTP" => "FTP Server",
            other => other,
        },
    }
}

/// The address the form's `values` make for `scheme`, or why none.
///
/// # Errors
/// What makes it no address, in the user's words.
pub fn address_of(scheme: Scheme, values: &Values) -> anyhow::Result<Address> {
    let text = |key: &str| {
        let value = values.get(key).trim();
        (!value.is_empty()).then(|| value.to_owned())
    };
    let host = text("host").ok_or_else(|| anyhow::anyhow!("A server is needed"))?;
    let port = match text("port") {
        Some(port) => Some(
            port.parse::<u16>()
                .ok()
                .filter(|p| *p > 0)
                .ok_or_else(|| anyhow::anyhow!("A port is a number from 1 to 65535"))?,
        ),
        None => None,
    };
    let path = text("path").unwrap_or_default();
    let path = format!("/{}", path.trim_start_matches('/'));
    let address = Address {
        scheme,
        user: text("user"),
        domain: text("domain").filter(|_| scheme == Scheme::Smb),
        host,
        port,
        path: path.into_bytes(),
    };
    // Read back as any address is: its server's name checked, `..` and
    // `.` refused.
    Address::parse(&address.to_string()).map_err(|refused| anyhow::anyhow!("{refused}"))
}

/// What became of a server's form.
pub enum Added {
    /// Kept, by the name given, and opened.
    Done(String),
    Back,
    Cancelled,
}

/// Ask the parts of a server of `scheme` until they make an address of no
/// server kept yet, keep it (`servers::add`) and open it in `pane`, the
/// one the command was asked in: its password is asked then, by the
/// connection. The form starts from `given` (a caller's, never a
/// password: the form asks none).
///
/// # Errors
/// When `given` names no part of the form, or the server could not be
/// written where it is kept.
pub fn add(
    app: &App,
    scheme: Scheme,
    given: &std::collections::BTreeMap<String, String>,
    back: bool,
    pane: PaneId,
    cancel: &CancelToken,
) -> anyhow::Result<Added> {
    let mut blank = form(scheme, crate::servers::keeper(app, scheme), back);
    for (key, value) in given {
        let field = blank
            .fields
            .iter_mut()
            .find(|f| f.key == *key)
            .ok_or_else(|| anyhow::anyhow!("a server's address has no part “{key}”"))?;
        field.value.clone_from(value);
        field.advanced = false;
    }
    let mut asked = blank.clone();
    loop {
        let values = match app.fill(asked, cancel) {
            Some(Answer::Accepted(values)) => values,
            Some(Answer::Back(_)) => return Ok(Added::Back),
            None => return Ok(Added::Cancelled),
        };
        let address = match address_of(scheme, &values) {
            Ok(address) => address,
            Err(refused) => {
                asked = blank.clone().again(&values, format!("{refused:#}"));
                continue;
            }
        };
        if let Some(kept) = crate::servers::find(app, &address) {
            asked = blank.clone().again(
                &values,
                format!("“{}” is a server already, at that address", kept.label),
            );
            continue;
        }
        let name = name_of(&values, &address);
        crate::servers::add(app, &name, &address)?;
        if let Err(error) = app.navigate(pane, &address.location()) {
            tracing::debug!("the server added was not opened: {error:#}");
        }
        return Ok(Added::Done(name));
    }
}

/// The name a server's form gives it: its address's own where empty.
fn name_of(values: &Values, address: &Address) -> String {
    let name = values.get("title").trim();
    if name.is_empty() {
        address.label()
    } else {
        name.to_owned()
    }
}

/// Ask the parts of `server`, added by its address, again, its form filled
/// in as it is kept: its name and new address, `None` when the user gave
/// up. Changing it where it is kept is the caller's.
///
/// # Errors
/// None yet: the form refuses what makes no address, or one another
/// server is kept at.
pub fn edit(
    app: &App,
    server: &crate::connector::Server,
    cancel: &CancelToken,
) -> anyhow::Result<Option<(String, Address)>> {
    let old = &server.address;
    let mut blank = form(old.scheme, server.kept, false);
    blank.title = format!("Edit “{}”", server.label);
    "Save".clone_into(&mut blank.accept);
    blank.intro = format!(
        "Kept {}. Its mount options follow it to a new address.",
        server.kept.said()
    );
    let path = String::from_utf8_lossy(&old.path);
    let named = (server.label != old.label()).then(|| server.label.clone());
    for (key, value) in [
        ("host", Some(old.host.clone())),
        ("path", Some(path.trim_start_matches('/').to_owned())),
        ("user", old.user.clone()),
        ("domain", old.domain.clone()),
        ("port", old.port.map(|p| p.to_string())),
        ("title", named),
    ] {
        if let (Some(field), Some(value)) = (blank.fields.iter_mut().find(|f| f.key == key), value)
        {
            field.value = value;
            field.advanced = false;
        }
    }
    let mut asked = blank.clone();
    loop {
        let Some(Answer::Accepted(values)) = app.fill(asked, cancel) else {
            return Ok(None);
        };
        let address = match address_of(old.scheme, &values) {
            Ok(address) => address,
            Err(refused) => {
                asked = blank.clone().again(&values, format!("{refused:#}"));
                continue;
            }
        };
        if address != *old
            && let Some(other) = crate::servers::find(app, &address)
        {
            asked = blank.clone().again(
                &values,
                format!("“{}” is a server already, at that address", other.label),
            );
            continue;
        }
        return Ok(Some((name_of(&values, &address), address)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values(pairs: &[(&str, &str)]) -> Values {
        let mut values = Values::default();
        for (key, value) in pairs {
            values.set(*key, *value);
        }
        values
    }

    #[test]
    fn a_server_s_parts_make_its_address() {
        let address = address_of(
            Scheme::Smb,
            &values(&[
                ("host", "nas.local"),
                ("path", "photos/2024 it's"),
                ("user", "me"),
                ("domain", "WORK"),
            ]),
        )
        .unwrap();
        assert_eq!(
            address.to_string(),
            "smb://WORK;me@nas.local/photos/2024 it's"
        );
        assert_eq!(address.first_part(), b"photos");
        let sftp = address_of(Scheme::Sftp, &values(&[("host", "h"), ("port", "2222")])).unwrap();
        assert_eq!(sftp.to_string(), "sftp://h:2222/");
        assert!(address_of(Scheme::Sftp, &values(&[("host", "a b")])).is_err());
        assert!(address_of(Scheme::Sftp, &values(&[("host", "h"), ("port", "99999")])).is_err());
        // A path is read as any address's: `..` and `.` taken out.
        let up = address_of(Scheme::Sftp, &values(&[("host", "h"), ("path", "a/../b")])).unwrap();
        assert_eq!(up.path, b"/b");
        // A domain is SMB's alone.
        let dav = address_of(Scheme::Dav, &values(&[("host", "h"), ("domain", "WORK")])).unwrap();
        assert_eq!(dav.domain, None);
    }

    #[test]
    fn the_kinds_added_by_address_are_told_from_rclone_s() {
        let all = choices();
        assert_eq!(all.len(), 8);
        assert_eq!(scheme_of(&all[0].value), Some(Scheme::Sftp));
        assert_eq!(scheme_of("sftp"), None, "rclone's own kind");
        assert_eq!(scheme_of("address:rclone"), None);
        assert!(
            form(Scheme::Nfs, Kept::Skwir, false)
                .fields
                .iter()
                .any(|f| f.key == "path" && f.required)
        );
        assert!(
            !form(Scheme::Nfs, Kept::Skwir, false)
                .fields
                .iter()
                .any(|f| f.key == "user")
        );
    }
}
