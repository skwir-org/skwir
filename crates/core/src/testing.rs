//! Inputs no file should hold, for tests of what reads files skwir did
//! not write: arbitrary bytes, and a sample file mutated (bytes changed,
//! characters of several bytes put where a parser expects ASCII, cut
//! short). A mutated sample stays close to a real file, so it reaches
//! past a parser's first checks. Behind the `testing` feature: plugins
//! ask for it in their dev-dependencies alone.

use proptest::prelude::*;

/// What is put into a sample: characters of two, three and four bytes,
/// and the punctuation file formats are cut at.
const PUT: &[&str] = &[
    "é", "€", "𝄞", "\u{0}", "\n", "\r", "\t", " ", "\"", "'", "<", ">", "&", ":", ";", ",", "=",
    "[", "]", "{", "}", "(", ")", "-", ".", "/", "\\", "#", "*", "%", "0", "9",
];

/// Arbitrary bytes, at most `max`.
pub fn bytes(max: usize) -> impl Strategy<Value = Vec<u8>> {
    proptest::collection::vec(any::<u8>(), 0..=max)
}

/// `sample` with a few bytes changed, a few pieces of [`PUT`] put in,
/// and, one time in four, cut short.
pub fn mutated(sample: impl Into<Vec<u8>>) -> impl Strategy<Value = Vec<u8>> {
    let sample: std::sync::Arc<[u8]> = sample.into().into();
    let len = sample.len();
    (
        proptest::collection::vec((0..len.max(1), any::<u8>()), 0..8),
        proptest::collection::vec((0..=len, proptest::sample::select(PUT)), 0..6),
        prop_oneof![3 => Just(None), 1 => (0..=len).prop_map(Some)],
    )
        .prop_map(move |(changes, puts, cut)| {
            let mut out = sample.to_vec();
            for (at, byte) in changes {
                if let Some(b) = out.get_mut(at) {
                    *b = byte;
                }
            }
            for (at, piece) in puts {
                let at = at.min(out.len());
                out.splice(at..at, piece.bytes());
            }
            if let Some(cut) = cut {
                out.truncate(cut);
            }
            out
        })
}

/// [`mutated`] for a parser of text: invalid UTF-8 made into U+FFFD.
pub fn mutated_text(sample: impl Into<String>) -> impl Strategy<Value = String> {
    mutated(sample.into().into_bytes()).prop_map(|b| String::from_utf8_lossy(&b).into_owned())
}

/// A TCP port of 127.0.0.1 for a server a test starts, which no other test
/// of this machine is given while this one runs.
///
/// Tests run at once, each in a process of its own. The system's choice of
/// a free port (bound, then let go for the server to take) may be its
/// choice for the next process asking too: a server then could not take
/// its port, and what answered there was another test's (2026-11-14:
/// rclone's WebDAV server, and an sshd answering its client). So the port
/// is claimed by a lock on a file named after it, which every test asks
/// for, held until this process ends, its servers with it: a server
/// stopped and started again on its port finds it still free.
///
/// # Panics
/// Where no port can be bound, or its file not made.
pub fn free_port() -> u16 {
    static CLAIMED: std::sync::Mutex<Vec<std::fs::File>> = std::sync::Mutex::new(Vec::new());
    let folder = std::env::home_dir()
        .expect("a home folder")
        .join(".cache/skwir-tests/ports");
    std::fs::create_dir_all(&folder).expect("the ports' folder");
    loop {
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .and_then(|listener| listener.local_addr())
            .expect("a free port")
            .port();
        let file = std::fs::File::create(folder.join(port.to_string())).expect("a port's file");
        if file.try_lock().is_ok() {
            CLAIMED
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(file);
            return port;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::free_port;

    /// A port given is not given again while its process runs, even when
    /// the system offers it once more.
    #[test]
    fn a_port_given_is_not_given_again() {
        let ports: std::collections::HashSet<u16> = (0..64).map(|_| free_port()).collect();
        assert_eq!(ports.len(), 64);
        for port in &ports {
            let file = std::fs::File::open(
                std::env::home_dir()
                    .unwrap()
                    .join(".cache/skwir-tests/ports")
                    .join(port.to_string()),
            )
            .unwrap();
            assert!(file.try_lock().is_err(), "{port} is claimed");
        }
    }
}
