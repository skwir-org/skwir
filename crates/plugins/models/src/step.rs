//! What a STEP file (ISO 10303-21) says of itself in its header: the
//! protocol it is written to (AP203, AP214, AP242) and the program that
//! wrote it. The model is drawn by `exchange`.
use skwir_platform::inspection::limits;

/// What the header says: the protocol and the program that wrote the file.
pub fn header(text: &str) -> Vec<(String, String)> {
    let Some(start) = text.find("HEADER;") else {
        return Vec::new();
    };
    let text = &text[start + "HEADER;".len()..];
    let text = &text[..text.find("ENDSEC;").unwrap_or(text.len())];
    let mut format = None;
    let mut program = None;
    for (name, values) in statements(&without_comments(text)) {
        match name.as_str() {
            "FILE_SCHEMA" => {
                let schema = match values.first() {
                    Some(Value::List(names)) => names.first().and_then(Value::text),
                    _ => None,
                };
                format = Some(match schema.and_then(protocol) {
                    Some(protocol) => format!("STEP {protocol}"),
                    None => "STEP".to_owned(),
                });
            }
            // The originating system, else the program that translated it.
            "FILE_NAME" => {
                program = [5, 4]
                    .into_iter()
                    .filter_map(|at| values.get(at).and_then(Value::text))
                    .map(str::trim)
                    .find(|said| !said.is_empty())
                    .map(|said| said.chars().take(limits::DETAIL_CHARS).collect());
            }
            _ => {}
        }
    }
    [("Made with", program), ("Format", format)]
        .into_iter()
        .filter_map(|(label, value)| Some((label.to_owned(), value?)))
        .collect()
}

/// The application protocol a schema's name says.
fn protocol(schema: &str) -> Option<&'static str> {
    let name = schema
        .split([' ', '{'])
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    match name.as_str() {
        "CONFIG_CONTROL_DESIGN" => Some("AP203"),
        "AUTOMOTIVE_DESIGN" | "AUTOMOTIVE_DESIGN_CC2" => Some("AP214"),
        _ if name.starts_with("AP203") => Some("AP203"),
        _ if name.starts_with("AP214") => Some("AP214"),
        _ if name.starts_with("AP242") => Some("AP242"),
        _ => None,
    }
}

/// A parameter of a header's statement.
#[derive(Debug, PartialEq)]
enum Value {
    Text(String),
    List(Vec<Value>),
    /// A number, an enumeration, `$` or `*`: nothing read here.
    Other,
}

impl Value {
    fn text(&self) -> Option<&str> {
        match self {
            Value::Text(text) => Some(text),
            _ => None,
        }
    }
}

/// The text without its `/* comments */`, which a string never holds.
fn without_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("/*") {
        out.push_str(&rest[..start]);
        rest = rest[start..]
            .find("*/")
            .map_or("", |end| &rest[start + end + 2..]);
    }
    out.push_str(rest);
    out
}

/// Each `NAME(parameters);` of a header, with its parameters.
fn statements(text: &str) -> Vec<(String, Vec<Value>)> {
    let mut parser = Parser {
        bytes: text.as_bytes(),
        at: 0,
    };
    let mut out = Vec::new();
    loop {
        parser.blank();
        let start = parser.at;
        while parser
            .peek()
            .is_some_and(|b| b.is_ascii_alphanumeric() || b == b'_')
        {
            parser.at += 1;
        }
        if parser.at == start {
            break;
        }
        let name = text[start..parser.at].to_ascii_uppercase();
        parser.blank();
        let Some(Value::List(values)) = parser.value() else {
            break;
        };
        parser.blank();
        if parser.peek() != Some(b';') {
            break;
        }
        parser.at += 1;
        out.push((name, values));
    }
    out
}

struct Parser<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.at).copied()
    }

    fn blank(&mut self) {
        while self.peek().is_some_and(|b| b.is_ascii_whitespace()) {
            self.at += 1;
        }
    }

    fn value(&mut self) -> Option<Value> {
        self.blank();
        match self.peek()? {
            b'\'' => {
                self.at += 1;
                let mut raw = Vec::new();
                loop {
                    let byte = self.peek()?;
                    self.at += 1;
                    if byte == b'\'' {
                        if self.peek() != Some(b'\'') {
                            break;
                        }
                        self.at += 1;
                    }
                    raw.push(byte);
                }
                Some(Value::Text(decoded(&String::from_utf8_lossy(&raw))))
            }
            b'(' => {
                self.at += 1;
                let mut values = Vec::new();
                loop {
                    self.blank();
                    if self.peek()? == b')' {
                        self.at += 1;
                        return Some(Value::List(values));
                    }
                    values.push(self.value()?);
                    self.blank();
                    match self.peek()? {
                        b',' => self.at += 1,
                        b')' => {}
                        _ => return None,
                    }
                }
            }
            // A number, an enumeration, `$`, `*`, or a typed parameter
            // (`NAME(…)`).
            _ => {
                while let Some(byte) = self.peek() {
                    match byte {
                        b',' | b')' | b';' => break,
                        b'(' => {
                            self.value()?;
                        }
                        _ => self.at += 1,
                    }
                }
                Some(Value::Other)
            }
        }
    }
}

/// A string's text: `\X2\…\X0\` spells UTF-16 units in hexadecimal,
/// `\X\hh` a Latin-1 byte, `\S\c` a character of the upper half, `\\` a
/// backslash.
fn decoded(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(at) = rest.find('\\') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        if let Some(after) = rest.strip_prefix("\\X2\\") {
            let end = after.find("\\X0\\").unwrap_or(after.len());
            let units: Vec<u16> = after.as_bytes()[..end]
                .as_chunks::<4>()
                .0
                .iter()
                .filter_map(|four| u16::from_str_radix(std::str::from_utf8(four).ok()?, 16).ok())
                .collect();
            out.extend(char::decode_utf16(units).map(|c| c.unwrap_or('\u{FFFD}')));
            rest = after.get(end + 4..).unwrap_or_default();
        } else if let Some(after) = rest.strip_prefix("\\X\\")
            && let Some(byte) = after
                .get(..2)
                .and_then(|hex| u8::from_str_radix(hex, 16).ok())
        {
            out.push(char::from(byte));
            rest = &after[2..];
        } else if let Some(after) = rest.strip_prefix("\\S\\")
            && let Some(c) = after.chars().next()
            && c.is_ascii()
        {
            out.push(char::from(c as u8 + 128));
            rest = &after[1..];
        } else if let Some(after) = rest.strip_prefix("\\\\") {
            out.push('\\');
            rest = after;
        } else {
            out.push('\\');
            rest = &rest[1..];
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_header_says_the_protocol_and_the_program() {
        let text = "ISO-10303-21;\nHEADER;\n/* written by a test */\n\
            FILE_DESCRIPTION(('a part; with a semicolon'),'2;1');\n\
            FILE_NAME(/* name */ 'a part''s model.stp', '2026-10-19T12:00:00', \
            ('someone'), ('somewhere'), 'Translator 7.1', \
            'Modeller \\X2\\00E9\\X0\\dition 2026', '');\n\
            FILE_SCHEMA(('AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF { 1 0 10303 442 1 1 4 }'));\n\
            ENDSEC;\nDATA;\n#1=PRODUCT('x','x','',(#2));\nENDSEC;\n";
        assert_eq!(
            header(text),
            [
                ("Made with".to_owned(), "Modeller édition 2026".to_owned()),
                ("Format".to_owned(), "STEP AP242".to_owned()),
            ]
        );
        // No originating system: the translator.
        let text = "HEADER;FILE_NAME('p','t',(''),(''),'Translator 7.1','','');\
            FILE_SCHEMA(('AUTOMOTIVE_DESIGN { 1 0 10303 214 1 1 1 1 }'));ENDSEC;";
        assert_eq!(
            header(text),
            [
                ("Made with".to_owned(), "Translator 7.1".to_owned()),
                ("Format".to_owned(), "STEP AP214".to_owned()),
            ]
        );
        let text = "HEADER;FILE_NAME('p','t',(''),(''),'','','');\
            FILE_SCHEMA(('CONFIG_CONTROL_DESIGN'));ENDSEC;";
        assert_eq!(
            header(text),
            [("Format".to_owned(), "STEP AP203".to_owned())]
        );
        let text = "HEADER;FILE_SCHEMA(('IFC4'));ENDSEC;";
        assert_eq!(header(text), [("Format".to_owned(), "STEP".to_owned())]);
        assert!(header("no header here").is_empty());
        // Broken off: what was read before stands.
        let text = "HEADER;FILE_SCHEMA(('AP203_CONFIGURATION_CONTROLLED_3D_DESIGN_OF_MECHANICAL_PARTS_AND_ASSEMBLIES_MIM_LF'));FILE_NAME('p";
        assert_eq!(
            header(text),
            [("Format".to_owned(), "STEP AP203".to_owned())]
        );
    }

    #[test]
    fn strings_decode_their_escapes() {
        assert_eq!(decoded("plain"), "plain");
        assert_eq!(decoded("\\X2\\00E900E8\\X0\\t\\X2\\00E9\\X0\\"), "éèté");
        assert_eq!(decoded("caf\\X\\E9"), "café");
        assert_eq!(decoded("\\S\\i"), "é");
        assert_eq!(decoded("a\\\\b"), "a\\b");
        assert_eq!(decoded("ends \\"), "ends \\");
        // Surrogate pairs, and units broken off.
        assert_eq!(decoded("\\X2\\D83DDE00\\X0\\"), "😀");
        assert_eq!(decoded("\\X2\\00E"), "");
    }
}
