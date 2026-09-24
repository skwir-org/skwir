"""Deterministic PDF fixtures. No real files, names, or metadata are copied."""
from pathlib import Path


def write_pdf(path, objects):
    data = bytearray(b"%PDF-1.4\n")
    offsets = [0]
    for index, obj in enumerate(objects, 1):
        offsets.append(len(data))
        data += f"{index} 0 obj\n".encode() + obj + b"\nendobj\n"
    xref = len(data)
    data += f"xref\n0 {len(offsets)}\n0000000000 65535 f \n".encode()
    for offset in offsets[1:]:
        data += f"{offset:010} 00000 n \n".encode()
    data += f"trailer\n<< /Size {len(offsets)} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n".encode()
    Path(path).write_bytes(data)


def stream(data):
    return f"<< /Length {len(data)} >>\nstream\n".encode() + data + b"\nendstream"


def reader(path, kind="text", crop=False, angle=90, named=False):
    objects = [b"", b"", b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>"]
    page_ids = []
    image = None
    if kind == "scan":
        width, height = 1600, 2200
        pixels = bytearray(width * height)
        state = 1
        for y in range(height):
            for x in range(width):
                state = (state * 1664525 + 1013904223) & 0xffffffff
                ink = 100 < x < 1500 and 100 < y < 2100 and y % 32 < 5 and x % 19 < 13
                pixels[y * width + x] = (35 if ink else 230) + (state >> 24) % 25
        objects.append(f"<< /Type /XObject /Subtype /Image /Width {width} /Height {height} /ColorSpace /DeviceGray /BitsPerComponent 8 /Length {len(pixels)} >>\nstream\n".encode() + pixels + b"\nendstream")
        image = len(objects)
    for number in range(3):
        text = f"BT /F1 18 Tf 60 720 Td (Synthetic Reader {number + 1}) Tj 0 -40 Td (needle alpha beta) Tj 0 -40 Td (External web link) Tj 0 -40 Td (Go to second page) Tj ET\n".encode()
        if kind == "vector":
            text += b"0.2 0.4 0.7 RG\n" + b"".join(f"{i % 500 + 50} 50 m {550 - i % 500} 500 l S\n".encode() for i in range(1500))
        if image:
            text = b"q 600 0 0 800 0 0 cm /Im0 Do Q\n"
        objects.append(stream(text))
        content = len(objects)
        rotation = f" /Rotate {angle}" if number == 1 and kind == "text" else ""
        if crop and number >= 1:
            rotation += " /CropBox [20 30 580 770]"
        resources = f"/Font << /F1 3 0 R >>" + (f" /XObject << /Im0 {image} 0 R >>" if image else "")
        objects.append(f"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 600 800]{rotation} /Resources << {resources} >> /Contents {content} 0 R >>".encode())
        page_ids.append(len(objects))
    objects.append(b"<< /Type /Annot /Subtype /Link /Rect [60 635 240 655] /Border [0 0 0] /A << /S /URI /URI (https://example.org/) >> >>")
    web = len(objects)
    destination = "(chapter2)" if named else f"[{page_ids[1]} 0 R /Fit]"
    objects.append(f"<< /Type /Annot /Subtype /Link /Rect [60 595 240 615] /Border [0 0 0] /Dest {destination} >>".encode())
    local = len(objects)
    for page in page_ids:
        objects[page - 1] = objects[page - 1][:-2] + f" /Annots [{web} 0 R {local} 0 R] >>".encode()
    outline = len(objects) + 1
    objects.append(f"<< /Type /Outlines /First {outline + 1} 0 R /Last {outline + 2} 0 R /Count 2 >>".encode())
    objects.append(f"<< /Title (Introduction) /Parent {outline} 0 R /Next {outline + 2} 0 R /Dest [{page_ids[0]} 0 R /Fit] >>".encode())
    objects.append(f"<< /Title (Second chapter) /Parent {outline} 0 R /Prev {outline + 1} 0 R /Dest {destination} >>".encode())
    names = f" /Dests << /chapter2 [{page_ids[1]} 0 R /Fit] >>" if named else ""
    objects[0] = f"<< /Type /Catalog /Pages 2 0 R /Outlines {outline} 0 R{names} >>".encode()
    objects[1] = ("<< /Type /Pages /Count 3 /Kids [" + " ".join(f"{p} 0 R" for p in page_ids) + "] >>").encode()
    write_pdf(path, objects)


if __name__ == "__main__":
    import sys
    root = Path(sys.argv[1])
    root.mkdir(parents=True, exist_ok=True)
    for kind in ("text", "vector", "scan"):
        reader(root / f"Synthetic {kind}.pdf", kind)


def book(path, chapters=3, paragraphs=12):
    """A reflowable EPUB in which every word is unique ("w1p07n23": chapter,
    paragraph, word), so a relaid-out page can be found by its first words."""
    import zipfile
    with zipfile.ZipFile(path, "w") as epub:
        epub.writestr("mimetype", "application/epub+zip", compress_type=zipfile.ZIP_STORED)
        epub.writestr("META-INF/container.xml",
                      '<?xml version="1.0"?><container version="1.0" '
                      'xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles>'
                      '<rootfile full-path="book.opf" media-type="application/oebps-package+xml"/>'
                      "</rootfiles></container>")
        items = "".join(f'<item id="c{n}" href="c{n}.xhtml" media-type="application/xhtml+xml"/>'
                        for n in range(chapters))
        spine = "".join(f'<itemref idref="c{n}"/>' for n in range(chapters))
        epub.writestr("book.opf",
                      '<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" '
                      'version="3.0" unique-identifier="id"><metadata '
                      'xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="id">'
                      "synthetic-book</dc:identifier><dc:title>Synthetic</dc:title>"
                      f"<dc:language>en</dc:language></metadata><manifest>{items}</manifest>"
                      f"<spine>{spine}</spine></package>")
        for n in range(chapters):
            body = "".join("<p>" + " ".join(f"w{n}p{p:02}n{i:02}" for i in range(40)) + "</p>"
                           for p in range(paragraphs))
            epub.writestr(f"c{n}.xhtml", '<html xmlns="http://www.w3.org/1999/xhtml"><body>'
                          f"<h1>Chapter {n}</h1>{body}</body></html>")
