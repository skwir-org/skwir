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
use skwir_core::words;
use skwir_core::words::Words;

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
    // Its comment, type and file are ssh's: as written.
    let title = match &key.file {
        Some(file) => Words::as_written(format!("{said}, {}", file.display())),
        None => words!(
            remote_keys_in_agent = "{ $key }, in the ssh agent",
            key = said,
            note = "A key the ssh agent holds: key is its comment and type (me@laptop (ed25519))."
        ),
    };
    Choice::new(value, title).detail(key.fingerprint.clone())
}

/// The form asking which key, for `top`'s server: a new one too where
/// one can be made in `ssh_dir` (`new`: none is there of its name).
fn form(top: &Address, offered: &[(String, Key)], ssh_dir: &std::path::Path, new: bool) -> Form {
    let mut choices: Vec<Choice> = offered.iter().map(|(v, k)| choice(v, k)).collect();
    if new {
        choices.push(
            Choice::new(NEW, words!(remote_keys_new = "A new key")).detail(words!(
                remote_keys_new_made_in = "Made in { $folder }",
                folder = ssh_dir.display().to_string(),
                note = "Where a new key is made: folder is a path (~/.ssh)."
            )),
        );
    }
    let first = offered.first().map_or(NEW, |(v, _)| v.as_str()).to_owned();
    let form = Form::new(
        words!(
            remote_keys_form_title = "Set Up Key Login on { $server }",
            server = crate::address::named(&top.label()),
            note = "server: the login and server a key is set up for (me@nas)."
        ),
        words!(remote_keys_form_accept = "Set Up"),
    )
    .intro(words!(
        remote_keys_form_intro = "Your key's public half is added to the server's authorized keys: ssh then logs in with it, no password. Its private half never leaves this machine. The server's password is asked once."
    ))
    .field(
        Field::new(
            "key",
            words!(remote_keys_form_key = "Key"),
            Kind::Choice {
                choices,
                open: false,
            },
        )
        .value(first)
        .required(),
    );
    if !new {
        return form;
    }
    form.field(
        Field::new(
            "passphrase",
            words!(remote_keys_form_passphrase = "New Key's Passphrase"),
            Kind::Secret { kept: false },
        )
        .help(words!(
            remote_keys_form_passphrase_help = "Encrypts a new key: asked when it is used, unless the agent holds it. Empty for none."
        )),
    )
    .field(Field::new(
        "again",
        words!(
            remote_keys_form_again = "The Same Again",
            note = "The field where a new key's passphrase is typed a second time."
        ),
        Kind::Secret { kept: false },
    ))
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
    let ssh_dir: PathBuf = with.ssh_dir.clone().or_else(keys::ssh_dir).ok_or_else(|| {
        anyhow::anyhow!(words!(
            remote_keys_no_home = "there is no home folder for ssh's keys"
        ))
    })?;
    let offered = offered(
        keys::agent_keys(with.agent.as_deref(), cancel),
        keys::default_keys(&ssh_dir, cancel),
    );
    let new = keys::makes_new_key(&ssh_dir);
    anyhow::ensure!(
        new || !offered.is_empty(),
        words!(
            remote_keys_none_offered =
                "ssh offers no key, and a new one would be made over the one at { $path }",
            path = ssh_dir.join("id_ed25519").display().to_string(),
            note = "path: where ssh's default key is."
        )
    );
    let blank = form(top, &offered, &ssh_dir, new);
    let mut asked = blank.clone();
    let values: Values = loop {
        let Some(Answer::Accepted(values)) = app.fill(asked, cancel) else {
            return Ok(());
        };
        if values.get("key") == NEW && values.get("passphrase") != values.get("again") {
            asked = blank.clone().again(
                &values,
                words!(remote_keys_passphrases_differ = "The two passphrases differ"),
            );
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
            .ok_or_else(|| anyhow::anyhow!(words!(remote_keys_no_such_key = "no such key")))?
    };
    let ask: Arc<dyn Ask> = Arc::new(crate::WindowAsk { app: app.weak() });
    let added = keys::install(top, std::slice::from_ref(&key), &with.ssh, &ask, cancel)?;
    let label = crate::address::named(&top.label());
    if keys::logs_in(top, &with.ssh, Some(&ask), cancel) {
        let said = if added == 0 {
            words!(
                remote_keys_had_it = "{ $server } had the key already: ssh logs in with it",
                server = label,
                note = "server: the login and server (me@nas)."
            )
        } else {
            words!(
                remote_keys_set_up = "Key login is set up on { $server }",
                server = label,
                note = "server: the login and server (me@nas)."
            )
        };
        app.notify(NotifyLevel::Info, said);
    } else {
        app.notify(
            NotifyLevel::Warning,
            words!(
                remote_keys_not_taken = "The key is on { $server }, but ssh does not log in with it: the server may not take keys",
                server = label,
                note = "server: the login and server (me@nas)."
            ),
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
        let top = Address::parse("sftp://me@nas/").unwrap();
        let form = form(&top, &offered, std::path::Path::new("/k"), true);
        let Kind::Choice { choices, open } = &form.fields[0].kind else {
            panic!("a choice of keys");
        };
        assert!(!open);
        assert_eq!(
            choices[0].title.english(),
            "me@laptop (ed25519), in the ssh agent"
        );
        assert_eq!(choices[1].title.english(), "ed25519, /k/id_ecdsa");
        assert_eq!(choices[2].value, NEW);
        assert_eq!(form.fields[0].value, "agent:0", "the agent's first");
        // One there of its name already: no new key offered, nor asked
        // a passphrase for.
        let form = super::form(&top, &offered, std::path::Path::new("/k"), false);
        let Kind::Choice { choices, .. } = &form.fields[0].kind else {
            panic!("a choice of keys");
        };
        assert!(!choices.iter().any(|c| c.value == NEW));
        assert_eq!(form.fields.len(), 1, "no passphrase");
        let folder = tempfile::tempdir().unwrap();
        assert!(keys::makes_new_key(folder.path()));
        std::fs::write(folder.path().join("id_ed25519.pub"), "a key").unwrap();
        assert!(!keys::makes_new_key(folder.path()));
    }
}
