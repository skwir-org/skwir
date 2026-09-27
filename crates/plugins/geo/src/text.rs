//! Text formats a line at a time (IGC and NMEA logs, `MAVLink` and Litchi
//! missions, X-Plane plans), bounded: a line is read up to its limit and
//! never held whole beyond it, so a file without line ends is not read
//! into memory.
use std::io::{BufRead, Read};

use bibli_core::CancelToken;

use crate::xml::Damaged;

/// How a format takes what is not a line of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A device's log: a line too long is skipped and the reading goes
    /// on, and bytes that are not UTF-8 are read as well as they can be,
    /// since a header may hold a name in another encoding.
    Log,
    /// A plan someone made: a line too long, or that is not text, stops
    /// the reading as damage, since a plan with a line missing is not
    /// the plan.
    Plan,
}

impl Kind {
    /// The longest line read: a log's records are short; a plan's lines
    /// are a hundred bytes.
    fn longest(self) -> usize {
        match self {
            Kind::Log => 4096,
            Kind::Plan => 64 * 1024,
        }
    }
}

/// Call `on` with each line of `input` and its number from 0, its end
/// (`\n`, `\r\n` or a lone `\r`), trailing spaces and a leading byte order
/// mark taken off. `Ok(Some(_))` says the input was damaged where the
/// reading stopped; what was handed over before stays good. `Err` is
/// `on`'s own error (a file refused by its header) or a cancellation.
pub fn lines<R: BufRead>(
    mut input: R,
    cancel: &CancelToken,
    kind: Kind,
    mut on: impl FnMut(usize, &str) -> anyhow::Result<()>,
) -> anyhow::Result<Option<Damaged>> {
    let longest = kind.longest();
    let mut buf = Vec::with_capacity(128);
    let mut number = 0usize;
    loop {
        if number.is_multiple_of(4096) {
            bibli_platform::inspection::check(cancel)?;
        }
        buf.clear();
        let read = match Read::take(&mut input, longest as u64 + 1).read_until(b'\n', &mut buf) {
            Ok(read) => read,
            Err(error) => return Ok(Some(Damaged(error.to_string()))),
        };
        if read == 0 {
            return Ok(None);
        }
        if read > longest && buf.last() != Some(&b'\n') {
            match kind {
                Kind::Log => {
                    // The rest of it, without holding it.
                    if let Err(error) = input.skip_until(b'\n') {
                        return Ok(Some(Damaged(error.to_string())));
                    }
                    number += 1;
                    continue;
                }
                Kind::Plan => {
                    return Ok(Some(Damaged(format!(
                        "line {} is longer than {}",
                        number + 1,
                        bibli_platform::inspection::limits::describe(longest)
                    ))));
                }
            }
        }
        let text = match (kind, std::str::from_utf8(&buf)) {
            (_, Ok(text)) => std::borrow::Cow::Borrowed(text),
            (Kind::Log, Err(_)) => String::from_utf8_lossy(&buf),
            (Kind::Plan, Err(_)) => {
                return Ok(Some(Damaged(format!("line {} is not text", number + 1))));
            }
        };
        let text = if number == 0 {
            text.strip_prefix('\u{feff}').unwrap_or(&text)
        } else {
            &text
        };
        // A classic Mac file ends its lines with a lone `\r`: its lines
        // are split here.
        for line in text.trim_end_matches(['\n', '\r']).split('\r') {
            on(number, line.trim_end())?;
            number += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(input: &[u8], kind: Kind) -> (Vec<String>, bool) {
        let mut seen = Vec::new();
        let damaged = lines(input, &CancelToken::new(), kind, |_, line| {
            seen.push(line.to_owned());
            Ok(())
        })
        .unwrap();
        (seen, damaged.is_some())
    }

    #[test]
    fn lines_end_as_any_system_ends_them() {
        let (seen, damaged) = read(b"\xef\xbb\xbfone\r\ntwo  \nthree\rfour\n", Kind::Plan);
        assert_eq!(seen, ["one", "two", "three", "four"]);
        assert!(!damaged);
    }

    #[test]
    fn a_log_skips_what_a_plan_calls_damage() {
        let long = [b"first\n".as_slice(), &[b'x'; 5000], b"\nlast\n"].concat();
        assert_eq!(
            read(&long, Kind::Log),
            (vec!["first".into(), "last".into()], false)
        );
        let latin = b"HFPLTPILOT:Ren\xe9\nB\n";
        let (seen, damaged) = read(latin, Kind::Log);
        assert_eq!((seen.len(), damaged), (2, false));
        assert_eq!(read(latin, Kind::Plan), (vec![], true));
        let huge = [b"first\n".as_slice(), &vec![b'x'; 70_000]].concat();
        assert_eq!(read(&huge, Kind::Plan), (vec!["first".into()], true));
    }
}
