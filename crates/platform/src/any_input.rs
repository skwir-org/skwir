//! What the platform reads of the system and of files (links and paths
//! in text, the volumes and folders the system lists, thumbnailers and
//! applications' entries, users, a document's XHTML, text in any
//! encoding, addresses, a package) given what no file should hold: a
//! sample mutated, and anything at random. It may refuse it; it never
//! panics.

use std::path::Path;

use bibli_core::testing::{bytes, mutated, mutated_text};
use proptest::prelude::*;

const LINKS: &str = "see https://example.org/a_(b)?q=1#x, mailto:a@example.org and \
\"src/main.rs:12:5\" or 'My Files/café.txt':3 then ~/notes.md and ./a\\ b.txt";

const MOUNTINFO: &str = "22 1 0:30 /@ / rw,relatime shared:1 - btrfs /dev/mapper/root rw\n\
40 22 0:52 / /run/media/u/Back\\040up rw,nosuid - vfat /dev/sdb1 rw\n\
41 22 0:53 /sub /mnt/x rw - fuse.sshfs user@host:/ rw\n";

const LSBLK: &str = r#"{"blockdevices":[{"path":"/dev/sdb1","label":"stick","uuid":"1234-ABCD",
"size":1024000000,"fstype":"vfat","mountpoint":null,"hotplug":true,"pkname":"sdb",
"children":[{"path":"/dev/mapper/luks-1","fstype":"ext4","mountpoint":"/run/media/u/x"}]}]}"#;

const USER_DIRS: &str = "# comment\nXDG_DOWNLOAD_DIR=\"$HOME/dl\"\nXDG_PICTURES_DIR=\"$HOME/My Pictures\"\nXDG_MUSIC_DIR=\"/abs/music\"\n";

const BOOKMARKS: &str =
    "file:///home/a/My%20Files Files\nfile:///home/a/caf%C3%A9\nsftp://host/x Remote\n";

const THUMBNAILER: &str = "[Thumbnailer Entry]\nTryExec=evince-thumbnailer\nExec=evince-thumbnailer -s %s %u %o\nMimeType=application/pdf;application/x-bzpdf;\n";

const DESKTOP: &str = "[Desktop Entry]\nType=Application\nName=Editor\nName[fr]=Éditeur\n\
Exec=editor --new %F \"quoted arg\" %%\nMimeType=text/plain;text/x-c;\nTerminal=false\n\
NoDisplay=false\n[Desktop Action new]\nName=New\nExec=editor --new\n";

const PASSWD: &str =
    "root:x:0:0:root:/root:/bin/sh\n# comment\nada:x:1000:100::/home/ada:/bin/sh\n";

const XHTML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<html xmlns="http://www.w3.org/1999/xhtml"><head><title>T &amp; t</title>
<style>p { color: red }</style></head><body><h1 id="a">Café &eacute; &#233; &#x2014;</h1>
<p>One <em>two</em> <a href="b.xhtml#c">three</a></p><img src="p.png" alt="x"/></body></html>"#;

fn zip() -> Vec<u8> {
    use std::io::Write;
    let mut out = std::io::Cursor::new(Vec::new());
    let mut zip = zip::ZipWriter::new(&mut out);
    let stored =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    zip.start_file("mimetype", stored).unwrap();
    zip.write_all(b"application/epub+zip").unwrap();
    zip.start_file("OEBPS/café.xhtml", zip::write::SimpleFileOptions::default())
        .unwrap();
    zip.write_all(XHTML.as_bytes()).unwrap();
    zip.finish().unwrap();
    out.into_inner()
}

proptest! {
    #[test]
    fn links_and_paths_in_text(text in prop_oneof![mutated_text(LINKS), any::<String>()]) {
        let _ = crate::links::urls(&text);
        for word in crate::links::paths(&text) {
            let _ = crate::links::resolve(&word.path, Path::new("/home/a"));
        }
    }

    #[test]
    fn what_the_system_lists(text in prop_oneof![
        mutated_text(MOUNTINFO),
        mutated_text(LSBLK),
        mutated_text(USER_DIRS),
        mutated_text(BOOKMARKS),
        any::<String>(),
    ]) {
        let _ = crate::places::parse_mountinfo(&text);
        let _ = crate::places::parse_lsblk(&text);
        let _ = crate::places::parse_user_dirs(&text, Path::new("/home/a"));
        let _ = crate::places::parse_bookmarks(&text);
    }

    #[test]
    fn entries_of_applications_and_thumbnailers(text in prop_oneof![
        mutated_text(DESKTOP),
        mutated_text(THUMBNAILER),
        any::<String>(),
    ]) {
        let _ = crate::thumb_providers::parse_thumbnailer("t.thumbnailer", &text);
        let _ = crate::apps::parse_entry(
            "editor.desktop",
            Path::new("/usr/share/applications/editor.desktop"),
            &text,
            &crate::apps::Locale::default(),
            &["GNOME".to_owned()],
        );
        for (_, key, value) in crate::keyfile::entries(&text) {
            let _ = crate::keyfile::unescape(value);
            let _ = crate::keyfile::list(value);
            if key == "Exec" {
                let _ = crate::keyfile::exec_words(value);
            }
        }
    }

    #[test]
    fn users_and_addresses(text in prop_oneof![
        mutated_text(PASSWD),
        mutated_text("file:///home/a/caf%C3%A9%20notes.txt"),
        any::<String>(),
    ]) {
        let _ = crate::users::parse(&text);
        let _ = crate::uri::path_of(&text);
        let _ = crate::uri::percent_decode(&text);
        let _ = crate::package::normalize(&text);
    }

    #[test]
    fn a_document_s_xhtml(text in prop_oneof![mutated_text(XHTML), any::<String>()]) {
        let _ = crate::rich_preview::xml::parse(&text);
    }

    #[test]
    fn text_in_any_encoding(input in prop_oneof![
        mutated(&b"\xef\xbb\xbfcaf\xc3\xa9\n"[..]),
        mutated(&b"\xff\xfec\x00a\x00f\x00\xe9\x00"[..]),
        bytes(1024),
    ]) {
        let _ = crate::inspection::decode(&input);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    #[test]
    fn a_package(input in prop_oneof![mutated(zip()), bytes(512)]) {
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(file.path(), &input).unwrap();
        let cancel = bibli_core::cancel::CancelToken::new();
        if let Ok(mut package) =
            crate::package::Package::open(file.path(), "This package", false, &cancel)
        {
            let names: Vec<String> = package.names().map(str::to_owned).collect();
            for name in names {
                let _ = package.head(&name, 64);
                let _ = package.read(&name, 4096, "This member", &cancel);
            }
        }
    }
}
