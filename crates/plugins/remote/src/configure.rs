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
struct NamedArgs {
    /// The remote's name in rclone's configuration.
    id: String,
}

/// The items of an rclone remote's menu in the sidebar: Edit, Sign In
/// Again where it signs in through a browser (`signs_in`), Remove.
pub(crate) fn menu_items(name: &str, signs_in: bool) -> Vec<MenuItem> {
    let named = |command: &str| Invocation::new(command).with("id", name);
    let mut items = vec![
        MenuItem::new("place", "Edit Server…", "settings", 410, named("remote.edit_server")),
        MenuItem::new("place", "Test Server", "", 411, named("remote.test_server")),
        MenuItem::new("place", "Remove Server…", "trash", 310, named("remote.remove_server")),
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
            "rclone remotes",
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
                let prefill = crate::rclone_forms::Prefill {
                    kind: args.kind,
                    provider: args.provider,
                    name: args.name,
                    options: args.options,
                };
                in_form(&remotes_, app, "added", move |flow, remotes, app| {
                    if let Some(name) = flow.add(&prefill)? {
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
        app.notify(NotifyLevel::Info, "rclone's configuration is encrypted already");
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
            let ask: Arc<dyn crate::connector::Ask> = Arc::new(crate::rclone_config::ConfigAsk::new(
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
