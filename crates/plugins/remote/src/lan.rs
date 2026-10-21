//! Whether a server is on the local network: what a remote's thumbnails
//! follow when the user chose none for it (ADR-077: Automatic is Always on
//! the local network, Never when that cannot be told).

use std::net::{IpAddr, ToSocketAddrs};
use std::time::Duration;

/// How long a name may take to resolve before it is taken as not known.
const RESOLVING: Duration = Duration::from_secs(2);

/// What `host` says of itself, nothing asked: an address, or a name the
/// local network gives ([`local`]); `None` for a name to resolve.
pub fn said(host: &str) -> Option<bool> {
    let host = host.trim_start_matches('[').trim_end_matches(']');
    if let Ok(ip) = host.parse::<IpAddr>() {
        return Some(private(ip));
    }
    let name = host.trim_end_matches('.').to_ascii_lowercase();
    (name == "localhost"
        || [".local", ".lan", ".home.arpa", ".localdomain"]
            .iter()
            .any(|suffix| name.ends_with(suffix)))
    .then_some(true)
}

/// Whether `host` (a name, or an address as a URL writes it) is on this
/// machine's local network: a private, link-local or loopback address, or
/// a name the local network gives (`nas.local`, `nas.lan`,
/// `nas.home.arpa`). A name is resolved, on a thread of its own and for a
/// moment only; `None` when nothing could be told. Blocks: on a worker.
pub fn local(host: &str) -> Option<bool> {
    if let Some(said) = said(host) {
        return Some(said);
    }
    let host = host.trim_start_matches('[').trim_end_matches(']');
    let name = host.trim_end_matches('.').to_ascii_lowercase();
    let (tx, rx) = crossbeam_channel::bounded(1);
    let lookup = format!("{name}:0");
    std::thread::Builder::new()
        .name("skwir-resolve".into())
        .spawn(move || {
            let ips: Vec<IpAddr> = lookup
                .to_socket_addrs()
                .map(|addrs| addrs.map(|a| a.ip()).collect())
                .unwrap_or_default();
            let _ = tx.send(ips);
        })
        .ok()?;
    let ips = rx.recv_timeout(RESOLVING).ok()?;
    if ips.is_empty() {
        return None;
    }
    Some(ips.into_iter().all(private))
}

/// A private (RFC 1918, unique local), link-local or loopback address.
/// The shared range of carrier-grade NAT (100.64.0.0/10), which VPNs
/// such as Tailscale use across the internet, is not.
pub fn private(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_private() || v4.is_link_local() || v4.is_loopback(),
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return private(IpAddr::V4(v4));
            }
            let first = v6.segments()[0];
            v6.is_loopback() || (first & 0xfe00) == 0xfc00 || (first & 0xffc0) == 0xfe80
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_addresses_are_told_from_others() {
        for local_one in [
            "192.168.1.10",
            "10.0.0.2",
            "172.16.5.1",
            "169.254.1.1",
            "127.0.0.1",
            "::1",
            "fe80::1",
            "fd12:3456::1",
        ] {
            assert_eq!(local(local_one), Some(true), "{local_one}");
        }
        for far in ["8.8.8.8", "100.100.1.1", "2001:db8::1"] {
            assert_eq!(local(far), Some(false), "{far}");
        }
        assert_eq!(local("[fe80::2]"), Some(true), "as a URL writes it");
        assert_eq!(local("nas.local"), Some(true));
        assert_eq!(local("printer.home.arpa"), Some(true));
        assert_eq!(local("localhost"), Some(true));
    }
}
