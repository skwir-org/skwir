//! JSON lines, the one framing between skwir and its workers: a message is
//! one line of JSON, written in one buffer (never left half written), read
//! within a bound (a line longer, cut short or not parsing ends the
//! stream). The manager's side is the platform's `LineChannel`; a worker's
//! is its own loop over these.

use std::io::{BufRead, Read};

use serde::Serialize;
use serde::de::DeserializeOwned;

/// `message` as one line, its newline included.
///
/// # Errors
/// When it cannot be serialized.
pub fn encode(message: &impl Serialize) -> serde_json::Result<Vec<u8>> {
    let mut line = serde_json::to_vec(message)?;
    line.push(b'\n');
    Ok(line)
}

/// The next message of `input`, at most `max` bytes with its newline;
/// `None` at its end. An error is the stream's last: do not read on.
pub fn read<T: DeserializeOwned>(
    input: &mut impl BufRead,
    max: usize,
) -> Option<anyhow::Result<T>> {
    let mut line = Vec::new();
    let read = Read::by_ref(input)
        .take((max as u64).saturating_add(1))
        .read_until(b'\n', &mut line);
    match read {
        Ok(0) => None,
        Err(error) => Some(Err(error.into())),
        Ok(_) if line.len() > max || line.last() != Some(&b'\n') => {
            Some(Err(anyhow::anyhow!("message exceeds limit")))
        }
        Ok(_) => Some(serde_json::from_slice(&line).map_err(Into::into)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_is_a_message_within_its_bound() {
        let mut stream = Vec::new();
        stream.extend(encode(&1u32).unwrap());
        stream.extend(encode(&"two").unwrap());
        stream.extend(b"[3,4,5,6,7,8,9]\n");
        let mut input = std::io::Cursor::new(stream);
        assert_eq!(read::<u32>(&mut input, 8).unwrap().unwrap(), 1);
        assert_eq!(read::<String>(&mut input, 8).unwrap().unwrap(), "two");
        assert!(
            read::<Vec<u32>>(&mut input, 8).unwrap().is_err(),
            "too long"
        );
        let mut empty = std::io::Cursor::new(Vec::new());
        assert!(read::<u32>(&mut empty, 8).is_none());
    }
}
