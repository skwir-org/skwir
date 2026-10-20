//! What the helper reads of a member that is not what was stored: a ZIP
//! whose stored bytes were altered, or decrypted by a wrong password that
//! passes the archive's one-byte check, stops the extraction and the
//! preview of the member, never leaving garbage taken for the file
//! (ADR-059). The ZIPs are written here byte by byte, `ZipCrypto` included,
//! so that what they hold is known exactly.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const WORKER: &str = env!("CARGO_BIN_EXE_skwir-archive-worker");

/// A folder of its own under `~/.cache/skwir-tests`, removed when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let home = std::env::home_dir().expect("a home folder");
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = Path::new(&home).join(".cache/skwir-tests").join(format!(
            "archive-worker-{name}-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &b in bytes {
        crc = update_crc(crc, b);
    }
    !crc
}

fn update_crc(crc: u32, byte: u8) -> u32 {
    let mut c = (crc ^ u32::from(byte)) & 0xff;
    for _ in 0..8 {
        c = if c & 1 == 1 {
            0xEDB8_8320 ^ (c >> 1)
        } else {
            c >> 1
        };
    }
    c ^ (crc >> 8)
}

/// PKWARE's traditional encryption (`ZipCrypto`), as APPNOTE 6.1 says.
struct Keys([u32; 3]);

impl Keys {
    fn new(password: &[u8]) -> Self {
        let mut keys = Keys([0x1234_5678, 0x2345_6789, 0x3456_7890]);
        for &b in password {
            keys.update(b);
        }
        keys
    }
    fn update(&mut self, plain: u8) {
        let k = &mut self.0;
        k[0] = update_crc(k[0], plain);
        k[1] = k[1]
            .wrapping_add(k[0] & 0xff)
            .wrapping_mul(134_775_813)
            .wrapping_add(1);
        k[2] = update_crc(k[2], (k[1] >> 24) as u8);
    }
    fn stream(&self) -> u8 {
        let temp = (self.0[2] | 2) & 0xffff;
        (temp.wrapping_mul(temp ^ 1) >> 8) as u8
    }
    fn encrypt(&mut self, plain: u8) -> u8 {
        let cipher = plain ^ self.stream();
        self.update(plain);
        cipher
    }
    fn decrypt(&mut self, cipher: u8) -> u8 {
        let plain = cipher ^ self.stream();
        self.update(plain);
        plain
    }
}

/// A member: its name, its bytes, and the password it is encrypted by.
struct Member<'a> {
    name: &'a str,
    body: &'a [u8],
    password: Option<&'a [u8]>,
}

/// The 12 bytes before an encrypted member's data: 11 of ours, then the
/// high byte of its CRC, which a password is checked against.
fn header(crc: u32) -> [u8; 12] {
    let mut header = *b"skwir-tests\0";
    header[11] = (crc >> 24) as u8;
    header
}

/// A ZIP of `members`, stored (no compression), with a time and no
/// extra field; `alter` is called on the whole file once written.
fn write_zip(path: &Path, members: &[Member], alter: impl FnOnce(&mut Vec<u8>)) {
    // 2024-01-02 03:04:06, in MS-DOS's fields.
    let (time, day) = ((3u16 << 11) | (4 << 5) | 3, (44u16 << 9) | (1 << 5) | 2);
    let mut out = Vec::new();
    let mut central = Vec::new();
    for member in members {
        let crc = crc32(member.body);
        let data: Vec<u8> = match member.password {
            None => member.body.to_vec(),
            Some(password) => {
                let mut keys = Keys::new(password);
                header(crc)
                    .iter()
                    .chain(member.body)
                    .map(|&b| keys.encrypt(b))
                    .collect()
            }
        };
        let flags: u16 = u16::from(member.password.is_some());
        let offset = u32::try_from(out.len()).unwrap();
        let name = member.name.as_bytes();
        let fields = |out: &mut Vec<u8>| {
            out.extend_from_slice(&flags.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes()); // stored
            out.extend_from_slice(&time.to_le_bytes());
            out.extend_from_slice(&day.to_le_bytes());
            out.extend_from_slice(&crc.to_le_bytes());
            out.extend_from_slice(&u32::try_from(data.len()).unwrap().to_le_bytes());
            out.extend_from_slice(&u32::try_from(member.body.len()).unwrap().to_le_bytes());
            out.extend_from_slice(&u16::try_from(name.len()).unwrap().to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes()); // extra
        };
        out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        out.extend_from_slice(&20u16.to_le_bytes());
        fields(&mut out);
        out.extend_from_slice(name);
        out.extend_from_slice(&data);

        central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes()); // made by
        central.extend_from_slice(&20u16.to_le_bytes()); // needed
        fields(&mut central);
        central.extend_from_slice(&[0; 4]); // comment, disk
        central.extend_from_slice(&[0; 6]); // attributes
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(name);
    }
    let at = u32::try_from(out.len()).unwrap();
    let count = u16::try_from(members.len()).unwrap();
    out.extend_from_slice(&central);
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&u32::try_from(central.len()).unwrap().to_le_bytes());
    out.extend_from_slice(&at.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    alter(&mut out);
    std::fs::write(path, out).unwrap();
}

/// One byte of `body`, where it is stored in `zip`, flipped.
fn flip_in(zip: &mut [u8], body: &[u8]) {
    let at = zip
        .windows(body.len())
        .position(|w| w == body)
        .expect("stored as is");
    zip[at + 3] ^= 0x20;
}

/// The helper run with `args` in `dir`, given `input` on its standard
/// input.
fn run(dir: &Path, args: &[&str], input: &[u8]) -> Output {
    let mut child = Command::new(WORKER)
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // A helper that stopped early reads no more: its status says why.
    let _ = child.stdin.take().unwrap().write_all(input);
    child.wait_with_output().unwrap()
}

/// Units of work handed out ahead, a byte each.
const TICKETS: [u8; 64] = [1; 64];

/// The hexadecimal line a passphrase is given in.
fn answer(password: &[u8]) -> Vec<u8> {
    let mut line: Vec<u8> = password
        .iter()
        .flat_map(|b| format!("{b:02x}").into_bytes())
        .collect();
    line.push(b'\n');
    line
}

const BODY: &[u8] = b"These words were stored without compression.";

#[test]
fn a_stored_member_altered_is_not_extracted_as_it_reads() {
    let scratch = Scratch::new("crc");
    let archive = scratch.0.join("altered.zip");
    let member = Member {
        name: "notes.txt",
        body: BODY,
        password: None,
    };
    write_zip(&archive, &[member], |zip| flip_in(zip, BODY));
    let into = scratch.0.join("into");
    std::fs::create_dir(&into).unwrap();
    let out = run(
        &into,
        &[archive.to_str().unwrap(), "--extract", "--passwords"],
        &TICKETS,
    );
    let said = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "extracted: {said}");
    assert!(said.contains("“notes.txt” is damaged"), "{said}");

    // Nor is it previewed.
    let preview = scratch.0.join("preview");
    std::fs::create_dir(&preview).unwrap();
    let output = preview.join("member.txt");
    let out = run(
        &preview,
        &[
            archive.to_str().unwrap(),
            "0",
            output.to_str().unwrap(),
            "--name",
            "notes.txt",
            "--bytes",
            "4096",
        ],
        b"",
    );
    let said = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "previewed: {said}");
    assert!(said.contains("“notes.txt” is damaged"), "{said}");
    assert!(!output.exists(), "nothing of it left to show");
}

#[test]
fn a_stored_member_whole_is_extracted_and_previewed() {
    let scratch = Scratch::new("whole");
    let archive = scratch.0.join("whole.zip");
    let member = Member {
        name: "notes.txt",
        body: BODY,
        password: None,
    };
    write_zip(&archive, &[member], |_| {});
    let into = scratch.0.join("into");
    std::fs::create_dir(&into).unwrap();
    let out = run(
        &into,
        &[archive.to_str().unwrap(), "--extract", "--passwords"],
        &TICKETS,
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(std::fs::read(into.join("notes.txt")).unwrap(), BODY);
}

/// A password other than `right` that passes the one-byte check of what
/// `right` encrypted: `ZipCrypto`'s check lets one in 256 through.
fn wrong_but_checked(right: &[u8], crc: u32) -> Vec<u8> {
    let mut keys = Keys::new(right);
    let encrypted: Vec<u8> = header(crc).iter().map(|&b| keys.encrypt(b)).collect();
    (0..100_000)
        .map(|n| format!("wrong {n}").into_bytes())
        .find(|candidate| {
            let mut keys = Keys::new(candidate);
            let plain: Vec<u8> = encrypted.iter().map(|&b| keys.decrypt(b)).collect();
            plain[11] == (crc >> 24) as u8
        })
        .expect("one in 256 passes")
}

#[test]
fn a_wrong_password_the_check_lets_through_is_told_as_such() {
    let scratch = Scratch::new("password");
    let archive = scratch.0.join("locked.zip");
    let right: &[u8] = b"right words";
    let member = Member {
        name: "secret.txt",
        body: BODY,
        password: Some(right),
    };
    write_zip(&archive, &[member], |_| {});

    // The right one: the writer above encrypts as an archiver does.
    let into = scratch.0.join("right");
    std::fs::create_dir(&into).unwrap();
    let input = [&TICKETS[..], &answer(right), &TICKETS[..]].concat();
    let out = run(
        &into,
        &[archive.to_str().unwrap(), "--extract", "--passwords"],
        &input,
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8_lossy(&out.stdout).contains(r#"{"passphrase":1}"#));
    assert_eq!(std::fs::read(into.join("secret.txt")).unwrap(), BODY);

    // A wrong one that passes the check decrypts garbage: never taken for
    // the file, and said to be a wrong password or a damaged archive.
    let wrong = wrong_but_checked(right, crc32(BODY));
    let into = scratch.0.join("wrong");
    std::fs::create_dir(&into).unwrap();
    let input = [&TICKETS[..], &answer(&wrong), &TICKETS[..]].concat();
    let out = run(
        &into,
        &[archive.to_str().unwrap(), "--extract", "--passwords"],
        &input,
    );
    let said = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "extracted: {said}");
    assert!(
        said.contains("“secret.txt”: wrong password or damaged archive"),
        "{said}"
    );
}

#[test]
fn each_member_asks_its_password_from_the_first_attempt() {
    let scratch = Scratch::new("attempts");
    let archive = scratch.0.join("two.zip");
    let (one, two): (&[u8], &[u8]) = (b"first words", b"second words");
    write_zip(
        &archive,
        &[
            Member {
                name: "a.txt",
                body: b"the first member",
                password: Some(one),
            },
            Member {
                name: "b.txt",
                body: b"the second member",
                password: Some(two),
            },
        ],
        |_| {},
    );
    // The first member's password must not pass the second's check, or
    // libarchive would decrypt the second with it: our header and these
    // words are fixed, so this holds or fails every time alike.
    let crc = crc32(b"the second member");
    let mut keys = Keys::new(two);
    let encrypted: Vec<u8> = header(crc).iter().map(|&b| keys.encrypt(b)).collect();
    let mut keys = Keys::new(one);
    let plain: Vec<u8> = encrypted.iter().map(|&b| keys.decrypt(b)).collect();
    assert_ne!(plain[11], (crc >> 24) as u8, "pick other words");

    let into = scratch.0.join("into");
    std::fs::create_dir(&into).unwrap();
    let input = [
        &TICKETS[..],
        &answer(one),
        &TICKETS[..],
        &answer(two),
        &TICKETS[..],
    ]
    .concat();
    let out = run(
        &into,
        &[archive.to_str().unwrap(), "--extract", "--passwords"],
        &input,
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let asked: Vec<String> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|l| l.contains("passphrase"))
        .map(str::to_owned)
        .collect();
    assert_eq!(
        asked,
        [r#"{"passphrase":1}"#, r#"{"passphrase":1}"#],
        "the second member's first ask is not a wrong one"
    );
    assert_eq!(
        std::fs::read(into.join("b.txt")).unwrap(),
        b"the second member"
    );
}
