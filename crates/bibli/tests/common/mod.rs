//! What several integration tests build.

/// A PNG of `width` by `height` pixels, written by hand (stored, not
/// compressed), so no encoder is needed.
#[allow(dead_code, reason = "each test crate uses some of what is here")]
pub fn synthetic_png(width: u32, height: u32) -> Vec<u8> {
    fn crc(bytes: &[u8]) -> u32 {
        !bytes.iter().fold(u32::MAX, |crc, &b| {
            (0..8).fold(crc ^ u32::from(b), |c, _| {
                if c & 1 == 1 {
                    0xEDB8_8320 ^ (c >> 1)
                } else {
                    c >> 1
                }
            })
        })
    }
    fn chunk(out: &mut Vec<u8>, kind: [u8; 4], data: &[u8]) {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let mut body = kind.to_vec();
        body.extend_from_slice(data);
        out.extend_from_slice(&body);
        out.extend_from_slice(&crc(&body).to_be_bytes());
    }
    let mut raw = Vec::new();
    for _ in 0..height {
        raw.push(0);
        raw.extend((0..width).flat_map(|_| [1u8, 2, 3]));
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in &raw {
        a = (a + u32::from(byte)) % 65521;
        b = (b + a) % 65521;
    }
    let mut idat = vec![0x78, 0x01, 0x01];
    idat.extend_from_slice(&(raw.len() as u16).to_le_bytes());
    idat.extend_from_slice(&(!(raw.len() as u16)).to_le_bytes());
    idat.extend_from_slice(&raw);
    idat.extend_from_slice(&((b << 16) | a).to_be_bytes());
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    chunk(&mut out, *b"IHDR", &ihdr);
    chunk(&mut out, *b"IDAT", &idat);
    chunk(&mut out, *b"IEND", &[]);
    out
}

/// A temporary folder under `~/.cache/bibli-tests`: a real filesystem,
/// never the user's files.
#[allow(dead_code, reason = "each test crate uses some of what is here")]
pub fn home_tempdir() -> tempfile::TempDir {
    let base = dirs::home_dir()
        .expect("home")
        .join(".cache")
        .join("bibli-tests");
    std::fs::create_dir_all(&base).expect("test base");
    tempfile::Builder::new()
        .prefix("any-file-")
        .tempdir_in(base)
        .expect("tempdir")
}
