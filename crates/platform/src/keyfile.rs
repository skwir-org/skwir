//! The desktop's key files (the Desktop Entry Specification's format):
//! `.desktop` applications, `.thumbnailer` files, `mimeapps.list`. One
//! reader for all, and one way to split an `Exec` line into words.

/// `(group, key, value)` for every entry of a key file, comments and blank
/// lines skipped, the value untrimmed on the right as written.
pub fn entries(text: &str) -> impl Iterator<Item = (&str, &str, &str)> {
    let mut group = "";
    text.lines().filter_map(move |line| {
        let line = line.trim_start();
        if line.is_empty() || line.starts_with('#') {
            return None;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            group = name;
            return None;
        }
        let (key, value) = line.split_once('=')?;
        Some((group, key.trim_end(), value.trim_start()))
    })
}

/// A string value's escapes: `\s`, `\n`, `\t`, `\r`, `\\`.
pub fn unescape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('s') => out.push(' '),
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

pub fn is_true(value: &str) -> bool {
    value.trim() == "true"
}

/// A `;`-separated list's items.
pub fn list(value: &str) -> Vec<String> {
    value
        .split(';')
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Split an `Exec` value into arguments, as the specification says and
/// as `GLib` reads it (a shell's words): double quotes group, and inside them
/// a backslash escapes `"`, `` ` ``, `$` and `\`; single quotes group
/// without escapes; outside quotes a backslash takes the next character
/// as it is. Each argument says whether any of it was quoted: field codes
/// are expanded only outside quotes.
pub fn exec_words(exec: &str) -> Vec<(String, bool)> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut in_word = false;
    let mut quote: Option<char> = None;
    let mut chars = exec.chars();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some('"'), '\\') => {
                if let Some(next) = chars.next() {
                    if !matches!(next, '"' | '`' | '$' | '\\') {
                        current.push('\\');
                    }
                    current.push(next);
                }
            }
            (Some(_), c) => current.push(c),
            (None, '"' | '\'') => {
                quote = Some(c);
                quoted = true;
                in_word = true;
            }
            (None, '\\') => {
                if let Some(next) = chars.next() {
                    current.push(next);
                    in_word = true;
                }
            }
            (None, c) if c.is_whitespace() => {
                if in_word {
                    words.push((std::mem::take(&mut current), quoted));
                    quoted = false;
                    in_word = false;
                }
            }
            (None, c) => {
                current.push(c);
                in_word = true;
            }
        }
    }
    if in_word {
        words.push((current, quoted));
    }
    words
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exec_lines_split_as_the_desktop_reads_them() {
        let words = |exec| {
            exec_words(exec)
                .into_iter()
                .map(|(w, _)| w)
                .collect::<Vec<_>>()
        };
        assert_eq!(words("app %F"), ["app", "%F"]);
        assert_eq!(
            words(r#"sh -c "echo \"a b\" \$HOME" x"#),
            ["sh", "-c", r#"echo "a b" $HOME"#, "x"]
        );
        assert_eq!(
            words("thumb -s %s 'my %i' %o"),
            ["thumb", "-s", "%s", "my %i", "%o"]
        );
        assert_eq!(words(r"a\ b c"), ["a b", "c"]);
        assert_eq!(exec_words(r#"x "%f""#)[1], ("%f".to_owned(), true));
        assert_eq!(exec_words("x %f")[1], ("%f".to_owned(), false));
    }

    #[test]
    fn a_key_file_is_read_by_group() {
        let text = "# c\n[Desktop Entry]\nName = Viewer\nExec=view\\s%f\n\n[Other]\nName=No\n";
        let got: Vec<_> = entries(text).collect();
        assert_eq!(
            got,
            [
                ("Desktop Entry", "Name", "Viewer"),
                ("Desktop Entry", "Exec", "view\\s%f"),
                ("Other", "Name", "No"),
            ]
        );
        assert_eq!(unescape("view\\s%f"), "view %f");
        assert_eq!(list("a;b;;c ;"), ["a", "b", "c"]);
    }
}
