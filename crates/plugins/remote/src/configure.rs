//! Adding, editing, testing, signing in again and removing servers
//! (ADR-086): rclone's remotes, in rclone's own configuration, through
//! forms the window draws (`rclone_forms`), and the servers added by their
//! address, in Skwir's list or KDE's network folders (`servers`,
//! `servers_form`). Every surface runs the same commands: Add Server… in
//! the Network section's menu and the palette, a server's own menu, the
//! list in Settings (every server, each kept by its owner,
//! `RecordSource`), an agent or a script. A form is what the user
//! reviews: a caller gives what it starts from, never a secret, which the
//! user types in the window.

use std::collections::BTreeMap;
use std::sync::Arc;

use skwir_core::app::{App, NotifyLevel};
use skwir_core::command::{Invocation, Outcome, Topic};
use skwir_core::registry::MenuItem;
use skwir_core::settings::{RecordField, SettingSpec};

use crate::address::{Address, Scheme};
use crate::{ID, Remotes};

/// Every server in Settings, each kept by its owner: rclone's remotes,
/// those added by their address, those other programs name.
pub(crate) const RECORDS: &str = "remote.servers";

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
    /// The pane a server added by its address opens in: the active one,
    /// as the command is asked, where none is given.
    #[serde(default)]
    pane: Option<skwir_core::workspace::PaneId>,
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

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct ServerArgs {
    /// The server: a remote's name in rclone's configuration (`photos`),
    /// or the address of one added by its address (`sftp://me@nas/`).
    id: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct LocationArgs {
    /// The place's location, as the sidebar has it.
    location: String,
    /// The pane the menu opens in, which a menu gives every source.
    #[serde(default, rename = "pane")]
    _pane: Option<skwir_core::workspace::PaneId>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct KeepArgs {
    /// The address of the remote folder to keep as a server.
    address: String,
    /// What the Network section calls it: the bookmark's name, else its
    /// address's own.
    #[serde(default)]
    name: Option<String>,
}

/// The server an `id` names: an address for one added by its address,
/// `None` for a remote of rclone's (its name).
fn by_address(id: &str) -> anyhow::Result<Option<Address>> {
    if id.contains("://") {
        Ok(Some(Address::parse(id.trim())?))
    } else {
        Ok(None)
    }
}

/// The items of a server added by its address in its menu: Edit, Remove.
pub(crate) fn kept_items(address: &Address) -> Vec<MenuItem> {
    let id = serde_json::json!({ "id": address.to_string() });
    vec![
        MenuItem::new(
            "place",
            "Edit Server…",
            "settings",
            410,
            Invocation::with_args("remote.edit_server", &id),
        ),
        MenuItem::new(
            "place",
            "Remove Server…",
            "trash",
            290,
            Invocation::with_args("remote.remove_server", &id),
        ),
    ]
}

/// What a server's line in Settings does beside Edit and Remove, as its
/// menu does: Test (rclone's), Mount Options, Connect at Login, Set Up Key
/// Login, Sign In Again, Forget Password where Skwir keeps one
/// (`remembered`, by key).
fn record_actions(
    server: &crate::connector::Server,
    at_login: &std::collections::HashSet<String>,
    remembered: &[String],
    unfinished: &[String],
) -> Vec<serde_json::Value> {
    let action = |label: &str, command: &str, args: serde_json::Value| serde_json::json!({ "label": label, "command": command, "args": args });
    let top = server.address.top();
    let mut actions = Vec::new();
    if let Some(name) = remote_name(&server.address).filter(|n| unfinished.iter().any(|u| u == n)) {
        actions.push(action(
            FINISH,
            "remote.edit_server",
            serde_json::json!({ "id": name }),
        ));
    }
    if let Some(name) = remote_name(&server.address) {
        actions.push(action(
            "Test",
            "remote.test_server",
            serde_json::json!({ "id": name }),
        ));
    }
    if crate::options::takes_options(top.scheme) {
        actions.push(action(
            "Mount Options…",
            "remote.mount_options",
            serde_json::json!({ "address": top.to_string() }),
        ));
    }
    if let Some(id) = login_id(&top) {
        let on = at_login.contains(&id);
        actions.push(action(
            if on {
                "Connect at Login ✓"
            } else {
                "Connect at Login"
            },
            "remote.connect_at_login",
            serde_json::json!({ "address": top.to_string(), "on": !on }),
        ));
    }
    if top.scheme == Scheme::Sftp {
        actions.push(action(
            "Set Up Key Login…",
            "remote.setup_key_login",
            serde_json::json!({ "address": top.to_string() }),
        ));
    }
    if remote_name(&server.address).is_some() && signs_in(&server.kind) {
        actions.push(action(
            "Sign In Again…",
            "remote.reconnect",
            serde_json::json!({ "id": server.address.host }),
        ));
    }
    let key = crate::passwords::key(
        &server.address,
        crate::passwords::user_of(&server.address).as_deref(),
    );
    if remembered.contains(&key) {
        actions.push(action(
            "Forget Password",
            "remote.forget_password",
            serde_json::json!({ "server": key }),
        ));
    }
    actions
}

/// The items of an rclone remote's menu in the sidebar: Edit, Sign In
/// Again where it signs in through a browser (`signs_in`), Remove; first,
/// Finish Setting Up where Skwir did not finish it (`unfinished`).
pub(crate) fn menu_items(name: &str, signs_in: bool, unfinished: bool) -> Vec<MenuItem> {
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
            290,
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
    if unfinished {
        items.push(MenuItem::new(
            "place",
            FINISH,
            "settings",
            405,
            named("remote.edit_server"),
        ));
    }
    items
}

/// What finishes setting up a remote of rclone's Skwir did not finish
/// (`rclone_forms::UNFINISHED`): its Edit, which asks rclone's questions
/// again.
const FINISH: &str = "Finish Setting Up…";

/// What a remote of rclone's Skwir did not finish setting up says of
/// itself where it is listed.
pub(crate) const NOT_FINISHED: &str = "not finished setting up";

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
        items.push(
            MenuItem::new(
                "place",
                "Connect at Login",
                "",
                430,
                Invocation::with_args(
                    "remote.connect_at_login",
                    &serde_json::json!({ "address": address.top().to_string(), "on": !on }),
                ),
            )
            .checked(on),
        );
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
            "Servers",
            "The servers of the sidebar's Network section, each changed where it is kept. Those added by their address are in Skwir's list or KDE's network folders; rclone's remotes in rclone's own configuration, where rclone in a terminal finds them too.",
            "Server",
            vec![
                RecordField::text("name", "Name", "", ""),
                RecordField::text("kind", "Kind", "", ""),
            ],
        )
        .records_kept_by(remotes.records.clone())
        .page(skwir_core::settings::page::NETWORK)
        .section("Servers"),
    );
    crate::servers::declare(app);
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
            ID, Topic::Places,
            "remote.server_records",
            "Servers",
            "Every server as Settings lists it: each one's name, kind and where it is kept, what its line does, and whether Skwir changes it (`fixed`: another program does)",
            move |_, app| {
                // What the lines say reads the user database (a password's
                // key): not under the state's lock.
                let (servers, at_login) = {
                    let state = remotes_.state.lock();
                    (state.servers.clone(), state.at_login.clone())
                };
                let remembered = crate::remembered(app);
                let unfinished = crate::rclone_forms::unfinished(app);
                let records: Vec<serde_json::Value> = servers
                    .iter()
                    .map(|server| {
                        let (id, name, kind) = match remote_name(&server.address) {
                            Some(name) if unfinished.iter().any(|u| u == name) => (
                                name.to_owned(),
                                name.to_owned(),
                                format!("{} · {} · {NOT_FINISHED}", server.kind, server.kept.said()),
                            ),
                            Some(name) => (
                                name.to_owned(),
                                name.to_owned(),
                                format!("{} · {}", server.kind, server.kept.said()),
                            ),
                            None => (
                                server.address.to_string(),
                                server.label.clone(),
                                format!("{} · {}", server.address, server.kept.said()),
                            ),
                        };
                        let fixed =
                            remote_name(&server.address).is_none() && !server.kept.by_address();
                        serde_json::json!({
                            "id": id,
                            "name": name,
                            "kind": kind,
                            "actions": record_actions(server, &at_login, &remembered, &unfinished),
                            "fixed": fixed,
                        })
                    })
                    .collect();
                Ok(Outcome::Value(serde_json::json!({ "records": records })))
            },
        )
        .data();
    let remotes_ = remotes.clone();
    app.commands
        .register::<AddArgs, _>(
            ID, Topic::Places,
            "remote.add_server",
            "Add server…",
            "Add a server: a remote of rclone's (cloud storage, S3, WebDAV, SFTP and seventy more), in a form the user fills; what it starts from may be given, never a secret",
            move |args, app| {
                window_for_forms(app)?;
                // Resolved now: the form may end long after, another pane
                // active by then.
                let pane = app.pane_or_active(args.pane);
                let prefill = crate::rclone_forms::Prefill {
                    kind: args.kind,
                    provider: args.provider,
                    name: args.name,
                    options: args.options,
                };
                // Without rclone, the servers added by their address alone.
                if skwir_core::os_path::find_program("rclone").is_none() {
                    return servers_alone(&remotes_, app, prefill, pane);
                }
                in_form(&remotes_, app, "added", move |flow, remotes, app| {
                    let made = flow.add(&prefill, pane)?;
                    if let Some(crate::rclone_forms::Made::Server(name)) = &made {
                        remotes.refresh(app);
                        remotes.records.changed();
                        app.notify(NotifyLevel::Info, format!("“{name}” is added"));
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
        .register::<ServerArgs, _>(
            ID,
            Topic::Places,
            "remote.edit_server",
            "Edit server…",
            "Change a server in its form: a remote of rclone's, or one added by its address",
            move |args, app| {
                window_for_forms(app)?;
                if let Some(address) = by_address(&args.id)? {
                    return on_form_thread(&remotes_, app, "saved", move |remotes, app, cancel| {
                        edit_kept(remotes, app, &address, cancel)
                    });
                }
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
            ID, Topic::Places,
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
        .register::<ServerArgs, _>(
            ID, Topic::Places,
            "remote.remove_server",
            "Remove server…",
            "Remove a server once the user confirms, where it is kept (rclone's configuration, Skwir's list, KDE's network folders), with its mount options and its entry connecting it at login: what it holds stays where it is",
            move |args, app| {
                window_for_forms(app)?;
                if let Some(address) = by_address(&args.id)? {
                    return on_form_thread(&remotes_, app, "removed", move |remotes, app, cancel| {
                        remove_kept(remotes, app, &address, cancel)
                    });
                }
                in_form(&remotes_, app, "removed", move |flow, remotes, app| {
                    let address = Address::rclone(&args.id, b"/")?;
                    let mounted: Vec<std::path::PathBuf> = remotes
                        .mounted()
                        .into_iter()
                        .filter(|m| m.recognised.address.as_ref() == Some(&address))
                        .map(|m| m.remote.point.clone())
                        .collect();
                    if !flow.confirm_remove(&args.id, !mounted.is_empty()) {
                        return Ok(());
                    }
                    // Let go of first, and kept while it stays mounted: what
                    // rclone mounted keeps working from what it read of the
                    // configuration.
                    for point in mounted {
                        remotes
                            .disconnect_here(app, &point, false)
                            .map_err(|error| anyhow::anyhow!("it stays mounted: {error:#}"))?;
                    }
                    flow.session.delete(&args.id, flow.cancel)?;
                    crate::rclone_forms::note_unfinished(app, &args.id, false);
                    forget_kept_for(remotes, app, &address, flow.cancel);
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
            ID, Topic::Places,
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
            ID, Topic::Places,
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
        ID, Topic::Places,
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
            ID, Topic::Places,
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
                let (remotes, weak) = (remotes_.clone(), app.weak());
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
                                    // Its entry at login mounts so too.
                                    let kept = crate::options::keep(&app, &top, &options)
                                        .map_err(|error| {
                                            format!("The mount options were not saved: {error:#}")
                                        })
                                        .and_then(|()| {
                                            renew_at_login(&remotes, &top, &options, &cancel)
                                                .map_err(|error| {
                                                    format!(
                                                        "The mount options were saved, but \
                                                         connecting at login mounts as before: \
                                                         {error:#}"
                                                    )
                                                })
                                        });
                                    match kept {
                                        Ok(_) => app.notify(
                                            NotifyLevel::Info,
                                            format!(
                                                "“{}” is mounted so when it next connects",
                                                top.label()
                                            ),
                                        ),
                                        Err(said) => app.notify(NotifyLevel::Error, said),
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
        ID, Topic::Places,
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
                    let done = at_login(&remotes, &app, &top, &id, args.on, &cancel);
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
            ID, Topic::Places,
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
        Topic::Places,
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
    // A remote folder bookmarked (another file manager's, or one the user
    // kept before it was a server) made a server of the Network section.
    let remotes_ = remotes.clone();
    app.commands
        .register::<LocationArgs, _>(
            ID, Topic::Places,
            "remote.keep_as_server_menu",
            "Offer Keep as Server in a place's menu",
            "A sidebar place's Keep as Server, for a remote folder's address that is no server yet (a bookmark's). Answers from memory",
            move |args, _| {
                let items: Vec<MenuItem> = Address::parse(&args.location)
                    .ok()
                    .filter(|a| !matches!(a.scheme, Scheme::Rclone | Scheme::S3))
                    .filter(|a| {
                        !remotes_
                            .state
                            .lock()
                            .servers
                            .iter()
                            .any(|s| s.address == *a)
                    })
                    .map(|address| {
                        MenuItem::new(
                            "place",
                            "Keep as Server",
                            "",
                            415,
                            Invocation::new("remote.keep_as_server")
                                .with("address", address.to_string()),
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
            menu: "place".into(),
            when: None,
            command: Invocation::new("remote.keep_as_server_menu"),
            replaces: Vec::new(),
        },
    );
    // A server found nearby (ADR-084): added as one, its form filled in
    // with what it announced.
    let remotes_ = remotes.clone();
    app.commands
        .register::<PaneArg, _>(
            ID, Topic::Places,
            "remote.nearby_menu",
            "Nearby server's menu",
            "The context menu's items for the one server selected in the Nearby place: Add as Server…, its form filled in with the name and address it announced, where no server holds that address. Answers from memory",
            move |args, app| {
                let pane = app.pane_or_active(args.pane);
                let items: Vec<MenuItem> = nearby_selected(app, pane)
                    .filter(|(address, _)| {
                        !remotes_
                            .state
                            .lock()
                            .servers
                            .iter()
                            .any(|s| s.address.below(address).is_some())
                    })
                    .map(|(address, name)| {
                        MenuItem::new(
                            "context",
                            "Add as Server…",
                            "",
                            410,
                            Invocation::with_args("remote.add_server", &add_found(&address, &name)),
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
            when: Some("place".into()),
            command: Invocation::new("remote.nearby_menu"),
            replaces: Vec::new(),
        },
    );
    let remotes_ = remotes.clone();
    app.commands
        .register::<KeepArgs, _>(
            ID, Topic::Places,
            "remote.keep_as_server",
            "Keep as server",
            "Make a remote folder's address a server of the Network section, kept in Skwir's list or KDE's network folders, under its bookmark's name; its bookmark, if it has one, is taken out",
            move |args, app| {
                let address = Address::parse(args.address.trim())?;
                anyhow::ensure!(
                    !matches!(address.scheme, Scheme::Rclone | Scheme::S3),
                    "{} is kept by rclone or the AWS tools",
                    address.scheme.title()
                );
                let name = args.name;
                on_form_thread(&remotes_, app, "kept", move |remotes, app, _| {
                    keep_as_server(remotes, app, &address, name)
                })
            },
        )
        .shows();
}

/// The one server selected in `pane` where it shows the Nearby place: its
/// address and the name it announced (`nearby::ANNOUNCED`).
fn nearby_selected(app: &App, pane: skwir_core::workspace::PaneId) -> Option<(Address, String)> {
    let ws = app.workspace.read();
    let p = ws.pane(pane)?;
    if p.location()?.scheme.as_ref() != crate::nearby::SCHEME {
        return None;
    }
    let mut selected = p.selection.iter().filter(|id| id.is_place());
    let (id, None) = (selected.next()?, selected.next()) else {
        return None;
    };
    let name = p
        .entry(id)?
        .metadata
        .value(crate::nearby::ANNOUNCED)?
        .as_str()?
        .to_owned();
    let address = Address::of_location(&p.location_of(id)?).ok()?;
    Some((address, name))
}

/// `remote.add_server`'s arguments for a server found at `address`,
/// announced as `name`: its kind, and its form's parts as it is reached.
fn add_found(address: &Address, name: &str) -> serde_json::Value {
    let mut options = serde_json::Map::new();
    let path = String::from_utf8_lossy(&address.path);
    for (key, value) in [
        ("host", Some(address.host.clone())),
        ("port", address.port.map(|p| p.to_string())),
        ("path", Some(path.trim_start_matches('/').to_owned())),
        ("user", address.user.clone()),
        ("domain", address.domain.clone()),
        ("title", Some(name.to_owned())),
    ] {
        if let Some(value) = value.filter(|v| !v.is_empty()) {
            options.insert(key.to_owned(), value.into());
        }
    }
    serde_json::json!({
        "kind": format!("address:{}", address.scheme.name()),
        "options": options,
    })
}

/// Keep the remote folder at `address` as a server, named `name` or as its
/// bookmark is, and take its bookmark out (the places plugin's): it is the
/// Network section's from now on.
fn keep_as_server(
    remotes: &Remotes,
    app: &App,
    address: &Address,
    name: Option<String>,
) -> anyhow::Result<()> {
    let bookmark = skwir_platform::places::remote_bookmarks()
        .into_iter()
        .find(|b| Address::parse(&b.address).is_ok_and(|a| a == *address));
    let name = name
        .or_else(|| bookmark.as_ref().map(|b| b.title.clone()))
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| address.label());
    let kept = crate::servers::add(app, &name, address)?;
    remotes.refresh(app);
    // Its bookmark out first, which the places plugin says: what the user
    // did is said last.
    if let Some(bookmark) = bookmark {
        app.dispatch(&Invocation::with_args(
            "places.unbookmark_address",
            &serde_json::json!({ "address": bookmark.address }),
        ))
        .map_err(|error| anyhow::anyhow!("its bookmark stays: {error:#}"))?;
    }
    app.notify(
        NotifyLevel::Info,
        format!(
            "“{name}” is a server of the Network section, kept {}",
            kept.said()
        ),
    );
    Ok(())
}

/// Set the server at `top` to connect at login (`on`), or no longer: its
/// entry `id` with the service manager, mounting with the options chosen
/// for the server.
fn at_login(
    remotes: &Remotes,
    app: &App,
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
    let at = login_entry(remotes, top, &crate::options::of(app, top), cancel)?;
    manager.install(&at, cancel)
}

/// What connects the server at `top` at login, mounting it with `options`.
/// Logs in to an SFTP server once: on a worker.
fn login_entry(
    remotes: &Remotes,
    top: &Address,
    options: &crate::options::MountOptions,
    cancel: &skwir_core::cancel::CancelToken,
) -> anyhow::Result<crate::login::AtLogin> {
    if top.scheme == Scheme::Rclone {
        crate::login::AtLogin::rclone(&remotes.rclone, top, options, cancel)
    } else {
        crate::login::AtLogin::sshfs(&remotes.sshfs(), top, options, cancel)
    }
}

/// Write again the entry connecting `top`'s server at login, where there
/// is one: what it mounts with changed (`options`). A mount it carries
/// stays as it was made.
fn renew_at_login(
    remotes: &Remotes,
    top: &Address,
    options: &crate::options::MountOptions,
    cancel: &skwir_core::cancel::CancelToken,
) -> anyhow::Result<bool> {
    let Some(id) = login_id(top) else {
        return Ok(false);
    };
    if !remotes.state.lock().at_login.contains(&id) {
        return Ok(false);
    }
    let at = login_entry(remotes, top, options, cancel)?;
    remotes.login_manager()?.renew(&at, cancel)?;
    Ok(true)
}

/// Add Server where rclone is not installed: the servers added by their
/// address alone, on a thread of its own.
fn servers_alone(
    remotes: &Remotes,
    app: &App,
    prefill: crate::rclone_forms::Prefill,
    pane: skwir_core::workspace::PaneId,
) -> anyhow::Result<Outcome> {
    on_form_thread(remotes, app, "added", move |remotes, app, cancel| {
        // A kind given (Nearby's Add as Server), its form as given.
        let given = prefill
            .kind
            .as_deref()
            .and_then(crate::servers_form::scheme_of);
        let added = match given {
            Some(scheme) => {
                match crate::servers_form::add(app, scheme, &prefill.options, false, pane, cancel)?
                {
                    crate::servers_form::Added::Done(name) => Some(name),
                    crate::servers_form::Added::Back | crate::servers_form::Added::Cancelled => {
                        None
                    }
                }
            }
            None => crate::servers_form::add_any(app, pane, cancel)?,
        };
        if let Some(name) = added {
            remotes.refresh(app);
            app.notify(NotifyLevel::Info, format!("“{name}” is added"));
        }
        Ok(())
    })
}

/// Run `work`, which may fill forms in, on a thread of its own: what
/// failed is said, as what was to be `done` that was not; Settings' list
/// is read again after.
fn on_form_thread(
    remotes: &Remotes,
    app: &App,
    done: &'static str,
    work: impl FnOnce(&Remotes, &App, &skwir_core::cancel::CancelToken) -> anyhow::Result<()>
    + Send
    + 'static,
) -> anyhow::Result<Outcome> {
    let lifetime = remotes
        .lifetime()
        .ok_or_else(|| anyhow::anyhow!("remote folders are turned off"))?;
    let (remotes, weak) = (remotes.clone(), app.weak());
    std::thread::Builder::new()
        .name("skwir-remote-form".into())
        .spawn(move || {
            let Some(app) = weak.upgrade() else { return };
            let cancel = lifetime.child();
            let result = work(&remotes, &app, &cancel);
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
    Ok(Outcome::Done)
}

/// The server added by its address at `address`, as kept.
fn kept_at(remotes: &Remotes, address: &Address) -> anyhow::Result<crate::connector::Server> {
    remotes
        .state
        .lock()
        .servers
        .iter()
        .find(|s| s.address == *address)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("no server is known at {address}"))
        .and_then(|server| {
            anyhow::ensure!(
                server.kept.by_address(),
                "“{}” is {}: change it there",
                server.label,
                server.kept.said()
            );
            Ok(server)
        })
}

/// Change the server added by its address at `address` in its form,
/// where it is kept: its mount options follow it to its new address; its
/// entry connecting it at login, made for the old one, is taken out.
fn edit_kept(
    remotes: &Remotes,
    app: &App,
    address: &Address,
    cancel: &skwir_core::cancel::CancelToken,
) -> anyhow::Result<()> {
    let server = kept_at(remotes, address)?;
    let Some((name, new)) = crate::servers_form::edit(app, &server, cancel)? else {
        return Ok(());
    };
    crate::servers::change(app, address, &name, &new)?;
    let (old_top, new_top) = (address.top(), new.top());
    if old_top != new_top {
        let options = crate::options::of(app, &old_top);
        crate::options::keep(app, &new_top, &options)?;
        crate::options::keep(app, &old_top, &crate::options::MountOptions::default())?;
        if login_off(remotes, app, &old_top, cancel)? {
            app.notify(
                NotifyLevel::Info,
                format!("“{name}” no longer connects at login: its address changed"),
            );
        }
    }
    remotes.refresh(app);
    app.notify(NotifyLevel::Info, format!("“{name}” is saved"));
    Ok(())
}

/// Remove the server added by its address at `address`, once the user
/// confirms, where it is kept, with what Skwir kept for it.
fn remove_kept(
    remotes: &Remotes,
    app: &App,
    address: &Address,
    cancel: &skwir_core::cancel::CancelToken,
) -> anyhow::Result<()> {
    use skwir_core::form::{Answer, Form};
    let server = kept_at(remotes, address)?;
    let password = if password_of(remotes, app, address).is_some() {
        ", and the password it keeps for it in the keyring"
    } else {
        ""
    };
    let form = Form::new(format!("Remove “{}”?", server.label), "Remove").intro(format!(
        "Skwir forgets the server, kept {}{password}: what it holds stays where it is, and adding it again brings it back.",
        server.kept.said()
    ));
    if !matches!(app.fill(form, cancel), Some(Answer::Accepted(_))) {
        return Ok(());
    }
    crate::servers::remove(app, address)?;
    forget_kept_for(remotes, app, address, cancel);
    remotes.refresh(app);
    app.notify(NotifyLevel::Info, format!("“{}” is removed", server.label));
    Ok(())
}

/// The key of the password Skwir keeps in the keyring for `address`'s
/// server, where it keeps one no other server kept needs (the same server,
/// logged in as the same user). Reads the user database: on a worker.
fn password_of(remotes: &Remotes, app: &App, address: &Address) -> Option<String> {
    let key_of = |a: &Address| crate::passwords::key(a, crate::passwords::user_of(a).as_deref());
    let key = key_of(address);
    if !crate::remembered(app).contains(&key) {
        return None;
    }
    // Those kept by address as they are now (a removal just made is not
    // yet in the state, refreshed after), the tools' as last read.
    let tools: Vec<Address> = remotes
        .state
        .lock()
        .servers
        .iter()
        .filter(|s| !s.kept.by_address())
        .map(|s| s.address.clone())
        .collect();
    let kept = crate::servers::listed(app).into_iter().map(|s| s.address);
    let mut others = kept.chain(tools).filter(|a| a != address);
    (!others.any(|a| key_of(&a) == key)).then_some(key)
}

/// What Skwir kept for the server at `address`, once it is removed: the
/// password it keeps for it in the keyring, where no other server kept
/// needs it; its mount options and its entry connecting it at login,
/// where no other server kept shares its top. What fails is said, the
/// removal done.
fn forget_kept_for(
    remotes: &Remotes,
    app: &App,
    address: &Address,
    cancel: &skwir_core::cancel::CancelToken,
) {
    if let Some(key) = password_of(remotes, app, address) {
        match remotes.keyring.forget(&key) {
            Ok(()) => remotes.note_remembered(app, &crate::passwords::Noted::Forgot(key)),
            Err(error) => app.notify(
                NotifyLevel::Warning,
                format!(
                    "The password Skwir keeps for {} stays in the keyring: {error:#}",
                    address.top().label()
                ),
            ),
        }
    }
    let top = address.top();
    let shared = remotes
        .state
        .lock()
        .servers
        .iter()
        .any(|s| s.address != *address && s.address.top() == top);
    if shared {
        return;
    }
    let forgot = crate::options::keep(app, &top, &crate::options::MountOptions::default())
        .and_then(|()| login_off(remotes, app, &top, cancel).map(|_| ()));
    if let Err(error) = forgot {
        app.notify(
            NotifyLevel::Warning,
            format!("What Skwir kept for {} stays: {error:#}", top.label()),
        );
    }
}

/// Take out the entry connecting `top`'s server at login, where there is
/// one: whether there was.
fn login_off(
    remotes: &Remotes,
    app: &App,
    top: &Address,
    cancel: &skwir_core::cancel::CancelToken,
) -> anyhow::Result<bool> {
    let Some(id) = login_id(top) else {
        return Ok(false);
    };
    if !remotes.state.lock().at_login.contains(&id) {
        return Ok(false);
    }
    at_login(remotes, app, top, &id, false, cancel)?;
    Ok(true)
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
    connecting_at_login_alone(remotes)?;
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
    // One turned on meanwhile.
    connecting_at_login_alone(remotes)?;
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

/// Refused while a remote of rclone's connects at login: its entry would
/// fail at every login, nobody being there to give the password
/// (`AtLogin::rclone` refuses an encrypted configuration).
fn connecting_at_login_alone(remotes: &Remotes) -> anyhow::Result<()> {
    // `login::id_of`: the connector, then the address.
    let mut connecting: Vec<String> = remotes
        .state
        .lock()
        .at_login
        .iter()
        .filter_map(|id| id.strip_prefix("rclone/"))
        .filter_map(|address| Address::parse(address).ok())
        .map(|address| format!("“{}”", address.label()))
        .collect();
    connecting.sort();
    anyhow::ensure!(
        connecting.is_empty(),
        "{} {} at login, which an encrypted configuration cannot: nobody is there to \
         give its password. Turn Connect at Login off first",
        connecting.join(", "),
        if connecting.len() == 1 {
            "connects"
        } else {
            "connect"
        }
    );
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
        assert!(titles(menu_items("photos", true, false)).contains(&"Sign In Again…".to_owned()));
        assert!(!titles(menu_items("nas", false, false)).contains(&"Sign In Again…".to_owned()));
        assert!(titles(menu_items("nas", false, true)).contains(&FINISH.to_owned()));
        assert!(!titles(menu_items("nas", false, false)).contains(&FINISH.to_owned()));
        let item = &menu_items("it's", false, false)[0];
        assert_eq!(item.command.args["id"], "it's");
        assert!(signs_in("drive") && !signs_in("s3"));
    }
}
