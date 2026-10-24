//! Adding, editing, testing, signing in again and removing the servers
//! the tools keep (ADR-086): rclone's remotes, in rclone's own
//! configuration, through forms the window draws (`rclone_forms`). Every
//! surface runs the same commands: Add Server… in the Network section's
//! menu and the palette, a remote's own menu, the list in Settings (its
//! records kept by rclone, `RecordSource`), an agent or a script. A form
//! is what the user reviews: a caller gives what it starts from, never a
//! secret, which the user types in the window.

use std::collections::BTreeMap;
use std::sync::Arc;

use skwir_core::app::{App, NotifyLevel};
use skwir_core::command::{Invocation, Outcome};
use skwir_core::registry::MenuItem;
use skwir_core::settings::{RecordField, SettingSpec};

use crate::address::{Address, Scheme};
use crate::{ID, Remotes};

/// The list of rclone's remotes in Settings, kept by rclone.
pub(crate) const RECORDS: &str = "remote.rclone_remotes";

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct AddArgs {
    /// rclone's type of storage (`s3`, `drive`, `webdav`, `crypt`): its
    /// list is asked first where none is given.
    #[serde(default)]
    kind: Option<String>,
    /// The provider of a type that has several (S3's `AWS`, `Minio`,
    /// `Cloudflare`).
    #[serde(default)]
    provider: Option<String>,
    /// The remote's name: letters a–z, digits and `_ - . +`.
    #[serde(default)]
    name: Option<String>,
    /// Options the form starts from, by rclone's names (`endpoint`,
    /// `region`). Never a secret: the user types those in the window.
    #[serde(default)]
    options: BTreeMap<String, String>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct PaneArg {
    #[serde(default)]
    pane: Option<skwir_core::workspace::PaneId>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct EntriesArgs {
    /// The file to link to: the selection's where none is given.
    #[serde(default)]
    entries: Vec<skwir_core::op::EntryArg>,
    #[serde(default)]
    pane: Option<skwir_core::workspace::PaneId>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct AddressArgs {
    /// The server's address (`sftp://me@nas/`, `rclone://photos/`).
    address: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct AtLoginArgs {
    /// The server's address: a remote of rclone's configuration
    /// (`rclone://photos/`) or an SFTP server (`sftp://me@nas/`).
    address: String,
    /// Connect it at login (true), or no longer (false).
    on: bool,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct NamedArgs {
    /// The remote's name in rclone's configuration.
    id: String,
}

/// The items of an rclone remote's menu in the sidebar: Edit, Sign In
/// Again where it signs in through a browser (`signs_in`), Remove.
pub(crate) fn menu_items(name: &str, signs_in: bool) -> Vec<MenuItem> {
    let named = |command: &str| Invocation::new(command).with("id", name);
    let mut items = vec![
        MenuItem::new(
            "place",
            "Edit Server…",
            "settings",
            410,
            named("remote.edit_server"),
        ),
        MenuItem::new("place", "Test Server", "", 411, named("remote.test_server")),
        MenuItem::new(
            "place",
            "Remove Server…",
            "trash",
            310,
            named("remote.remove_server"),
        ),
    ];
    if signs_in {
        items.push(MenuItem::new(
            "place",
            "Sign In Again…",
            "",
            412,
            named("remote.reconnect"),
        ));
    }
    items
}

/// The id of the entry that connects `address`'s server at login, where
/// one may (ADR-087): a remote of rclone's configuration, by rclone; an
/// SFTP server, by sshfs.
pub(crate) fn login_id(address: &Address) -> Option<String> {
    match address.scheme {
        Scheme::Rclone => Some(crate::login::id_of("rclone", address)),
        Scheme::Sftp => Some(crate::login::id_of("sshfs", address)),
        _ => None,
    }
}

/// A server's own items in its menu: Connect at Login (checked while it
/// does), and for SFTP, Set Up Key Login.
pub(crate) fn server_items(
    address: &Address,
    at_login: &std::collections::HashSet<String>,
) -> Vec<MenuItem> {
    let mut items = Vec::new();
    if let Some(id) = login_id(address) {
        let on = at_login.contains(&id);
        items.push(MenuItem::new(
            "place",
            if on {
                "Connect at Login ✓"
            } else {
                "Connect at Login"
            },
            "",
            430,
            Invocation::with_args(
                "remote.connect_at_login",
                &serde_json::json!({ "address": address.top().to_string(), "on": !on }),
            ),
        ));
    }
    if address.scheme == Scheme::Sftp {
        items.push(MenuItem::new(
            "place",
            "Set Up Key Login…",
            "lock",
            431,
            Invocation::new("remote.setup_key_login").with("address", address.top().to_string()),
        ));
    }
    items
}

/// The name of the rclone remote at `address`'s top, if it is one.
pub(crate) fn remote_name(address: &Address) -> Option<&str> {
    (address.scheme == Scheme::Rclone).then_some(address.host.as_str())
}

/// The types of rclone's storage that sign in through a browser, as
/// rclone names them. Read from what it describes where a form needs it;
/// for the menus, which are drawn from memory, these.
const SIGNS_IN: &[&str] = &[
    "box",
    "drive",
    "dropbox",
    "google photos",
    "gcs",
    "google cloud storage",
    "hidrive",
    "jottacloud",
    "onedrive",
    "pcloud",
    "premiumizeme",
    "putio",
    "sharefile",
    "yandex",
    "zoho",
    "pikpak",
    "mailru",
];

/// Whether rclone's storage of type `kind` signs in through a browser.
pub(crate) fn signs_in(kind: &str) -> bool {
    SIGNS_IN.contains(&kind)
}

pub(crate) fn register(remotes: &Remotes, app: &App) {
    app.settings.declare(
        SettingSpec::records(
            RECORDS,
            "Remotes of rclone",
            "The remotes of rclone's own configuration, where rclone in a terminal finds them too: Skwir keeps no copy.",
            "Server",
            vec![
                RecordField::text("name", "Name", "", ""),
                RecordField::text("kind", "Kind", "", ""),
            ],
        )
        .records_kept_by(remotes.records.clone())
        .record_action("Test", "remote.test_server")
        .page(skwir_core::settings::page::GENERAL)
        .section("Remote folders"),
    );
    // The Network section's own menu.
    app.registry.menus.add(
        ID,
        MenuItem::new(
            "sidebar:network",
            "Add Server…",
            "plus",
            -100,
            Invocation::new("remote.add_server"),
        ),
    );
    let remotes_ = remotes.clone();
    app.commands
        .register::<skwir_core::command::NoArgs, _>(
            ID,
            "remote.rclone_records",
            "rclone's remotes",
            "rclone's remotes as Settings lists them: each one's name and kind",
            move |_, _| {
                let records: Vec<serde_json::Value> = remotes_
                    .state
                    .lock()
                    .servers
                    .iter()
                    .filter_map(|server| {
                        let name = remote_name(&server.address)?;
                        Some(serde_json::json!({
                            "id": name,
                            "name": name,
                            "kind": server.kind,
                        }))
                    })
                    .collect();
                Ok(Outcome::Value(serde_json::json!({ "records": records })))
            },
        )
        .data();
    let remotes_ = remotes.clone();
    app.commands
        .register::<AddArgs, _>(
            ID,
            "remote.add_server",
            "Add server…",
            "Add a server: a remote of rclone's (cloud storage, S3, WebDAV, SFTP and seventy more), in a form the user fills; what it starts from may be given, never a secret",
            move |args, app| {
                window_for_forms(app)?;
                let prefill = crate::rclone_forms::Prefill {
                    kind: args.kind,
                    provider: args.provider,
                    name: args.name,
                    options: args.options,
                };
                // Without rclone, the servers added by their address alone.
                if skwir_core::os_path::find_program("rclone").is_none() {
                    return servers_alone(&remotes_, app);
                }
                in_form(&remotes_, app, "added", move |flow, remotes, app| {
                    let made = flow.add(&prefill)?;
                    if let Some(crate::rclone_forms::Made::Bookmark(name)) = &made {
                        app.notify(NotifyLevel::Info, format!("“{name}” is kept as a bookmark"));
                    }
                    if let Some(crate::rclone_forms::Made::Remote(name)) = made {
                        remotes.refresh(app);
                        // What it keeps of its secrets is only obscured:
                        // encrypting the whole is offered.
                        let rclone = skwir_core::os_path::find_program("rclone");
                        let plain = rclone.is_some_and(|rclone| {
                            crate::rclone_config::encrypted(&rclone, &remotes.rclone, flow.cancel)
                                .is_ok_and(|encrypted| !encrypted)
                        });
                        if plain {
                            app.notify_with_action(
                                NotifyLevel::Info,
                                format!("“{name}” is added. rclone keeps its secrets only obscured: encrypt its configuration?"),
                                "Encrypt…",
                                Invocation::new("remote.encrypt_rclone_config"),
                            );
                        } else {
                            app.notify(NotifyLevel::Info, format!("“{name}” is added"));
                        }
                    }
                    Ok(())
                })?;
                Ok(Outcome::Done)
            },
        )
        .shows();
    let remotes_ = remotes.clone();
    app.commands
        .register::<NamedArgs, _>(
            ID,
            "remote.edit_server",
            "Edit server…",
            "Change a remote of rclone's in its form",
            move |args, app| {
                window_for_forms(app)?;
                in_form(&remotes_, app, "saved", move |flow, remotes, app| {
                    if flow.edit(&args.id)? {
                        remotes.refresh(app);
                        app.notify(NotifyLevel::Info, format!("“{}” is saved", args.id));
                    }
                    Ok(())
                })?;
                Ok(Outcome::Done)
            },
        )
        .shows();
    let remotes_ = remotes.clone();
    app.commands
        .register::<NamedArgs, _>(
            ID,
            "remote.reconnect",
            "Sign in again…",
            "Sign a remote of rclone's in again through the browser, its access having expired or been revoked",
            move |args, app| {
                window_for_forms(app)?;
                in_form(&remotes_, app, "signed in", move |flow, remotes, app| {
                    if flow.reconnect(&args.id)? {
                        remotes.refresh(app);
                        app.notify(NotifyLevel::Info, format!("“{}” is signed in again", args.id));
                    }
                    Ok(())
                })?;
                Ok(Outcome::Done)
            },
        )
        .shows();
    let remotes_ = remotes.clone();
    app.commands
        .register::<NamedArgs, _>(
            ID,
            "remote.remove_server",
            "Remove server…",
            "Remove a remote from rclone's configuration, once the user confirms: what it holds stays where it is",
            move |args, app| {
                window_for_forms(app)?;
                in_form(&remotes_, app, "removed", move |flow, remotes, app| {
                    let address = Address::parse(&format!("rclone://{}/", args.id))?;
                    let mounted: Vec<std::path::PathBuf> = remotes
                        .mounted()
                        .into_iter()
                        .filter(|m| m.recognised.address.as_ref() == Some(&address))
                        .map(|m| m.remote.point.clone())
                        .collect();
                    if !flow.confirm_remove(&args.id, !mounted.is_empty()) {
                        return Ok(());
                    }
                    // Let go of first: what rclone mounted keeps working
                    // from what it read of the configuration.
                    for point in mounted {
                        remotes.disconnect(app, point, false)?;
                    }
                    flow.session.delete(&args.id, flow.cancel)?;
                    remotes.refresh(app);
                    app.notify(NotifyLevel::Info, format!("“{}” is removed", args.id));
                    Ok(())
                })?;
                Ok(Outcome::Done)
            },
        )
        .shows();
    let remotes_ = remotes.clone();
    app.commands
        .register::<skwir_core::command::NoArgs, _>(
            ID,
            "remote.encrypt_rclone_config",
            "Encrypt rclone's configuration…",
            "Encrypt rclone's configuration file, which keeps its remotes' secrets only obscured, with a password the user types and may keep in the keyring",
            move |_, app| {
                window_for_forms(app)?;
                let remotes = remotes_.clone();
                let rclone = skwir_core::os_path::find_program("rclone")
                    .ok_or_else(|| anyhow::anyhow!("rclone is not installed"))?;
                let lifetime = remotes
                    .lifetime()
                    .ok_or_else(|| anyhow::anyhow!("remote folders are turned off"))?;
                let weak = app.weak();
                std::thread::Builder::new()
                    .name("skwir-remote-form".into())
                    .spawn(move || {
                        let Some(app) = weak.upgrade() else { return };
                        let cancel = lifetime.child();
                        if let Err(error) = encrypt_in_form(&app, &remotes, &rclone, &cancel)
                            && !cancel.is_cancelled()
                        {
                            app.notify(
                                NotifyLevel::Error,
                                format!("rclone's configuration was not encrypted: {error:#}"),
                            );
                        }
                    })?;
                Ok(Outcome::Done)
            },
        )
        .shows();
    // Copy Public Link, where the storage of a mount Skwir made makes one
    // (rclone's `PublicLink`): offered from what was asked of the mount,
    // in memory.
    let remotes_ = remotes.clone();
    app.commands
        .register::<PaneArg, _>(
            ID,
            "remote.public_link_menu",
            "Offer a public link in the context menu",
            "The context menu's Copy Public Link, for a file of a remote whose storage makes one. Answers from memory",
            move |args, app| {
                let refs = app.selected_refs(app.pane_or_active(args.pane));
                let [one] = refs.as_slice() else {
                    return Ok(Outcome::Value(serde_json::json!({ "items": [] })));
                };
                let links = skwir_core::fs::reach::of(&one.path).is_some_and(|remote| {
                    remote.point != one.path
                        && remotes_
                            .state
                            .lock()
                            .features
                            .get(&remote.point)
                            .is_some_and(|f| f.public_link)
                });
                let items: Vec<MenuItem> = links
                    .then(|| {
                        MenuItem::new(
                            "context",
                            "Copy Public Link",
                            "link",
                            150,
                            Invocation::new("remote.public_link").with(
                                "entries",
                                serde_json::json!([skwir_core::op::EntryArg::from(one.clone())]),
                            ),
                        )
                    })
                    .into_iter()
                    .collect();
                Ok(Outcome::Value(serde_json::json!({ "items": items })))
            },
        )
        .data();
    app.registry.menu_sources.add(
        ID,
        skwir_core::registry::MenuSource {
            menu: "context".into(),
            when: Some("selection single !place".into()),
            command: Invocation::new("remote.public_link_menu"),
            replaces: Vec::new(),
        },
    );
    app.commands.register::<EntriesArgs, _>(
        ID,
        "remote.public_link",
        "Copy public link",
        "Make a public link to a file of a remote whose storage makes one (a cloud's share link), and put it on the clipboard",
        move |args, app| {
            let target = app
                .targets(args.pane, args.entries)?
                .into_iter()
                .next()
                .ok_or_else(|| anyhow::anyhow!("nothing to link to"))?;
            let remote = skwir_core::fs::reach::of(&target.path)
                .ok_or_else(|| anyhow::anyhow!("it is not in a remote folder"))?;
            let weak = app.weak();
            let cancel = app.lifetime().child();
            // The storage may take its time: in the remote's own lane.
            app.scheduler.spawn_in(
                remote.lane(),
                skwir_core::scheduler::Priority::Normal,
                "remote.public_link",
                cancel.clone(),
                move || {
                    let made = crate::rclone_features::public_link(&remote.point, &target.path, &cancel);
                    let Some(app) = weak.upgrade() else { return };
                    match made {
                        Ok(url) => app.request_front(Invocation::with_args(
                            "ui.copy_text",
                            &serde_json::json!({ "text": url, "what": "the public link" }),
                        )),
                        Err(error) => app.notify(
                            NotifyLevel::Error,
                            format!("No public link was made: {error:#}"),
                        ),
                    }
                },
            );
            Ok(Outcome::Done)
        },
    );
    let remotes_ = remotes.clone();
    app.commands
        .register::<AddressArgs, _>(
            ID,
            "remote.mount_options",
            "Mount options…",
            "Choose how a server is mounted when it next connects: read only, rclone's file cache and bandwidth, sshfs's compression",
            move |args, app| {
                window_for_forms(app)?;
                let top = Address::parse(args.address.trim())?.top();
                anyhow::ensure!(
                    crate::options::takes_options(top.scheme),
                    "no tool Skwir mounts {} with takes options",
                    top.scheme.title()
                );
                let lifetime = remotes_
                    .lifetime()
                    .ok_or_else(|| anyhow::anyhow!("remote folders are turned off"))?;
                let weak = app.weak();
                std::thread::Builder::new()
                    .name("skwir-remote-form".into())
                    .spawn(move || {
                        let Some(app) = weak.upgrade() else { return };
                        let cancel = lifetime.child();
                        let blank = crate::options::form(&top, &crate::options::of(&app, &top));
                        let mut asked = blank.clone();
                        loop {
                            let Some(skwir_core::form::Answer::Accepted(values)) =
                                app.fill(asked, &cancel)
                            else {
                                return;
                            };
                            match crate::options::of_values(&values) {
                                Ok(options) => {
                                    match crate::options::keep(&app, &top, &options) {
                                        Ok(()) => app.notify(
                                            NotifyLevel::Info,
                                            format!(
                                                "“{}” is mounted so when it next connects",
                                                top.label()
                                            ),
                                        ),
                                        Err(error) => app.notify(
                                            NotifyLevel::Error,
                                            format!("The mount options were not saved: {error:#}"),
                                        ),
                                    }
                                    return;
                                }
                                Err(refused) => {
                                    asked = blank.clone().again(&values, format!("{refused:#}"));
                                }
                            }
                        }
                    })?;
                Ok(Outcome::Done)
            },
        )
        .shows();
    let remotes_ = remotes.clone();
    app.commands.register::<AtLoginArgs, _>(
        ID,
        "remote.connect_at_login",
        "Connect at login",
        "Have the system's service manager connect a server when the user logs in (a systemd user unit, a launch agent, a logon task), or no longer; connects it now too. Only what asks nothing at login: a remote of rclone's configuration not encrypted, an SFTP server logged in to by key",
        move |args, app| {
            let top = Address::parse(args.address.trim())?.top();
            let id = login_id(&top).ok_or_else(|| {
                anyhow::anyhow!("only rclone's remotes and SFTP servers connect at login")
            })?;
            let (remotes, weak) = (remotes_.clone(), app.weak());
            let lifetime = remotes
                .lifetime()
                .ok_or_else(|| anyhow::anyhow!("remote folders are turned off"))?;
            // It logs in and waits on the service manager: on a thread.
            std::thread::Builder::new()
                .name("skwir-at-login".into())
                .spawn(move || {
                    let Some(app) = weak.upgrade() else { return };
                    let cancel = lifetime.child();
                    let done = at_login(&remotes, &top, &id, args.on, &cancel);
                    remotes.refresh(&app);
                    let label = top.label();
                    match (done, args.on) {
                        (Ok(()), true) => app.notify(
                            NotifyLevel::Info,
                            format!("“{label}” connects at login"),
                        ),
                        (Ok(()), false) => app.notify(
                            NotifyLevel::Info,
                            format!("“{label}” no longer connects at login"),
                        ),
                        (Err(error), _) if !cancel.is_cancelled() => app.notify(
                            NotifyLevel::Error,
                            format!("“{label}” was not set to connect at login: {error:#}"),
                        ),
                        (Err(_), _) => {}
                    }
                })?;
            Ok(Outcome::Done)
        },
    );
    let remotes_ = remotes.clone();
    app.commands
        .register::<AddressArgs, _>(
            ID,
            "remote.setup_key_login",
            "Set up key login…",
            "Let ssh log in to an SFTP server by key, no password: a key the ssh agent holds, the user's default one, or a new one, its public half added to the server's authorized keys once the user chose it",
            move |args, app| {
                window_for_forms(app)?;
                let top = Address::parse(args.address.trim())?.top();
                anyhow::ensure!(top.scheme == Scheme::Sftp, "key login is ssh's: SFTP servers alone");
                let (remotes, weak) = (remotes_.clone(), app.weak());
                let lifetime = remotes
                    .lifetime()
                    .ok_or_else(|| anyhow::anyhow!("remote folders are turned off"))?;
                std::thread::Builder::new()
                    .name("skwir-remote-form".into())
                    .spawn(move || {
                        let Some(app) = weak.upgrade() else { return };
                        let cancel = lifetime.child();
                        if let Err(error) = crate::keys_form::set_up(&app, &remotes.keys, &top, &cancel)
                            && !cancel.is_cancelled()
                        {
                            app.notify(
                                NotifyLevel::Error,
                                format!("Key login was not set up on “{}”: {error:#}", top.label()),
                            );
                        }
                    })?;
                Ok(Outcome::Done)
            },
        )
        .shows();
    let remotes_ = remotes.clone();
    app.commands.register::<NamedArgs, _>(
        ID,
        "remote.test_server",
        "Test server",
        "List the top of a remote of rclone's, as connecting would, and say whether it answers",
        move |args, app| {
            in_form(&remotes_, app, "tested", move |flow, _, app| {
                match flow.session.list_top(&format!("{}:", args.id), flow.cancel) {
                    Ok(()) => app.notify(NotifyLevel::Info, format!("“{}” answers", args.id)),
                    Err(error) => app.notify(
                        NotifyLevel::Error,
                        format!("“{}” does not answer: {error:#}", args.id),
                    ),
                }
                Ok(())
            })?;
            Ok(Outcome::Done)
        },
    );
}

/// Set the server at `top` to connect at login (`on`), or no longer: its
/// entry `id` with the service manager.
fn at_login(
    remotes: &Remotes,
    top: &Address,
    id: &str,
    on: bool,
    cancel: &skwir_core::cancel::CancelToken,
) -> anyhow::Result<()> {
    let manager = remotes.login_manager()?;
    let connector = if top.scheme == Scheme::Rclone {
        "rclone"
    } else {
        "sshfs"
    };
    if !on {
        let point = crate::login::point_of(connector, top)?;
        return manager.uninstall(id, &point, cancel);
    }
    let at = if top.scheme == Scheme::Rclone {
        crate::login::AtLogin::rclone(&remotes.rclone, top, cancel)?
    } else {
        crate::login::AtLogin::sshfs(&remotes.sshfs(), top, cancel)?
    };
    manager.install(&at, cancel)
}

/// Add Server where rclone is not installed: the servers added by their
/// address alone, on a thread of its own.
fn servers_alone(remotes: &Remotes, app: &App) -> anyhow::Result<Outcome> {
    let lifetime = remotes
        .lifetime()
        .ok_or_else(|| anyhow::anyhow!("remote folders are turned off"))?;
    let weak = app.weak();
    std::thread::Builder::new()
        .name("skwir-remote-form".into())
        .spawn(move || {
            let Some(app) = weak.upgrade() else { return };
            let cancel = lifetime.child();
            match crate::servers_form::add_any(&app, &cancel) {
                Ok(Some(name)) => {
                    app.notify(NotifyLevel::Info, format!("“{name}” is kept as a bookmark"));
                }
                Ok(None) => {}
                Err(error) => {
                    app.notify(
                        NotifyLevel::Error,
                        format!("The server was not added: {error:#}"),
                    );
                }
            }
        })?;
    Ok(Outcome::Done)
}

/// Ask for the password to encrypt rclone's configuration with, and
/// whether to keep it in the keyring, then encrypt it.
fn encrypt_in_form(
    app: &App,
    remotes: &Remotes,
    rclone: &std::path::Path,
    cancel: &skwir_core::cancel::CancelToken,
) -> anyhow::Result<()> {
    use skwir_core::form::{Answer, Field, Form, Kind};
    if crate::rclone_config::encrypted(rclone, &remotes.rclone, cancel)? {
        app.notify(
            NotifyLevel::Info,
            "rclone's configuration is encrypted already",
        );
        return Ok(());
    }
    let form = Form::new("Encrypt rclone's Configuration", "Encrypt")
        .intro("rclone keeps its remotes' keys and passwords in its configuration file, only obscured: anyone who reads the file can use them. Encrypted, it opens with a password.")
        .field(Field::new("password", "Password", Kind::Secret { kept: false }).required())
        .field(Field::new("again", "The Same Again", Kind::Secret { kept: false }).required())
        .field(
            Field::new("keep", "Keep It in the Keyring", Kind::Toggle)
                .value("true")
                .help("Skwir then opens the configuration without asking. rclone in a terminal asks for it, or reads it from RCLONE_CONFIG_PASS."),
        );
    let mut asked = form.clone();
    let values = loop {
        let Some(Answer::Accepted(values)) = app.fill(asked, cancel) else {
            return Ok(());
        };
        if values.get("password") == values.get("again") {
            break values;
        }
        asked = form.clone().again(&values, "The two passwords differ");
    };
    let password = values.get("password");
    // Kept first: a configuration encrypted with a password lost is lost.
    if values.on("keep") {
        remotes.rclone.keyring().keep(
            crate::rclone_config::KEYRING_KEY,
            "rclone's configuration",
            password,
        )?;
    }
    crate::rclone_config::encrypt(rclone, &remotes.rclone, password, cancel)?;
    app.notify(NotifyLevel::Info, "rclone's configuration is encrypted");
    Ok(())
}

/// Whether the window is there to fill a form in: a command that asks one
/// without it would do nothing and say nothing.
fn window_for_forms(app: &App) -> anyhow::Result<()> {
    anyhow::ensure!(
        app.window_shown(),
        "this is asked in a form, filled in Skwir's window: open it first"
    );
    Ok(())
}

/// Run `work` on a thread of its own with a configuration session of
/// rclone's, which it may fill forms in: what failed is said, as what was
/// to be `done` that was not. Nothing on a system Skwir reaches rclone's
/// remote control on in no way yet.
fn in_form(
    remotes: &Remotes,
    app: &App,
    done: &'static str,
    work: impl FnOnce(&crate::rclone_forms::Flow<'_>, &Remotes, &App) -> anyhow::Result<()>
    + Send
    + 'static,
) -> anyhow::Result<()> {
    let rclone = skwir_core::os_path::find_program("rclone")
        .ok_or_else(|| anyhow::anyhow!("rclone is not installed"))?;
    let lifetime = remotes
        .lifetime()
        .ok_or_else(|| anyhow::anyhow!("remote folders are turned off"))?;
    let (remotes, weak) = (remotes.clone(), app.weak());
    std::thread::Builder::new()
        .name("skwir-remote-form".into())
        .spawn(move || {
            let Some(app) = weak.upgrade() else { return };
            let cancel = lifetime.child();
            let ask: Arc<dyn crate::connector::Ask> =
                Arc::new(crate::rclone_config::ConfigAsk::new(
                    Arc::new(crate::WindowAsk {
                        app: Arc::downgrade(&app),
                    }),
                    remotes.rclone.keyring(),
                ));
            let result =
                crate::rclone_config::Session::start(&rclone, &remotes.rclone, &ask, &cancel)
                    .map(Arc::new)
                    .and_then(|session| {
                        let flow = crate::rclone_forms::Flow {
                            app: &app,
                            session: &session,
                            cancel: &cancel,
                        };
                        work(&flow, &remotes, &app)
                    });
            remotes.records.changed();
            if let Err(error) = result
                && !cancel.is_cancelled()
            {
                app.notify(
                    NotifyLevel::Error,
                    format!("The server was not {done}: {error:#}"),
                );
            }
        })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_rclone_remote_s_menu_signs_in_again_where_it_signs_in() {
        let titles = |items: Vec<MenuItem>| items.into_iter().map(|i| i.title).collect::<Vec<_>>();
        assert!(titles(menu_items("photos", true)).contains(&"Sign In Again…".to_owned()));
        assert!(!titles(menu_items("nas", false)).contains(&"Sign In Again…".to_owned()));
        let item = &menu_items("it's", false)[0];
        assert_eq!(item.command.args["id"], "it's");
        assert!(signs_in("drive") && !signs_in("s3"));
    }
}
