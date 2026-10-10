//! Keys as the bytes a program on a terminal reads: xterm's encoding, which
//! every shell and full-screen program understands.
//!
//! Plain text never comes here (it is `Input::Text`, UTF-8 as typed); these
//! are the keys that have no text, and characters with Ctrl or Alt.

use skwir_core::terminal::{Key, KeyCode, Mods};

const ESC: u8 = 0x1b;

/// xterm's modifier parameter: 1, plus 1 for Shift, 2 for Alt, 4 for Ctrl.
fn parameter(mods: Mods) -> u8 {
    1 + u8::from(mods.shift) + 2 * u8::from(mods.alt) + 4 * u8::from(mods.ctrl)
}

/// A cursor key or Home/End: `ESC [ X`, `ESC O X` in application cursor
/// mode, `ESC [ 1 ; m X` with modifiers.
fn cursor(letter: u8, mods: Mods, app_cursor: bool) -> Vec<u8> {
    match parameter(mods) {
        1 if app_cursor => vec![ESC, b'O', letter],
        1 => vec![ESC, b'[', letter],
        m => format!("\x1b[1;{m}{}", char::from(letter)).into_bytes(),
    }
}

/// A key of the editing pad or F5 and above: `ESC [ n ~`, `ESC [ n ; m ~`.
fn tilde(number: u8, mods: Mods) -> Vec<u8> {
    match parameter(mods) {
        1 => format!("\x1b[{number}~").into_bytes(),
        m => format!("\x1b[{number};{m}~").into_bytes(),
    }
}

/// What Ctrl makes of a character, as a terminal's keyboard always has.
fn control(c: char) -> Option<u8> {
    match c.to_ascii_lowercase() {
        l @ 'a'..='z' => Some(l as u8 & 0x1f),
        '@' | ' ' | '2' => Some(0),
        '[' | '3' => Some(0x1b),
        '\\' | '4' => Some(0x1c),
        ']' | '5' => Some(0x1d),
        '^' | '6' => Some(0x1e),
        '_' | '-' | '/' | '7' => Some(0x1f),
        '?' | '8' => Some(0x7f),
        _ => None,
    }
}

/// With Alt, a key's bytes follow an Escape.
fn meta(alt: bool, bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len() + 1);
    if alt {
        out.push(ESC);
    }
    out.extend_from_slice(bytes);
    out
}

/// The bytes for `key`. `app_cursor` is the mode a full-screen program sets
/// (DECCKM) to read cursor keys as `ESC O`.
pub fn encode(key: Key, app_cursor: bool) -> Vec<u8> {
    let mods = key.mods;
    match key.code {
        KeyCode::Char(c) => {
            let mut buf = [0; 4];
            match control(c).filter(|_| mods.ctrl) {
                Some(byte) => meta(mods.alt, &[byte]),
                None => meta(mods.alt, c.encode_utf8(&mut buf).as_bytes()),
            }
        }
        KeyCode::Enter => meta(mods.alt, b"\r"),
        KeyCode::Tab if mods.shift => b"\x1b[Z".to_vec(),
        KeyCode::Tab => meta(mods.alt, b"\t"),
        KeyCode::Backspace if mods.ctrl => meta(mods.alt, &[0x08]),
        KeyCode::Backspace => meta(mods.alt, &[0x7f]),
        KeyCode::Escape => meta(mods.alt, &[ESC]),
        KeyCode::Up => cursor(b'A', mods, app_cursor),
        KeyCode::Down => cursor(b'B', mods, app_cursor),
        KeyCode::Right => cursor(b'C', mods, app_cursor),
        KeyCode::Left => cursor(b'D', mods, app_cursor),
        KeyCode::Home => cursor(b'H', mods, app_cursor),
        KeyCode::End => cursor(b'F', mods, app_cursor),
        KeyCode::Insert => tilde(2, mods),
        KeyCode::Delete => tilde(3, mods),
        KeyCode::PageUp => tilde(5, mods),
        KeyCode::PageDown => tilde(6, mods),
        KeyCode::F(n @ 1..=4) => {
            let letter = b"PQRS"[usize::from(n - 1)];
            match parameter(mods) {
                1 => vec![ESC, b'O', letter],
                m => format!("\x1b[1;{m}{}", char::from(letter)).into_bytes(),
            }
        }
        KeyCode::F(n @ 5..=12) => tilde([15, 17, 18, 19, 20, 21, 23, 24][usize::from(n - 5)], mods),
        KeyCode::F(_) => Vec::new(),
    }
}

/// Text pasted into the terminal: framed when the program asked for
/// bracketed paste, so a shell does not run pasted lines as they arrive,
/// and with no escape inside, so nothing pasted can close the frame early
/// or send the program a sequence of its own: removing the end marker
/// alone left one made whole by the removal (`\x1b[201\x1b[201~~`), as
/// Alacritty found. Without the frame, line ends become the Enter a
/// program expects.
pub fn paste(text: &str, bracketed: bool) -> Vec<u8> {
    if bracketed {
        let inner = text.replace('\x1b', "");
        format!("\x1b[200~{inner}\x1b[201~").into_bytes()
    } else {
        text.replace("\r\n", "\r").replace('\n', "\r").into_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode, ctrl: bool, alt: bool, shift: bool) -> Key {
        Key {
            code,
            mods: Mods { ctrl, alt, shift },
        }
    }

    fn plain(code: KeyCode) -> Key {
        key(code, false, false, false)
    }

    #[test]
    fn control_characters() {
        let ctrl = |c| encode(key(KeyCode::Char(c), true, false, false), false);
        assert_eq!(ctrl('c'), [0x03]);
        assert_eq!(ctrl('C'), [0x03], "Shift changes nothing with Ctrl");
        assert_eq!(ctrl(' '), [0x00]);
        assert_eq!(ctrl('['), [0x1b]);
        assert_eq!(ctrl('/'), [0x1f]);
        assert_eq!(ctrl('?'), [0x7f]);
        assert_eq!(ctrl('é'), "é".as_bytes(), "no control code: the character");
        assert_eq!(
            encode(key(KeyCode::Char('d'), true, true, false), false),
            [0x1b, 0x04]
        );
        assert_eq!(
            encode(key(KeyCode::Char('b'), false, true, false), false),
            b"\x1bb"
        );
    }

    #[test]
    fn cursor_keys_follow_the_mode_and_carry_modifiers() {
        assert_eq!(encode(plain(KeyCode::Up), false), b"\x1b[A");
        assert_eq!(encode(plain(KeyCode::Up), true), b"\x1bOA");
        assert_eq!(
            encode(key(KeyCode::Left, true, false, false), true),
            b"\x1b[1;5D",
            "with a modifier, the mode does not matter"
        );
        assert_eq!(
            encode(key(KeyCode::End, false, true, true), false),
            b"\x1b[1;4F"
        );
    }

    #[test]
    fn editing_and_function_keys() {
        assert_eq!(encode(plain(KeyCode::Delete), false), b"\x1b[3~");
        assert_eq!(
            encode(key(KeyCode::PageUp, false, false, true), false),
            b"\x1b[5;2~"
        );
        assert_eq!(encode(plain(KeyCode::F(1)), false), b"\x1bOP");
        assert_eq!(
            encode(key(KeyCode::F(4), true, false, false), false),
            b"\x1b[1;5S"
        );
        assert_eq!(encode(plain(KeyCode::F(5)), false), b"\x1b[15~");
        assert_eq!(encode(plain(KeyCode::F(12)), false), b"\x1b[24~");
        assert!(encode(plain(KeyCode::F(13)), false).is_empty());
    }

    #[test]
    fn keys_with_text_of_their_own() {
        assert_eq!(encode(plain(KeyCode::Enter), false), b"\r");
        assert_eq!(encode(plain(KeyCode::Backspace), false), [0x7f]);
        assert_eq!(
            encode(key(KeyCode::Backspace, true, false, false), false),
            [0x08]
        );
        assert_eq!(
            encode(key(KeyCode::Tab, false, false, true), false),
            b"\x1b[Z"
        );
        assert_eq!(
            encode(key(KeyCode::Escape, false, true, false), false),
            b"\x1b\x1b"
        );
    }

    #[test]
    fn pasted_text_cannot_close_its_own_frame() {
        assert_eq!(paste("ls\nrm -rf x\n", false), b"ls\rrm -rf x\r");
        assert_eq!(
            paste("a\x1b[201~b", true),
            b"\x1b[200~a[201~b\x1b[201~",
            "an end marker inside loses its escape"
        );
        assert_eq!(
            paste("a\x1b[201\x1b[201~~\nrm -rf x\n", true),
            b"\x1b[200~a[201[201~~\nrm -rf x\n\x1b[201~",
            "nor is one made whole by taking another out"
        );
    }
}
