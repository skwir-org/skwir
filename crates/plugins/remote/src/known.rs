//! The servers the address field offers as an address is typed
//! (ADR-070, ADR-077): those mounted, those the tools' configurations name,
//! the remote bookmarks, and for SFTP the hosts of the user's ssh
//! configuration.

use crate::address::{Address, Scheme};

/// The hosts `config` (an ssh configuration's text) names on its `Host`
/// lines, patterns left out (`*`, `?`, `!`): what `ssh <host>` opens, its
/// own name, user and port as the configuration says.
pub fn ssh_hosts(config: &str) -> Vec<String> {
    let mut hosts = Vec::new();
    for line in config.lines() {
        let line = line.trim();
        let Some((keyword, rest)) = line.split_once(|c: char| c.is_whitespace() || c == '=') else {
            continue;
        };
        if !keyword.eq_ignore_ascii_case("host") {
            continue;
        }
        for host in rest.split_whitespace() {
            let host = host.trim_matches('"');
            if host.is_empty() || host.contains(['*', '?', '!']) {
                continue;
            }
            if !hosts.iter().any(|known| known == host) {
                hosts.push(host.to_owned());
            }
        }
    }
    hosts
}

/// The hosts of the user's ssh configuration (`~/.ssh/config`), read on
/// the caller's thread: a worker's.
pub fn user_ssh_hosts() -> Vec<String> {
    let Some(home) = skwir_core::os_path::home() else {
        return Vec::new();
    };
    std::fs::read_to_string(home.join(".ssh").join("config"))
        .map(|text| ssh_hosts(&text))
        .unwrap_or_default()
}

/// What an address's text after its scheme names of the server, and
/// where that part ends: `//me@ho` is being typed; `//me@host/docs` is
/// past it, its folders the mount's.
pub fn typing_server(rest: &str) -> Option<&str> {
    let after = rest.strip_prefix("//")?;
    (!after.contains('/')).then_some(after)
}

/// The server part of an address as an address writes it, which the
/// field puts in place of what is typed: `me@host:2222`.
pub fn authority(address: &Address) -> String {
    let text = address.to_string();
    let after = text
        .split_once("://")
        .map_or(text.as_str(), |(_, rest)| rest);
    after.split('/').next().unwrap_or(after).to_owned()
}

/// Addresses of `scheme` among `known`, once each by their server.
pub fn of_scheme(scheme: Scheme, known: impl IntoIterator<Item = Address>) -> Vec<Address> {
    let mut out: Vec<Address> = Vec::new();
    for address in known {
        if address.scheme == scheme && !out.iter().any(|a| authority(a) == authority(&address)) {
            out.push(address.top());
        }
    }
    out
}

/// A host of the user's ssh configuration as an SFTP address.
pub fn sftp_of(host: &str) -> Option<Address> {
    Address::parse(&format!("sftp://{host}/")).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_ssh_configurations_hosts_are_offered_without_its_patterns() {
        let config = "\
Host work-server backup
    HostName 192.0.2.10
    User me
Host *.example.com
Host=jump
Match host other
Host !negated *
";
        assert_eq!(ssh_hosts(config), ["work-server", "backup", "jump"]);
    }

    #[test]
    fn a_server_is_completed_until_its_path_begins() {
        assert_eq!(typing_server("//me@ho"), Some("me@ho"));
        assert_eq!(typing_server("//"), Some(""));
        assert_eq!(typing_server("//me@host/docs"), None);
        assert_eq!(typing_server("me@host"), None);
        let a = Address::parse("sftp://me@example.com:2222/home/me").unwrap();
        assert_eq!(authority(&a), "me@example.com:2222");
        let known = of_scheme(
            Scheme::Sftp,
            [
                a.clone(),
                Address::parse("sftp://me@example.com:2222/srv").unwrap(),
                Address::parse("smb://nas/photos").unwrap(),
            ],
        );
        assert_eq!(known.len(), 1, "one per server, of its scheme");
    }
}
