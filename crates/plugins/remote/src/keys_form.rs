//! Key login set up from Skwir, in a form (ADR-086): the user picks a key
//! the ssh agent holds (`KeePassXC`'s, gpg-agent's, a security key's), the
//! default one ssh tries, or a new one Skwir makes; its public half goes
//! up to the server's authorized keys (`keys::install`, the server's
//! password asked by the window once), then a login by key is tried.
//! Only public halves leave the machine; a new key's passphrase is typed
//! in the form, given to ssh-keygen through its askpass.

use std::path::PathBuf;
use std::sync::Arc;

use skwir_core::app::{App, NotifyLevel};
use skwir_core::cancel::CancelToken;
use skwir_core::form::{Answer, Choice, Field, Form, Kind, Values};

use crate::address::Address;
use crate::connector::Ask;
use crate::keys::{self, Key};

/// The choice of a new key.
const NEW: &str = "new";

/// What the form offers: the agent's keys, then the default ones it does
/// not hold already, each by its choice's value.
fn offered(agent: Vec<Key>, defaults: Vec<Key>) -> Vec<(String, Key)> {
    let mut offered: Vec<(String, Key)> = agent
        .into_iter()
        .enumerate()
        .map(|(i, key)| (format!("agent:{i}"), key))
        .collect();
    for (i, key) in defaults.into_iter().enumerate() {
        if !offered.iter().any(|(_, k)| k.same(&key)) {
            offered.push((format!("file:{i}"), key));
        }
    }
    offered
}

/// A key as the form says it: where it is, its type and comment, then
/// its fingerprint.
fn choice(value: &str, key: &Key) -> Choice {
    let kind = key.kind.trim_start_matches("ssh-");
    let said = if key.comment.is_empty() {
        kind.to_owned()
    } else {
        format!("{} ({kind})", key.comment)
    };
    let title = match &key.file {
        Some(file) => format!("{said}, {}", file.display()),
        None => format!("{said}, in the ssh agent"),
    };
    Choice::new(value, title).detail(key.fingerprint.clone())
}

/// The form asking which key, for `top`'s server.
fn form(top: &Address, offered: &[(String, Key)], ssh_dir: &std::path::Path) -> Form {
    let mut choices: Vec<Choice> = offered.iter().map(|(v, k)| choice(v, k)).collect();
    choices.push(Choice::new(NEW, "A new key").detail(format!("Made in {}", ssh_dir.display())));
    let first = offered.first().map_or(NEW, |(v, _)| v.as_str()).to_owned();
    Form::new(format!("Set Up Key Login on “{}”", top.label()), "Set Up")
        .intro("Your key's public half is added to the server's authorized keys: ssh then logs in with it, no password. Its private half never leaves this machine. The server's password is asked once.")
        .field(
            Field::new("key", "Key", Kind::Choice { choices, open: false })
                .value(first)
                .required(),
        )
        .field(
            Field::new("passphrase", "New Key's Passphrase", Kind::Secret { kept: false })
                .help("Encrypts a new key: asked when it is used, unless the agent holds it. Empty for none."),
        )
        .field(Field::new("again", "The Same Again", Kind::Secret { kept: false }))
}

/// The comment a new key is made with: whose it is.
fn comment() -> String {
    let user = std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "me".to_owned());
    format!("{user} (made by Skwir)")
}

/// Ask which key, put it up on `top`'s server, and try a login by it.
///
/// # Errors
/// What ssh, ssh-keygen or the server said.
pub fn set_up(
    app: &App,
    with: &crate::KeySetup,
    top: &Address,
    cancel: &CancelToken,
) -> anyhow::Result<()> {
    let ssh_dir: PathBuf = with
        .ssh_dir
        .clone()
        .or_else(keys::ssh_dir)
        .ok_or_else(|| anyhow::anyhow!("there is no home folder for ssh's keys"))?;
    let offered = offered(
        keys::agent_keys(with.agent.as_deref(), cancel),
        keys::default_keys(&ssh_dir, cancel),
    );
    let blank = form(top, &offered, &ssh_dir);
    let mut asked = blank.clone();
    let values: Values = loop {
        let Some(Answer::Accepted(values)) = app.fill(asked, cancel) else {
            return Ok(());
        };
        if values.get("key") == NEW && values.get("passphrase") != values.get("again") {
            asked = blank.clone().again(&values, "The two passphrases differ");
            continue;
        }
        break values;
    };
    let chosen = values.get("key");
    let key = if chosen == NEW {
        keys::new_key(
            &ssh_dir,
            &comment(),
            values.get("passphrase"),
            &with.ssh,
            cancel,
        )?
    } else {
        offered
            .iter()
            .find(|(v, _)| v == chosen)
            .map(|(_, k)| k.clone())
            .ok_or_else(|| anyhow::anyhow!("no such key"))?
    };
    let ask: Arc<dyn Ask> = Arc::new(crate::WindowAsk { app: app.weak() });
    let added = keys::install(top, std::slice::from_ref(&key), &with.ssh, &ask, cancel)?;
    let label = top.label();
    if keys::logs_in(top, &with.ssh, Some(&ask), cancel) {
        let said = if added == 0 {
            format!("“{label}” had the key already: ssh logs in with it")
        } else {
            format!("Key login is set up on “{label}”")
        };
        app.notify(NotifyLevel::Info, said);
    } else {
        app.notify(
            NotifyLevel::Warning,
            format!("The key is on “{label}”, but ssh does not log in with it: the server may not take keys"),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(blob: &str, comment: &str, file: Option<&str>) -> Key {
        Key {
            file: file.map(PathBuf::from),
            ..Key::parse(&format!("ssh-ed25519 {blob} {comment}")).unwrap()
        }
    }

    #[test]
    fn the_agent_s_keys_come_first_a_default_one_it_holds_once() {
        let agent = vec![key("AAAAC3NzaC1lZDI1NTE5AAAAIA111", "me@laptop", None)];
        let defaults = vec![
            key(
                "AAAAC3NzaC1lZDI1NTE5AAAAIA111",
                "me@laptop",
                Some("/k/id_ed25519"),
            ),
            key("AAAAC3NzaC1lZDI1NTE5AAAAIA222", "", Some("/k/id_ecdsa")),
        ];
        let offered = offered(agent, defaults);
        let values: Vec<&str> = offered.iter().map(|(v, _)| v.as_str()).collect();
        assert_eq!(values, ["agent:0", "file:1"]);
        let form = form(
            &Address::parse("sftp://me@nas/").unwrap(),
            &offered,
            std::path::Path::new("/k"),
        );
        let Kind::Choice { choices, open } = &form.fields[0].kind else {
            panic!("a choice of keys");
        };
        assert!(!open);
        assert_eq!(choices[0].title, "me@laptop (ed25519), in the ssh agent");
        assert_eq!(choices[1].title, "ed25519, /k/id_ecdsa");
        assert_eq!(choices[2].value, NEW);
        assert_eq!(form.fields[0].value, "agent:0", "the agent's first");
    }
}
