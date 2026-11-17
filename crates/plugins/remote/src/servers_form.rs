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
use skwir_core::words;
use skwir_core::words::Words;
use skwir_core::workspace::PaneId;

use crate::address::{Address, Scheme};
use crate::servers::Kept;

/// What a kind of server picked by its address is named in Add Server's
/// list: this, then the scheme's name.
pub const KIND_PREFIX: &str = "address:";

/// The heading of the kinds added by their address in Add Server's list.
fn section() -> Words {
    words!(
        remote_server_form_section = "Servers, by their address",
        note = "A heading in Add Server's list of kinds of storage."
    )
}

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
pub fn add_any(app: &App, pane: PaneId, cancel: &CancelToken) -> anyhow::Result<Option<Words>> {
    loop {
        let pick = Form::new(
            words!(remote_server_form_add_server = "Add Server"),
            words!(remote_server_form_next = "Next"),
        )
        .intro(words!(
            remote_server_form_add_intro =
                "A server by its address. With rclone installed, seventy kinds of storage more."
        ))
        .field(
            Field::new(
                "picked",
                words!(remote_server_form_kind = "Kind of server"),
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
            let detail = match scheme {
                Scheme::Sftp => {
                    words!(remote_server_form_sftp_detail = "A computer reached over ssh")
                }
                Scheme::Smb => {
                    words!(remote_server_form_smb_detail = "A NAS's or a computer's shared folder")
                }
                Scheme::Dav => words!(
                    remote_server_form_dav_detail = "Nextcloud, ownCloud and others, over HTTP"
                ),
                Scheme::Davs => {
                    words!(remote_server_form_davs_detail = "Nextcloud, ownCloud and others")
                }
                Scheme::Ftp => words!(remote_server_form_ftp_detail = "A server's files over FTP"),
                Scheme::Ftps => {
                    words!(remote_server_form_ftps_detail = "A server's files over FTP with TLS")
                }
                Scheme::Nfs => {
                    words!(remote_server_form_nfs_detail = "A Unix server's shared folder")
                }
                Scheme::Afp => {
                    words!(remote_server_form_afp_detail = "An older Mac's or NAS's shared volume")
                }
                Scheme::Rclone | Scheme::S3 | Scheme::Mtp | Scheme::Gphoto2 | Scheme::Afc => {
                    Words::default()
                }
            };
            Choice::new(format!("{KIND_PREFIX}{}", scheme.name()), kind(scheme))
                .detail(detail)
                .section(section())
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
            words!(remote_server_form_share = "Share"),
            words!(
                remote_server_form_share_help =
                    "The shared folder's name, and a folder of it after a /"
            ),
        ),
        Scheme::Afp => (
            words!(remote_server_form_volume = "Volume"),
            words!(
                remote_server_form_volume_help =
                    "The shared volume's name, and a folder of it after a /"
            ),
        ),
        Scheme::Nfs => (
            words!(remote_server_form_export = "Export"),
            words!(
                remote_server_form_export_help =
                    "The exported folder's path on the server: /srv/photos"
            ),
        ),
        _ => (
            words!(remote_server_form_folder = "Folder"),
            words!(
                remote_server_form_folder_help =
                    "Where to open, on the server: empty for its top (SFTP: your home)"
            ),
        ),
    };
    let intro = match kept {
        Kept::Kde => words!(
            remote_server_form_intro_kde = "Kept in KDE's network folders, where Dolphin lists it too. Its password is asked when it first connects."
        ),
        _ => words!(
            remote_server_form_intro_skwir =
                "Kept in Skwir's list of servers. Its password is asked when it first connects."
        ),
    };
    let mut form = Form::new(add_title(scheme), words!(remote_server_form_add = "Add"))
        .intro(intro)
        .field(
            Field::text("host", words!(remote_server_form_host = "Server"))
                .help(words!(
                    remote_server_form_host_help =
                        "Its name or address: nas.local, files.example.com, 192.168.1.20"
                ))
                .required(),
        )
        .field(Field::text("path", folder).help(folder_help));
    if scheme == Scheme::Nfs {
        let last = form.fields.len() - 1;
        form.fields[last].required = true;
    } else {
        form = form.field(
            Field::text("user", words!(remote_server_form_user = "User")).help(words!(
                remote_server_form_user_help = "The name you log in with; empty for your own (SFTP: as your ssh configuration says)"
            )),
        );
    }
    if scheme == Scheme::Smb {
        form = form.field(
            Field::text("domain", words!(remote_server_form_domain = "Domain"))
                .help(words!(
                    remote_server_form_domain_help =
                        "The Windows domain or workgroup, where the server wants one"
                ))
                .advanced(),
        );
    }
    form = form
        .field(
            Field::new(
                "port",
                words!(remote_server_form_port = "Port"),
                Kind::Integer { min: 1, max: 65535 },
            )
            .advanced(),
        )
        .field(
            Field::text(
                "title",
                words!(
                    remote_server_form_name = "Name",
                    note = "The name a server is given in the sidebar."
                ),
            )
            .help(words!(
                remote_server_form_name_help =
                    "What the sidebar calls it; its address's own name where empty"
            )),
        );
    if back { form.with_back() } else { form }
}

/// A kind of server added by its address, as Add Server's list names it.
fn kind(scheme: Scheme) -> Words {
    match scheme {
        Scheme::Sftp => words!(remote_server_form_sftp = "SFTP Server"),
        Scheme::Smb => words!(remote_server_form_smb = "Windows Share (SMB)"),
        Scheme::Dav => words!(remote_server_form_dav = "WebDAV Server"),
        Scheme::Davs => words!(remote_server_form_davs = "WebDAV Server (HTTPS)"),
        Scheme::Ftp => words!(remote_server_form_ftp = "FTP Server"),
        Scheme::Ftps => words!(remote_server_form_ftps = "FTP Server (TLS)"),
        Scheme::Nfs => words!(remote_server_form_nfs = "NFS Export"),
        Scheme::Afp => words!(remote_server_form_afp = "AFP Volume"),
        Scheme::Rclone | Scheme::S3 | Scheme::Mtp | Scheme::Gphoto2 | Scheme::Afc => {
            Words::default()
        }
    }
}

/// The title of the form adding a server of `scheme`.
fn add_title(scheme: Scheme) -> Words {
    match scheme {
        Scheme::Sftp => words!(remote_server_form_add_sftp = "Add SFTP Server"),
        Scheme::Smb => words!(remote_server_form_add_smb = "Add Windows Share"),
        Scheme::Dav => words!(remote_server_form_add_dav = "Add WebDAV Server"),
        Scheme::Davs => words!(remote_server_form_add_davs = "Add WebDAV Server (HTTPS)"),
        Scheme::Ftp => words!(remote_server_form_add_ftp = "Add FTP Server"),
        Scheme::Ftps => words!(remote_server_form_add_ftps = "Add FTP Server (TLS)"),
        Scheme::Nfs => words!(remote_server_form_add_nfs = "Add NFS Export"),
        Scheme::Afp => words!(remote_server_form_add_afp = "Add AFP Volume"),
        // Never asked: these are added otherwise.
        Scheme::Rclone | Scheme::S3 | Scheme::Mtp | Scheme::Gphoto2 | Scheme::Afc => {
            words!(
                remote_server_form_add_other = "Add { $kind }",
                kind = scheme.title(),
                note = "kind: a protocol's name (rclone, S3)."
            )
        }
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
    let host = text("host").ok_or_else(|| {
        anyhow::anyhow!(words!(
            remote_server_form_host_needed = "A server is needed"
        ))
    })?;
    let port = match text("port") {
        Some(port) => Some(port.parse::<u16>().ok().filter(|p| *p > 0).ok_or_else(|| {
            anyhow::anyhow!(words!(
                remote_server_form_port_refused = "A port is a number from 1 to 65535"
            ))
        })?),
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
    Address::parse(&address.to_string()).map_err(anyhow::Error::new)
}

/// What became of a server's form.
pub enum Added {
    /// Kept, by the name given (else its address's), and opened.
    Done(Words),
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
            .ok_or_else(|| {
                anyhow::anyhow!(words!(
                    remote_server_form_no_part = "a server's address has no part “{ $key }”",
                    key = key.as_str(),
                    note = "key: the name of a part a caller gave (host, port), as it gave it."
                ))
            })?;
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
                asked = blank.clone().again(&values, Words::of_error(refused));
                continue;
            }
        };
        if let Some(kept) = crate::servers::find(app, &address) {
            asked = blank.clone().again(&values, server_already(&kept.label));
            continue;
        }
        let name = name_of(&values);
        crate::servers::add(app, &name, &address)?;
        if let Err(error) = app.navigate(pane, &address.location()) {
            tracing::debug!("the server added was not opened: {error:#}");
        }
        return Ok(Added::Done(crate::servers::label_of(&name, &address)));
    }
}

/// A server is kept already at the address given: the one `named`.
fn server_already(named: &Words) -> Words {
    words!(
        remote_server_form_already = "{ $server } is a server already, at that address",
        server = crate::address::named(named),
        note = "server: the name of the server kept at that address."
    )
}

/// The name a server's form gives it: empty where it goes by its
/// address's own (`servers::add`).
fn name_of(values: &Values) -> String {
    values.get("title").trim().to_owned()
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
    blank.title = words!(
        remote_server_form_edit = "Edit { $server }",
        server = crate::address::named(&server.label)
    );
    blank.accept = words!(remote_server_form_save = "Save");
    blank.intro = words!(
        remote_server_form_edit_intro =
            "Kept { $kept }. Its mount options follow it to a new address.",
        kept = server.kept.words(),
        note = "kept: where the server is kept (in Skwir's list, in KDE's network folders, in rclone's configuration)."
    );
    let path = String::from_utf8_lossy(&old.path);
    // The name the user gave it, as written: none where it goes by its
    // address's, whose words are no name to edit.
    let named = (server.label != old.label() && server.label.message_of().is_none())
        .then(|| server.label.english());
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
                asked = blank.clone().again(&values, Words::of_error(refused));
                continue;
            }
        };
        if address != *old
            && let Some(other) = crate::servers::find(app, &address)
        {
            asked = blank.clone().again(&values, server_already(&other.label));
            continue;
        }
        return Ok(Some((name_of(&values), address)));
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
