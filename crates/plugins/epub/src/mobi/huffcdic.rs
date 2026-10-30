//! Mobipocket's Huffman coding with a dictionary (HUFF and CDIC records):
//! each code names a phrase of the dictionary, itself coded the same way
//! unless marked as plain bytes. A phrase is decoded once, the first time
//! it is met; one that leads back to itself is damage.

/// Phrases decoded within phrases, at most: deeper is damage.
const DEPTH: usize = 32;

/// The tables of a HUFF record and the phrases of its CDIC records.
pub(crate) struct Huffman {
    /// By a code's first byte: its length, whether that byte ends it, the
    /// largest code of that length.
    first: [(u32, bool, u64); 256],
    /// By length: the smallest and largest codes of that length.
    lowest: [u64; 33],
    highest: [u64; 33],
    phrases: Vec<Phrase>,
}

enum Phrase {
    Coded(Vec<u8>),
    /// Being decoded: met again, the dictionary loops.
    Open,
    Plain(Vec<u8>),
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    let b = bytes.get(at..at.checked_add(4)?)?;
    Some(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

impl Huffman {
    /// The coding the `huff` record and its `cdics` give.
    pub(crate) fn new<'a>(
        huff: &[u8],
        cdics: impl IntoIterator<Item = &'a [u8]>,
    ) -> anyhow::Result<Self> {
        let damaged = || anyhow::anyhow!("This book's Huffman tables are damaged");
        anyhow::ensure!(huff.get(..8) == Some(b"HUFF\0\0\0\x18"), damaged());
        let tables = u32_at(huff, 8).ok_or_else(damaged)? as usize;
        let ranges = u32_at(huff, 12).ok_or_else(damaged)? as usize;
        let mut first = [(0, false, 0); 256];
        for (byte, entry) in first.iter_mut().enumerate() {
            let value = u32_at(huff, tables + 4 * byte).ok_or_else(damaged)?;
            let length = value & 0x1F;
            anyhow::ensure!((1..=32).contains(&length), damaged());
            let highest = ((u64::from(value >> 8) + 1) << (32 - length)) - 1;
            *entry = (length, value & 0x80 != 0, highest);
        }
        let mut lowest = [0u64; 33];
        let mut highest = [0u64; 33];
        for length in 1..=32 {
            let low = u32_at(huff, ranges + 8 * (length - 1)).ok_or_else(damaged)?;
            let high = u32_at(huff, ranges + 8 * (length - 1) + 4).ok_or_else(damaged)?;
            lowest[length] = u64::from(low) << (32 - length);
            highest[length] = ((u64::from(high) + 1) << (32 - length)) - 1;
        }
        let mut phrases = Vec::new();
        for cdic in cdics {
            anyhow::ensure!(cdic.get(..8) == Some(b"CDIC\0\0\0\x10"), damaged());
            let count = u32_at(cdic, 8).ok_or_else(damaged)? as usize;
            let bits = u32_at(cdic, 12).ok_or_else(damaged)?;
            let here = (1usize << bits.min(20)).min(count.saturating_sub(phrases.len()));
            for n in 0..here {
                let at = 16 + 2 * n;
                let offset = usize::from(u16::from_be_bytes([
                    *cdic.get(at).ok_or_else(damaged)?,
                    *cdic.get(at + 1).ok_or_else(damaged)?,
                ]));
                let head = cdic.get(16 + offset..18 + offset).ok_or_else(damaged)?;
                let size = u16::from_be_bytes([head[0], head[1]]);
                let start = 18 + offset;
                let bytes = cdic
                    .get(start..start + usize::from(size & 0x7FFF))
                    .ok_or_else(damaged)?
                    .to_vec();
                phrases.push(if size & 0x8000 != 0 {
                    Phrase::Plain(bytes)
                } else {
                    Phrase::Coded(bytes)
                });
            }
        }
        Ok(Self {
            first,
            lowest,
            highest,
            phrases,
        })
    }

    /// `record` decoded, at most `limit` bytes of it.
    pub(crate) fn decode(&mut self, record: &[u8], limit: usize) -> anyhow::Result<Vec<u8>> {
        let mut out = Vec::new();
        self.unpack(record, limit, 0, &mut out)?;
        out.truncate(limit);
        Ok(out)
    }

    fn unpack(
        &mut self,
        data: &[u8],
        limit: usize,
        depth: usize,
        out: &mut Vec<u8>,
    ) -> anyhow::Result<()> {
        let damaged = || anyhow::anyhow!("This book's text is damaged");
        anyhow::ensure!(depth < DEPTH, damaged());
        // The 64 bits from byte `at`, zeros past the end.
        let window = |at: usize| {
            let mut bytes = [0u8; 8];
            for (k, byte) in bytes.iter_mut().enumerate() {
                *byte = data.get(at + k).copied().unwrap_or(0);
            }
            u64::from_be_bytes(bytes)
        };
        let mut left = i64::try_from(data.len()).map_err(|_| damaged())? * 8;
        let mut at = 0;
        let mut x = window(at);
        let mut n: i64 = 32;
        while out.len() < limit {
            if n <= 0 {
                at += 4;
                x = window(at);
                n += 32;
            }
            let code = (x >> n) & 0xFFFF_FFFF;
            let (mut length, ends, mut highest) = self.first[(code >> 24) as usize];
            if !ends {
                while length < 32 && code < self.lowest[length as usize] {
                    length += 1;
                }
                highest = self.highest[length as usize];
            }
            n -= i64::from(length);
            left -= i64::from(length);
            if left < 0 {
                break;
            }
            let index = highest
                .checked_sub(code)
                .and_then(|r| usize::try_from(r >> (32 - length)).ok())
                .ok_or_else(damaged)?;
            match self.phrases.get_mut(index).ok_or_else(damaged)? {
                Phrase::Plain(bytes) => out.extend_from_slice(bytes),
                Phrase::Open => anyhow::bail!(damaged()),
                phrase @ Phrase::Coded(_) => {
                    let Phrase::Coded(bytes) = std::mem::replace(phrase, Phrase::Open) else {
                        unreachable!("a coded phrase");
                    };
                    let mut decoded = Vec::new();
                    self.unpack(&bytes, limit, depth + 1, &mut decoded)?;
                    out.extend_from_slice(&decoded);
                    self.phrases[index] = Phrase::Plain(decoded);
                }
            }
        }
        Ok(())
    }
}

/// A HUFF record and one CDIC record coding each of `phrases` (the last
/// ones made of the first: coded, not plain) as one byte, and `text` (by
/// phrase number) coded with them: what the tests decode.
#[cfg(test)]
pub(crate) fn encode(
    phrases: &[&[u8]],
    coded: &[&[u8]],
    text: &[u8],
) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    let mut huff = b"HUFF\0\0\0\x18".to_vec();
    huff.extend_from_slice(&24u32.to_be_bytes());
    huff.extend_from_slice(&(24u32 + 1024).to_be_bytes());
    huff.extend_from_slice(&[0; 8]);
    // Every byte a code of 8 bits naming phrase `byte`: the largest code
    // of that length minus the code is the phrase's number; a byte past
    // the phrases names one past the dictionary.
    let all = phrases.len() + coded.len();
    for byte in 0..256u32 {
        let highest = if (byte as usize) < all {
            2 * byte
        } else {
            byte + 300
        };
        huff.extend_from_slice(&((highest << 8) | 0x80 | 8).to_be_bytes());
    }
    huff.extend_from_slice(&[0; 256]);
    let mut cdic = b"CDIC\0\0\0\x10".to_vec();
    cdic.extend_from_slice(&u32::try_from(all).unwrap().to_be_bytes());
    cdic.extend_from_slice(&8u32.to_be_bytes());
    let mut entries = Vec::new();
    let mut offsets = Vec::new();
    for (k, phrase) in phrases.iter().chain(coded).enumerate() {
        offsets.push(u16::try_from(2 * all + entries.len()).unwrap());
        let plain = k < phrases.len();
        let size = u16::try_from(phrase.len()).unwrap() | if plain { 0x8000 } else { 0 };
        entries.extend_from_slice(&size.to_be_bytes());
        entries.extend_from_slice(phrase);
    }
    for offset in offsets {
        cdic.extend_from_slice(&offset.to_be_bytes());
    }
    cdic.extend_from_slice(&entries);
    (huff, cdic, text.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_name_phrases_and_phrases_made_of_phrases() {
        // Phrase 3 is coded: phrases 0 and 1.
        let (huff, cdic, text) = encode(
            &[b"The needle", b" is here", b"."],
            &[&[0, 1]],
            &[3, 2, 0, 2],
        );
        let mut huffman = Huffman::new(&huff, [cdic.as_slice()]).unwrap();
        assert_eq!(
            huffman.decode(&text, 1 << 20).unwrap(),
            b"The needle is here.The needle."
        );
        assert_eq!(huffman.decode(&text, 7).unwrap(), b"The nee");
        // A phrase made of itself is damage, not a loop.
        let (huff, cdic, text) = encode(&[b"a"], &[&[1]], &[1]);
        let mut huffman = Huffman::new(&huff, [cdic.as_slice()]).unwrap();
        assert!(huffman.decode(&text, 1 << 20).is_err());
        // A code past the dictionary, or tables cut short, are damage.
        let (huff, cdic, _) = encode(&[b"a"], &[], &[]);
        let mut huffman = Huffman::new(&huff, [cdic.as_slice()]).unwrap();
        assert!(huffman.decode(&[9], 100).is_err());
        assert!(Huffman::new(&huff[..100], [cdic.as_slice()]).is_err());
    }
}
