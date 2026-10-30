//! Mobipocket's Huffman coding with a dictionary (HUFF and CDIC records):
//! each code names a phrase of the dictionary, itself coded the same way
//! unless marked as plain bytes. The CDIC records are held once and a
//! phrase is a span into them, never a copy. A coded phrase is decoded once,
//! the first time it is met, and the dictionary keeps no more than the text
//! bound of what it decodes; one that leads back to itself is damage.
use skwir_core::CancelToken;
use skwir_platform::inspection;

/// Phrases decoded within phrases, at most: deeper is damage.
const DEPTH: usize = 32;
/// The decoding loop checks for cancellation this often: a record of codes
/// that each name an empty phrase yields nothing but still walks the record.
const CHECK: u32 = 1 << 13;

/// The tables of a HUFF record and the phrases of its CDIC records.
pub(crate) struct Huffman {
    /// By a code's first byte: its length, whether that byte ends it, the
    /// largest code of that length.
    first: [(u32, bool, u64); 256],
    /// By length: the smallest and largest codes of that length.
    lowest: [u64; 33],
    highest: [u64; 33],
    /// The CDIC records, held once; a phrase is a span into one of them.
    cdics: Vec<Vec<u8>>,
    phrases: Vec<Phrase>,
    /// Bytes the decoded dictionary may still keep: each coded phrase is
    /// kept as it decodes, all of them within the text bound.
    budget: usize,
}

/// One phrase: a span of a CDIC record, and, once a coded phrase is met, its
/// decoding kept for next time.
struct Phrase {
    cdic: usize,
    at: usize,
    len: usize,
    plain: bool,
    state: State,
}

enum State {
    /// A span not yet decoded (a coded phrase), or one never decoded (plain).
    Raw,
    /// Being decoded: met again, the dictionary loops.
    Open,
    /// A coded phrase decoded once, kept.
    Done(Vec<u8>),
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    let b = bytes.get(at..at.checked_add(4)?)?;
    Some(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

impl Huffman {
    /// The coding the `huff` record and its `cdics` give, whose decoded
    /// dictionary stays within `budget` bytes.
    pub(crate) fn new<'a>(
        huff: &[u8],
        cdics: impl IntoIterator<Item = &'a [u8]>,
        budget: usize,
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
        // The records are held once; a phrase names the record it is in and
        // where in it, so a dictionary of a million phrases is a million
        // spans, not a million copies of their records' bytes.
        let cdics: Vec<Vec<u8>> = cdics.into_iter().map(<[u8]>::to_vec).collect();
        let mut phrases = Vec::new();
        for (index, cdic) in cdics.iter().enumerate() {
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
                let len = usize::from(size & 0x7FFF);
                anyhow::ensure!(cdic.get(start..start + len).is_some(), damaged());
                phrases.push(Phrase {
                    cdic: index,
                    at: start,
                    len,
                    plain: size & 0x8000 != 0,
                    state: State::Raw,
                });
            }
        }
        Ok(Self {
            first,
            lowest,
            highest,
            cdics,
            phrases,
            budget,
        })
    }

    /// `record` decoded, at most `limit` bytes of it.
    pub(crate) fn decode(
        &mut self,
        record: &[u8],
        limit: usize,
        cancel: &CancelToken,
    ) -> anyhow::Result<Vec<u8>> {
        let mut out = Vec::new();
        self.unpack(record, limit, 0, cancel, &mut out)?;
        out.truncate(limit);
        Ok(out)
    }

    fn unpack(
        &mut self,
        data: &[u8],
        limit: usize,
        depth: usize,
        cancel: &CancelToken,
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
        let mut steps: u32 = 0;
        while out.len() < limit {
            // A record of codes each naming an empty phrase yields nothing
            // but still walks the record; cancellation stops it at once.
            steps = steps.wrapping_add(1);
            if steps.is_multiple_of(CHECK) {
                inspection::check(cancel)?;
            }
            if n <= 0 {
                at += 4;
                x = window(at);
                n += 32;
            }
            let code = (x >> n) & 0xFFFF_FFFF;
            let (mut length, ends, mut top) = self.first[(code >> 24) as usize];
            if !ends {
                while length < 32 && code < self.lowest[length as usize] {
                    length += 1;
                }
                top = self.highest[length as usize];
            }
            n -= i64::from(length);
            left -= i64::from(length);
            if left < 0 {
                break;
            }
            let index = top
                .checked_sub(code)
                .and_then(|r| usize::try_from(r >> (32 - length)).ok())
                .ok_or_else(damaged)?;
            // Decode or emit phrase `index`. The immutable borrow ends
            // before a coded phrase is decoded and cached.
            let coded = {
                let phrase = self.phrases.get(index).ok_or_else(damaged)?;
                match &phrase.state {
                    State::Open => anyhow::bail!(damaged()),
                    State::Done(bytes) => {
                        out.extend_from_slice(bytes);
                        None
                    }
                    State::Raw => {
                        let bytes = self
                            .cdics
                            .get(phrase.cdic)
                            .and_then(|cdic| cdic.get(phrase.at..phrase.at + phrase.len))
                            .ok_or_else(damaged)?;
                        if phrase.plain {
                            out.extend_from_slice(bytes);
                            None
                        } else {
                            Some(bytes.to_vec())
                        }
                    }
                }
            };
            if let Some(raw) = coded {
                self.phrases[index].state = State::Open;
                let mut decoded = Vec::new();
                self.unpack(&raw, limit, depth + 1, cancel, &mut decoded)?;
                // A phrase is kept as it decodes, all of them within the
                // text bound.
                self.budget = self.budget.checked_sub(decoded.len()).ok_or_else(|| {
                    anyhow::anyhow!("This book's dictionary is too large to read")
                })?;
                out.extend_from_slice(&decoded);
                self.phrases[index].state = State::Done(decoded);
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

    /// The budget every real decode gives: more than any novel's text.
    const BUDGET: usize = 64 * 1024 * 1024;

    #[test]
    fn codes_name_phrases_and_phrases_made_of_phrases() {
        let cancel = CancelToken::new();
        // Phrase 3 is coded: phrases 0 and 1.
        let (huff, cdic, text) = encode(
            &[b"The needle", b" is here", b"."],
            &[&[0, 1]],
            &[3, 2, 0, 2],
        );
        let mut huffman = Huffman::new(&huff, [cdic.as_slice()], BUDGET).unwrap();
        assert_eq!(
            huffman.decode(&text, 1 << 20, &cancel).unwrap(),
            b"The needle is here.The needle."
        );
        assert_eq!(huffman.decode(&text, 7, &cancel).unwrap(), b"The nee");
        // A phrase made of itself is damage, not a loop.
        let (huff, cdic, text) = encode(&[b"a"], &[&[1]], &[1]);
        let mut huffman = Huffman::new(&huff, [cdic.as_slice()], BUDGET).unwrap();
        assert!(huffman.decode(&text, 1 << 20, &cancel).is_err());
        // A code past the dictionary, or tables cut short, are damage.
        let (huff, cdic, _) = encode(&[b"a"], &[], &[]);
        let mut huffman = Huffman::new(&huff, [cdic.as_slice()], BUDGET).unwrap();
        assert!(huffman.decode(&[9], 100, &cancel).is_err());
        assert!(Huffman::new(&huff[..100], [cdic.as_slice()], BUDGET).is_err());
    }

    /// A phrase decoded to more than the budget is refused, and a cancelled
    /// token stops a decode that yields nothing.
    #[test]
    fn a_runaway_dictionary_and_a_cancel_both_stop() {
        // Five phrases, each naming the next, the last 200 copies of a
        // two-byte phrase: 2000 bytes of dictionary, past a 1500-byte budget.
        let bottom: &[u8] = &[0u8; 200];
        let chain: [&[u8]; 5] = [bottom, &[1u8], &[2u8], &[3u8], &[4u8]];
        let (huff, cdic, _) = encode(&[b"xx"], &chain, &[]);
        let mut huffman = Huffman::new(&huff, [cdic.as_slice()], 1500).unwrap();
        let error = huffman
            .decode(&[5], 1 << 20, &CancelToken::new())
            .unwrap_err()
            .to_string();
        assert!(error.contains("dictionary"), "{error}");
        // A record of codes naming an empty phrase, with a cancelled token.
        let (huff, cdic, _) = encode(&[b""], &[], &[]);
        let mut huffman = Huffman::new(&huff, [cdic.as_slice()], BUDGET).unwrap();
        let cancel = CancelToken::new();
        cancel.cancel();
        assert!(
            huffman
                .decode(&vec![0u8; 1 << 16], 1 << 20, &cancel)
                .is_err()
        );
    }
}
