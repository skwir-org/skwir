//! What an IGES file says of itself in its global section: the program that
//! wrote it and the IGES version. The global section is the lines marked
//! `G` in column 73, their first 72 columns one text: parameters split by
//! a delimiter the section names first (`,` unless said), ended by another
//! (`;`), strings counted (`15HSolidWorks 2014`). The model is drawn by
//! `exchange`.
use skwir_platform::inspection::limits;

/// The global section's parameters (1-based in the standard).
const NATIVE_SYSTEM: usize = 5;
const PREPROCESSOR: usize = 6;
const VERSION: usize = 23;

/// What the global section says: the program and the IGES version.
pub fn header(text: &str) -> Vec<(String, String)> {
    let global: Vec<u8> = text
        .lines()
        .filter_map(|line| {
            let line = line.as_bytes();
            (line.get(72) == Some(&b'G')).then(|| &line[..72])
        })
        .flatten()
        .copied()
        .collect();
    let parameters = parameters(&global);
    let said = |n: usize| {
        parameters
            .get(n - 1)
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
    };
    let program = said(NATIVE_SYSTEM)
        .or_else(|| said(PREPROCESSOR))
        .map(|program| program.chars().take(limits::DETAIL_CHARS).collect());
    let version = said(VERSION)
        .and_then(|flag| flag.parse::<u32>().ok())
        .and_then(version);
    let format = match version {
        Some(version) => format!("IGES {version}"),
        None => "IGES".to_owned(),
    };
    let mut out = Vec::new();
    if let Some(program) = program {
        out.push(("Made with".to_owned(), program));
    }
    if !global.is_empty() {
        out.push(("Format".to_owned(), format));
    }
    out
}

/// The version a version flag names.
fn version(flag: u32) -> Option<&'static str> {
    Some(match flag {
        1 | 2 => "1.0",
        3 => "2.0",
        4 | 5 => "3.0",
        6 | 7 => "4.0",
        8 => "5.0",
        9 => "5.1",
        10 => "5.2",
        11 => "5.3",
        _ => return None,
    })
}

/// The parameters, a counted string as its text, any other as written; as
/// many as are read before the section ends or breaks.
fn parameters(global: &[u8]) -> Vec<String> {
    delimited(global).unwrap_or_default()
}

fn delimited(global: &[u8]) -> Option<Vec<String>> {
    // The first two name the delimiters, or are empty for the usual ones.
    let mut at = 0;
    let mut delimiter = b',';
    if global.starts_with(b"1H") {
        delimiter = *global.get(2)?;
        at = 3;
    }
    (global.get(at) == Some(&delimiter)).then_some(())?;
    at += 1;
    let mut end = b';';
    if global.get(at..)?.starts_with(b"1H") {
        end = *global.get(at + 2)?;
        at += 3;
    }
    (global.get(at) == Some(&delimiter)).then_some(())?;
    at += 1;
    let mut out = vec![String::new(), String::new()];
    while at < global.len() {
        let start = at;
        while global.get(at).is_some_and(u8::is_ascii_digit) {
            at += 1;
        }
        let value = if at > start && global.get(at) == Some(&b'H') {
            let count: usize = std::str::from_utf8(&global[start..at]).ok()?.parse().ok()?;
            let Some(text) = global.get(at + 1..at + 1 + count) else {
                break;
            };
            at += 1 + count;
            String::from_utf8_lossy(text).into_owned()
        } else {
            while global
                .get(at)
                .is_some_and(|&byte| byte != delimiter && byte != end)
            {
                at += 1;
            }
            String::from_utf8_lossy(&global[start..at]).into_owned()
        };
        out.push(value);
        match global.get(at) {
            Some(&byte) if byte == delimiter => at += 1,
            _ => break,
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A start line, then `global` in lines of 72 columns marked `G`.
    fn file(global: &str) -> String {
        use std::fmt::Write as _;
        let mut out = format!("{:72}S      1\n", "a part");
        for (index, chunk) in global.as_bytes().chunks(72).enumerate() {
            let _ = writeln!(out, "{:72}G{:7}", String::from_utf8_lossy(chunk), index + 1);
        }
        out
    }

    #[test]
    fn the_global_section_says_the_program_and_the_version() {
        // As SolidWorks writes it: the delimiters named.
        let text = file(
            "1H,,1H;,4Hpart,8Hpart.igs,15HSolidWorks 2014,15HSolidWorks 2014,32,308,15,308,15,4Hpart,1.,2,2HMM,50,0.125,13H260101.120000,1E-08,100.,0H,0H,11,0,13H260101.120000;",
        );
        assert_eq!(
            header(&text),
            [
                ("Made with".to_owned(), "SolidWorks 2014".to_owned()),
                ("Format".to_owned(), "IGES 5.3".to_owned()),
            ]
        );
        // The usual delimiters left out, no product named.
        let text = file(
            ",,,13HFilename.iges,9HEUCLID-QM,4HOL-2,32,308,15,308,15,7HUnknown,1.,2,2HMM,1,0.01,13H260101.120000,0.0001,1000.,6Hauthor,7Hcompany,9,0,13H260101.120000;",
        );
        assert_eq!(
            header(&text),
            [
                ("Made with".to_owned(), "EUCLID-QM".to_owned()),
                ("Format".to_owned(), "IGES 5.1".to_owned()),
            ]
        );
        // A string holding the delimiters, and delimiters of its own.
        let text = file("1H//1H#/4Hpart/5Ha,b;c/0H/7HWriter;/32#");
        assert_eq!(header(&text)[0].1, "Writer;");
        // No program, an unknown version, and a section cut short.
        let text =
            file(",,,0H,0H,0H,32,308,15,308,15,0H,1.,2,2HMM,1,0.01,0H,0.0001,1000.,0H,0H,99,0,0H;");
        assert_eq!(header(&text), [("Format".to_owned(), "IGES".to_owned())]);
        let text = file(",,,4Hpart,99HCut off");
        assert_eq!(header(&text), [("Format".to_owned(), "IGES".to_owned())]);
        assert!(header("no global section here").is_empty());
    }
}
