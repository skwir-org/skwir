//! A PDF packed tighter, without loss (ADR-112): what it stores twice kept
//! once (a form's same appearance drawn for each of its fields), what
//! nothing reaches let go of (an older revision's objects, the pages'
//! thumbnails, which readers draw again), Deflate written at its best,
//! and the objects that are not streams packed in compressed object
//! streams when it is saved.

use std::collections::hash_map::DefaultHasher;
use std::collections::{BTreeMap, HashMap};
use std::hash::{Hash, Hasher};

use lopdf::{Dictionary, Document, Object, ObjectId, Stream};

use crate::samples;

/// The objects of `doc` stored twice (streams of the same dictionary and
/// data) kept once, every reference to another led to the one kept.
/// How many were let go of. Streams are told apart by their dictionary,
/// their data's length and a hash of it, and compared byte for byte only
/// when those agree: no stream's data is copied to be compared.
pub fn merge_duplicates(doc: &mut Document) -> usize {
    let mut kept: HashMap<(Vec<u8>, usize, u64), Vec<ObjectId>> = HashMap::new();
    let mut to: BTreeMap<ObjectId, ObjectId> = BTreeMap::new();
    let content = |id: &ObjectId| match doc.objects.get(id) {
        Some(Object::Stream(stream)) => Some(&stream.content),
        _ => None,
    };
    for (&id, object) in &doc.objects {
        let Object::Stream(stream) = object else {
            continue;
        };
        let mut dict = stream.dict.clone();
        dict.remove(b"Length");
        let mut hasher = DefaultHasher::new();
        stream.content.hash(&mut hasher);
        let key = (written(&dict), stream.content.len(), hasher.finish());
        let same = kept.entry(key).or_default();
        match same
            .iter()
            .find(|first| content(first) == Some(&stream.content))
        {
            Some(&first) => {
                to.insert(id, first);
            }
            None => same.push(id),
        }
    }
    if to.is_empty() {
        return 0;
    }
    for object in doc.objects.values_mut() {
        led(object, &to);
    }
    led_dict(&mut doc.trailer, &to);
    for id in to.keys() {
        doc.objects.remove(id);
    }
    to.len()
}

/// `dict` spelt out, its entries in their order: what tells two
/// dictionaries apart (the same entries in another order count as two,
/// which only keeps both).
fn written(dict: &Dictionary) -> Vec<u8> {
    format!("{dict:?}").into_bytes()
}

/// Every reference in `object` to one let go of led to the one kept.
fn led(object: &mut Object, to: &BTreeMap<ObjectId, ObjectId>) {
    match object {
        Object::Reference(id) => {
            if let Some(&kept) = to.get(id) {
                *id = kept;
            }
        }
        Object::Array(items) => {
            for item in items {
                led(item, to);
            }
        }
        Object::Dictionary(dict) => led_dict(dict, to),
        Object::Stream(stream) => led_dict(&mut stream.dict, to),
        _ => {}
    }
}

fn led_dict(dict: &mut Dictionary, to: &BTreeMap<ObjectId, ObjectId>) {
    for (_, value) in dict.iter_mut() {
        led(value, to);
    }
}

/// The pages' thumbnails let go of: readers draw their own.
pub fn drop_thumbnails(doc: &mut Document) {
    let pages: Vec<ObjectId> = doc.get_pages().into_values().collect();
    for page in pages {
        if let Ok(dict) = doc.get_dictionary_mut(page) {
            dict.remove(b"Thumb");
        }
    }
}

/// The most a stream is inflated to be compressed again: one holding more
/// is left as it is.
const INFLATED: u64 = 1 << 30;

/// Every stream `which` names compressed by Deflate alone written again
/// at its best (its predictor, if any, kept as it is: the bytes it
/// predicted are what is compressed), when that is smaller; one stored as
/// it is compressed, but the document's metadata, which some readers want
/// plain. `beat` is called after each, as it may take long.
pub fn deflate_best(
    doc: &mut Document,
    which: impl Fn(&ObjectId, &Stream) -> bool,
    beat: &mut dyn FnMut(),
) {
    for (id, object) in &mut doc.objects {
        let Object::Stream(stream) = object else {
            continue;
        };
        if !which(id, stream) {
            continue;
        }
        beat();
        let is_metadata = stream
            .dict
            .get(b"Type")
            .and_then(Object::as_name)
            .is_ok_and(|kind| kind == b"Metadata");
        let filter = stream.dict.get(b"Filter").ok();
        let deflate_alone = match filter {
            Some(Object::Name(name)) => name == b"FlateDecode",
            Some(Object::Array(items)) => {
                items.len() == 1 && items[0].as_name().is_ok_and(|name| name == b"FlateDecode")
            }
            _ => false,
        };
        if deflate_alone {
            if let Some(again) = deflated_again(&stream.content, INFLATED) {
                stream.set_content(again);
            }
        } else if filter.is_none() && !is_metadata && !stream.content.is_empty() {
            let deflated = samples::deflate(&stream.content);
            if deflated.len() < stream.content.len() {
                stream
                    .dict
                    .set("Filter", Object::Name(b"FlateDecode".to_vec()));
                stream.set_content(deflated);
            }
        }
        stream.allows_compression = false;
    }
}

/// `content`, Deflate data, compressed again at its best, when that is
/// smaller: none when it is not, when it cannot be read, or when it holds
/// more than `most` bytes, which reading it to `most` would cut short.
fn deflated_again(content: &[u8], most: u64) -> Option<Vec<u8>> {
    let mut raw = Vec::new();
    // One past the most, to tell data that holds more from data that
    // holds exactly that much.
    std::io::Read::read_to_end(
        &mut std::io::Read::take(flate2::read::ZlibDecoder::new(content), most + 1),
        &mut raw,
    )
    .ok()?;
    if raw.len() as u64 > most {
        return None;
    }
    let again = samples::deflate(&raw);
    (again.len() < content.len()).then_some(again)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stream_stored_twice_is_kept_once() {
        let mut doc = Document::with_version("1.5");
        let first = doc.add_object(Stream::new(Dictionary::new(), b"0 0 m 10 10 l S".to_vec()));
        let second = doc.add_object(Stream::new(Dictionary::new(), b"0 0 m 10 10 l S".to_vec()));
        let other = doc.add_object(Stream::new(Dictionary::new(), b"1 1 m".to_vec()));
        let holder = doc.add_object(Object::Array(vec![
            Object::Reference(first),
            Object::Reference(second),
            Object::Reference(other),
        ]));
        assert_eq!(merge_duplicates(&mut doc), 1);
        let Object::Array(items) = doc.get_object(holder).unwrap() else {
            panic!("an array");
        };
        assert_eq!(items[0], items[1]);
        assert_ne!(items[0], items[2]);
        assert!(!doc.objects.contains_key(&second));
    }

    /// Streams of the same dictionary and length but other data are two.
    #[test]
    fn streams_of_one_length_are_compared_by_their_data() {
        let mut doc = Document::with_version("1.5");
        let first = doc.add_object(Stream::new(Dictionary::new(), b"0 0 m 10 10 l S".to_vec()));
        let other = doc.add_object(Stream::new(Dictionary::new(), b"0 0 m 10 20 l S".to_vec()));
        doc.add_object(Object::Array(vec![
            Object::Reference(first),
            Object::Reference(other),
        ]));
        assert_eq!(merge_duplicates(&mut doc), 0);
        assert!(doc.objects.contains_key(&first) && doc.objects.contains_key(&other));
    }

    /// Deflate data holding more than is read is left as it is, never
    /// written again cut short.
    #[test]
    fn deflate_holding_more_than_is_read_is_left_whole() {
        // Words drawn from a few, as a page's drawing repeats itself.
        let words = [
            "0 0 m ",
            "10 10 l ",
            "S ",
            "q ",
            "Q ",
            "1 0 0 1 72 72 cm ",
            "BT ",
        ];
        let mut seed = 0x9E37_79B9_u32;
        let mut raw = Vec::new();
        while raw.len() < 20_000 {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            raw.extend_from_slice(words[seed as usize % words.len()].as_bytes());
        }
        raw.truncate(20_000);
        let mut fast = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
        std::io::Write::write_all(&mut fast, &raw).unwrap();
        let fast = fast.finish().unwrap();
        assert!(deflated_again(&fast, 10_000).is_none());
        let again = deflated_again(&fast, 20_000).expect("smaller at its best");
        let mut back = Vec::new();
        std::io::Read::read_to_end(&mut flate2::read::ZlibDecoder::new(&again[..]), &mut back)
            .unwrap();
        assert_eq!(back, raw);
    }
}
