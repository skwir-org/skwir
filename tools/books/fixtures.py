#!/usr/bin/env python3
"""The books the book readers are tested on (ADR-106), all synthetic.

A small EPUB (a part over two chapters, a note, a picture, a cover) and an
FB2 written by hand, then calibre's own conversions of the EPUB: MOBI 6
(PalmDOC and uncompressed), MOBI 6 + KF8 in one file, AZW3 (KF8), an FB2,
and a PalmDOC text. Written to crates/plugins/books/testdata/.

    nix build github:NixOS/nixpkgs/nixos-unstable#calibre --out-link /tmp/calibre
    python3 tools/books/fixtures.py --calibre /tmp/calibre/bin/ebook-convert

calibre runs with a home of its own, so nothing of the machine's ends up in
a book; the script refuses an output that names this machine's user or
host.
"""

import argparse
import getpass
import os
import socket
import struct
import subprocess
import sys
import tempfile
import zipfile
import zlib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / "crates/plugins/books/testdata"


def png(width, height, colour):
    """A picture of one colour with a darker band across its middle."""
    def chunk(kind, data):
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body))
    rows = b""
    for y in range(height):
        band = height // 3 <= y < 2 * height // 3
        pixel = bytes(c // 2 for c in colour) if band else bytes(colour)
        rows += b"\x00" + pixel * width
    header = struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0)
    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", header)
            + chunk(b"IDAT", zlib.compress(rows, 9)) + chunk(b"IEND", b""))


COVER = png(60, 90, (40, 90, 160))
PICTURE = png(40, 30, (200, 60, 40))

XHTML = ('<?xml version="1.0" encoding="utf-8"?>\n'
         '<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">'
         '<head><title>{title}</title><link rel="stylesheet" href="style.css"/></head>'
         "<body>{body}</body></html>")

CHAPTERS = [
    ("one", "The First Chapter",
     '<h1 id="first">The First Chapter</h1>'
     "<p>The needle is here, in a sentence with a note"
     '<a epub:type="noteref" id="r1" href="notes.xhtml#n1">1</a>.</p>'
     '<p><img src="picture.png" alt="A red picture"/></p>'
     "<p>Words that only the first chapter has: alpha bravo.</p>"),
    ("two", "The Second Chapter",
     '<h1 id="second">The Second Chapter</h1>'
     "<p>Café, naïve and “quoted” words; nothing to see.</p>"
     '<p>Back to <a href="one.xhtml#first">the first chapter</a>.</p>'
     "<p><em>Emphasis</em>, <strong>strength</strong> and H<sub>2</sub>O.</p>"),
    # Long enough to span several of a MOBI's 4 KiB text records, with
    # characters of several bytes where a record may end.
    ("long", "The Long Chapter",
     '<h1 id="long">The Long Chapter</h1>'
     + "".join(f"<p>Paragraph {p}: " + " ".join(f"w{p:02}n{i:02}\u00e9\u2014" for i in range(24))
               + "</p>" for p in range(40))),
    ("three", "The Third Chapter",
     '<h1 id="third">The Third Chapter</h1>'
     "<p>Another needle, in the last chapter.</p>"
     "<blockquote><p>A quotation set apart.</p></blockquote>"),
    ("notes", "Notes",
     "<h1>Notes</h1>"
     '<aside epub:type="footnote" id="n1"><p><a href="one.xhtml#r1">1.</a> '
     "The note's own words.</p></aside>"),
]

NAV = ('<?xml version="1.0" encoding="utf-8"?>\n'
       '<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">'
       "<head><title>Contents</title></head><body>"
       '<nav epub:type="toc"><h1>Contents</h1><ol>'
       '<li><a href="one.xhtml">Part One</a><ol>'
       '<li><a href="one.xhtml#first">The First Chapter</a></li>'
       '<li><a href="two.xhtml">The Second Chapter</a></li>'
       '<li><a href="long.xhtml">The Long Chapter</a></li></ol></li>'
       '<li><a href="three.xhtml">Part Two</a><ol>'
       '<li><a href="three.xhtml#third">The Third Chapter</a></li></ol></li>'
       '<li><a href="notes.xhtml">Notes</a></li>'
       "</ol></nav></body></html>")

NCX = ('<?xml version="1.0" encoding="utf-8"?>\n'
       '<ncx xmlns="http://www.daisy.org/z3986/2005/ncx/" version="2005-1"><head>'
       '<meta name="dtb:uid" content="urn:uuid:5e1f8d2a-0b7c-4c3e-9a1d-2f6b8c4d7e90"/></head>'
       "<docTitle><text>Synthetic Book</text></docTitle><navMap>"
       '<navPoint id="p1" playOrder="1"><navLabel><text>Part One</text></navLabel>'
       '<content src="one.xhtml"/>'
       '<navPoint id="p2" playOrder="2"><navLabel><text>The First Chapter</text></navLabel>'
       '<content src="one.xhtml#first"/></navPoint>'
       '<navPoint id="p3" playOrder="3"><navLabel><text>The Second Chapter</text></navLabel>'
       '<content src="two.xhtml"/></navPoint>'
       '<navPoint id="p7" playOrder="4"><navLabel><text>The Long Chapter</text></navLabel>'
       '<content src="long.xhtml"/></navPoint></navPoint>'
       '<navPoint id="p4" playOrder="5"><navLabel><text>Part Two</text></navLabel>'
       '<content src="three.xhtml"/>'
       '<navPoint id="p5" playOrder="6"><navLabel><text>The Third Chapter</text></navLabel>'
       '<content src="three.xhtml#third"/></navPoint></navPoint>'
       '<navPoint id="p6" playOrder="7"><navLabel><text>Notes</text></navLabel>'
       '<content src="notes.xhtml"/></navPoint>'
       "</navMap></ncx>")


def epub(path):
    manifest = "".join(f'<item id="{name}" href="{name}.xhtml" media-type="application/xhtml+xml"/>'
                       for name, _, _ in CHAPTERS)
    spine = "".join(f'<itemref idref="{name}"/>' for name, _, _ in CHAPTERS)
    opf = ('<?xml version="1.0" encoding="utf-8"?>\n'
           '<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id">'
           '<metadata xmlns:dc="http://purl.org/dc/elements/1.1/">'
           '<dc:identifier id="id">urn:uuid:5e1f8d2a-0b7c-4c3e-9a1d-2f6b8c4d7e90</dc:identifier>'
           "<dc:title>Synthetic Book</dc:title><dc:creator>Synthetic Author</dc:creator>"
           "<dc:publisher>Synthetic Press</dc:publisher><dc:language>en</dc:language>"
           '<meta property="dcterms:modified">2026-10-30T00:00:00Z</meta>'
           '<meta name="cover" content="cover"/></metadata><manifest>'
           '<item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>'
           '<item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/>'
           '<item id="style" href="style.css" media-type="text/css"/>'
           '<item id="cover" href="cover.png" media-type="image/png" properties="cover-image"/>'
           '<item id="picture" href="picture.png" media-type="image/png"/>'
           f'{manifest}</manifest><spine toc="ncx">{spine}</spine></package>')
    with zipfile.ZipFile(path, "w") as book:
        book.writestr("mimetype", "application/epub+zip", compress_type=zipfile.ZIP_STORED)
        book.writestr("META-INF/container.xml",
                      '<?xml version="1.0"?><container version="1.0" '
                      'xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles>'
                      '<rootfile full-path="OEBPS/book.opf" media-type="application/oebps-package+xml"/>'
                      "</rootfiles></container>")
        book.writestr("OEBPS/book.opf", opf)
        book.writestr("OEBPS/nav.xhtml", NAV)
        book.writestr("OEBPS/toc.ncx", NCX)
        book.writestr("OEBPS/style.css", "blockquote { margin-left: 2em; font-style: italic }")
        book.writestr("OEBPS/cover.png", COVER)
        book.writestr("OEBPS/picture.png", PICTURE)
        for name, title, body in CHAPTERS:
            book.writestr(f"OEBPS/{name}.xhtml", XHTML.format(title=title, body=body))


def b64(data):
    import base64
    text = base64.b64encode(data).decode()
    return "\n".join(text[i:i + 76] for i in range(0, len(text), 76))


def fb2(title, lang, body, notes, encoding="utf-8", author=("Synthetic", "Author")):
    """A FictionBook 2 document: `body` and `notes` are its two bodies' inner
    XML, with the cover and the picture as its binaries."""
    return (f'<?xml version="1.0" encoding="{encoding}"?>\n'
            '<FictionBook xmlns="http://www.gribuser.ru/xml/fictionbook/2.0" '
            'xmlns:l="http://www.w3.org/1999/xlink">'
            "<description><title-info><genre>prose_contemporary</genre>"
            f"<author><first-name>{author[0]}</first-name><last-name>{author[1]}</last-name></author>"
            f"<book-title>{title}</book-title>"
            "<annotation><p>A book made for tests.</p></annotation>"
            '<coverpage><image l:href="#cover.png"/></coverpage>'
            f'<lang>{lang}</lang><sequence name="Synthetic Series" number="2"/></title-info>'
            "<document-info><author><nickname>Synthetic Author</nickname></author>"
            "<date>2026-10-30</date><id>synthetic-fb2</id><version>1.0</version></document-info>"
            "<publish-info><publisher>Synthetic Press</publisher><year>2026</year></publish-info>"
            f"</description><body>{body}</body>"
            f'<body name="notes"><title><p>Notes</p></title>{notes}</body>'
            f'<binary id="cover.png" content-type="image/png">{b64(COVER)}</binary>'
            f'<binary id="picture.png" content-type="image/png">{b64(PICTURE)}</binary>'
            "</FictionBook>\n").encode(encoding)


FB2_BODY = (
    "<title><p>Synthetic Book</p></title>"
    '<epigraph><p>An epigraph before it all.</p><text-author>Someone Synthetic</text-author></epigraph>'
    '<section id="part1"><title><p>Part One</p></title>'
    '<section id="first"><title><p>The First</p><p>Chapter</p></title>'
    '<p>The needle is here, in a sentence with a note<a l:href="#n1" type="note">[1]</a>.</p>'
    '<image l:href="#picture.png"/>'
    "<p>Words that only the first chapter has: alpha bravo.</p></section>"
    '<section id="second"><title><p>The Second Chapter</p></title>'
    "<subtitle>A subtitle</subtitle>"
    "<p>Café, naïve and “quoted” words; <emphasis>emphasis</emphasis>, "
    "<strong>strength</strong>, <strikethrough>struck</strikethrough>, H<sub>2</sub>O, "
    "<code>code</code>.</p><empty-line/>"
    '<p>Back to <a l:href="#first">the first chapter</a>.</p>'
    "<poem><title><p>A Poem</p></title><stanza><v>First line of verse,</v>"
    "<v>second line of verse.</v></stanza></poem>"
    "<cite><p>A citation set apart.</p><text-author>Its Author</text-author></cite>"
    "<table><tr><th>Name</th><th>Value</th></tr><tr><td>one</td><td>1</td></tr></table>"
    "</section></section>"
    '<section id="part2"><title><p>Part Two</p></title>'
    '<section id="third"><title><p>The Third Chapter</p></title>'
    "<p>Another needle, in the last chapter.</p></section></section>")

FB2_NOTES = '<section id="n1"><title><p>1</p></title><p>The note\'s own words.</p></section>'

# Russian books are often FB2 in Windows-1251.
FB2_CYRILLIC_BODY = (
    '<section id="glava1"><title><p>Глава первая</p></title>'
    "<p>Синтетический текст для проверки кодировки.</p></section>"
    '<section id="glava2"><title><p>Глава вторая</p></title>'
    "<p>Ещё немного синтетического текста.</p></section>")


def run(calibre, *args):
    env = {key: value for key, value in os.environ.items()
           if key in ("PATH", "LANG", "LC_ALL", "TZ", "NIX_SSL_CERT_FILE")}
    home = tempfile.mkdtemp(prefix="calibre-home-")
    env.update(HOME=home, CALIBRE_CONFIG_DIRECTORY=f"{home}/config",
               CALIBRE_TEMP_DIR=f"{home}/tmp", QT_QPA_PLATFORM="offscreen", TZ="UTC")
    os.makedirs(f"{home}/tmp")
    subprocess.run([calibre, *map(str, args)], env=env, check=True,
                   stdout=subprocess.DEVNULL)


def private(path):
    """Whether a book names this machine's user or host."""
    data = path.read_bytes()
    names = {getpass.getuser(), socket.gethostname(), str(Path.home())}
    return [name for name in names if name and name.encode() in data]


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--calibre", required=True, help="calibre's ebook-convert")
    args = parser.parse_args()
    OUT.mkdir(parents=True, exist_ok=True)
    source = OUT / "synthetic.epub"
    epub(source)
    (OUT / "synthetic.fb2").write_bytes(fb2("Synthetic Book", "en", FB2_BODY, FB2_NOTES))
    with zipfile.ZipFile(OUT / "synthetic.fb2.zip", "w", zipfile.ZIP_DEFLATED) as packed:
        packed.write(OUT / "synthetic.fb2", "Synthetic book.fb2")
    (OUT / "synthetic-1251.fb2").write_bytes(
        fb2("Синтетическая книга", "ru", FB2_CYRILLIC_BODY, FB2_NOTES, "windows-1251",
            ("Синтетический", "Автор")))
    common = ["--title", "Synthetic Book", "--authors", "Synthetic Author"]
    # The book has its own contents: no list of chapters added to the text.
    kindle = ["--no-inline-toc"]
    for name, extra in [
        ("synthetic.mobi", [*kindle, "--mobi-file-type", "old"]),
        ("synthetic-uncompressed.mobi", [*kindle, "--mobi-file-type", "old", "--dont-compress"]),
        ("synthetic-both.mobi", [*kindle, "--mobi-file-type", "both"]),
        ("synthetic.azw3", kindle),
        ("synthetic-calibre.fb2", []),
    ]:
        run(args.calibre, source, OUT / name, *common, *extra)
    text = Path(tempfile.mkdtemp(prefix="books-")) / "Synthetic text.txt"
    text.write_text("Synthetic Text\n\nThe needle is here, in a plain PalmDOC text.\n\n"
                    "A second paragraph, with its own words.\n")
    run(args.calibre, text, OUT / "synthetic.pdb", "--format", "doc",
        "--title", "Synthetic Text", "--authors", "Synthetic Author")
    leaks = {path.name: private(path) for path in OUT.iterdir() if private(path)}
    if leaks:
        sys.exit(f"these books name this machine: {leaks}")
    for path in sorted(OUT.iterdir()):
        print(f"{path.stat().st_size:7} {path.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
