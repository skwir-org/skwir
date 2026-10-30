//! `PalmDOC`'s compression, an LZ77 of bytes: literals, runs copied from
//! what was written before, and a space joined to the character after it.

/// `record` decompressed, at most `limit` bytes of it.
///
/// # Errors
/// When a copy reaches before the start of what was written.
pub(crate) fn decompress(record: &[u8], limit: usize) -> anyhow::Result<Vec<u8>> {
    let mut out = Vec::with_capacity(record.len().saturating_mul(2).min(limit));
    let mut i = 0;
    while i < record.len() && out.len() < limit {
        let byte = record[i];
        i += 1;
        match byte {
            // That many bytes, as they are.
            1..=8 => {
                let end = (i + usize::from(byte)).min(record.len());
                out.extend_from_slice(&record[i..end]);
                i = end;
            }
            // Two bytes: 11 bits back, 3 bits of length (3 to 10).
            0x80..=0xBF => {
                let Some(&next) = record.get(i) else {
                    break;
                };
                i += 1;
                let pair = (usize::from(byte) << 8 | usize::from(next)) & 0x3FFF;
                let distance = pair >> 3;
                let length = (pair & 7) + 3;
                anyhow::ensure!(
                    distance > 0 && distance <= out.len(),
                    "This book's text is damaged"
                );
                for _ in 0..length {
                    out.push(out[out.len() - distance]);
                }
            }
            // A space and the character after it.
            0xC0..=0xFF => {
                out.push(b' ');
                out.push(byte ^ 0x80);
            }
            // Itself.
            _ => out.push(byte),
        }
    }
    out.truncate(limit);
    Ok(out)
}

/// `text` compressed as `PalmDOC` compresses it: what the tests decompress.
#[cfg(test)]
pub(crate) fn compress(text: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < text.len() {
        // The longest earlier run of 3 to 10 bytes within 2047 back.
        let best = (1..=i.min(2047))
            .filter_map(|distance| {
                let length = (0..10.min(text.len() - i))
                    .take_while(|&k| text[i + k] == text[i - distance + k])
                    .count();
                (length >= 3).then_some((length, distance))
            })
            .max();
        if let Some((length, distance)) = best {
            let pair = 0x8000 | (distance << 3) | (length - 3);
            out.extend_from_slice(&u16::try_from(pair).unwrap().to_be_bytes());
            i += length;
        } else if text[i] == b' ' && text.get(i + 1).is_some_and(|b| (0x40..0x80).contains(b)) {
            out.push(text[i + 1] ^ 0x80);
            i += 2;
        } else if (0x09..0x80).contains(&text[i]) || text[i] == 0 {
            out.push(text[i]);
            i += 1;
        } else {
            out.push(1);
            out.push(text[i]);
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_is_compressed_decompresses_to_itself() {
        for text in [
            &b"The needle is here, the needle is here, the needle is here."[..],
            "Café — naïve é".as_bytes(),
            b"aaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            b"",
        ] {
            assert_eq!(decompress(&compress(text), 1 << 20).unwrap(), text);
        }
        // Bounded, whatever it would expand to.
        let runs = compress(&[b'a'; 4096]);
        assert_eq!(decompress(&runs, 100).unwrap().len(), 100);
        // A copy from before the start is damage, not a panic.
        assert!(decompress(&[0x80, 0x18], 100).is_err());
        assert!(decompress(&[b'a', 0x87], 100).is_ok());
    }
}
