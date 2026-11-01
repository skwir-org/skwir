//! A PDF packed tighter, without loss (ADR-112): what it stores twice kept
//! once (a form's same appearance drawn for each of its fields), what
//! nothing reaches let go of (an older revision's objects, the pages'
//! thumbnails, which readers draw again), Deflate written at its best,
//! and the objects that are not streams packed in compressed object
//! streams when it is saved.

use std::collections::{BTreeMap, HashMap};

use lopdf::{Dictionary, Document, Object, ObjectId};

use crate::samples;

/// The objects of `doc` stored twice (streams of the same dictionary and
/// data) kept once, every reference to another led to the one kept.
/// How many were let go of.
pub fn merge_duplicates(doc: &mut Document) -> usize {
    let mut kept: HashMap<(Vec<u8>, Vec<u8>), ObjectId> = HashMap::new();
    let mut to: BTreeMap<ObjectId, ObjectId> = BTreeMap::new();
    for (&id, object) in &doc.objects {
        let Object::Stream(stream) = object else {
            continue;
        };
        let mut dict = stream.dict.clone();
        dict.remove(b"Length");
        let key = (written(&dict), stream.content.clone());
        match kept.get(&key) {
            Some(&first) => {
                to.insert(id, first);
            }
            None => {
                kept.insert(key, id);
            }
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

/// Every stream compressed by Deflate alone written again at its best
/// (its predictor, if any, kept as it is: the bytes it predicted are
/// what is compressed), when that is smaller; one stored as it is
/// compressed, but the document's metadata, which some readers want
/// plain.
pub fn deflate_best(doc: &mut Document) {
    for object in doc.objects.values_mut() {
        let Object::Stream(stream) = object else {
            continue;
        };
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
            let mut raw = Vec::new();
            let read = std::io::Read::read_to_end(
                &mut std::io::Read::take(
                    flate2::read::ZlibDecoder::new(&stream.content[..]),
                    1 << 30,
                ),
                &mut raw,
            );
            if read.is_ok() {
                let again = samples::deflate(&raw);
                if again.len() < stream.content.len() {
                    stream.set_content(again);
                }
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

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::Stream;

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
}
