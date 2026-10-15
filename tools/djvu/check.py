#!/usr/bin/env python3
"""Exercise a DjVu worker (DjVuLibre's or djvu-rs's) with generated,
non-personal documents, made by DjVuLibre's own tools (cjb2, c44, djvused,
djvm, djvmcvt): the same assertions for both engines (ADR-061)."""
import argparse
import json
from pathlib import Path
import shutil
import subprocess
import tempfile

# A page as stored, in pixels at 200 dpi (US letter).
WIDTH, HEIGHT, DPI = 1700, 2200, 200
# The words of each page, in its pixels from its top left: what is drawn
# (a black box each) and what the text layer says is there.
WORDS = {
    1: [("Alpha", (200, 300, 500, 380)), ("Beta", (560, 300, 800, 380)),
        ("Gamma", (200, 500, 600, 580))],
    2: [("Delta", (300, 400, 700, 480)), ("beta", (760, 400, 1000, 480))],
    # Stored upright, shown turned a quarter counterclockwise.
    3: [("Epsilon", (150, 200, 650, 280))],
}
# Page 1's links: to page 3, and out.
LINKS = [("#3", (200, 300, 500, 380)), ("https://example.org/", (200, 500, 600, 580))]


def reply(process, expected):
    """The next reply, which must answer request `expected` (0 is the greeting)."""
    line = json.loads(process.stdout.readline())
    assert line["id"] == expected, line
    return line["body"]


def send(process, request):
    """Requests are numbered from 1 and answered once each, in order."""
    process.sent = getattr(process, "sent", 0) + 1
    process.stdin.write(json.dumps({"id": process.sent, "body": request}) + "\n")
    process.stdin.flush()
    return reply(process, process.sent)


def bitmap(path, boxes, colour=False):
    """A page with a black box at each of `boxes`: a PBM, or a PPM over a
    gradient for a colour page. Rows are made once for each set of boxes
    they cross (and shade)."""
    made = {}

    def row(y):
        crossed = tuple(box for box in boxes if box[1] <= y < box[3])
        # Light enough never to be taken for ink.
        shade = 170 + y * 80 // HEIGHT if colour else 0
        key = (crossed, shade)
        if key not in made:
            dark = bytearray(WIDTH)
            for (l, _, r, _) in crossed:
                dark[l:r] = b"\1" * (r - l)
            if colour:
                background = bytes((shade, 210, 255 - shade // 4))
                made[key] = b"".join(b"\0\0\0" if d else background for d in dark)
            else:
                pad = (8 - WIDTH % 8) % 8
                bits = "".join("1" if d else "0" for d in dark) + "0" * pad
                made[key] = int(bits, 2).to_bytes(len(bits) // 8, "big")
        return made[key]

    header = b"P6\n%d %d\n255\n" if colour else b"P4\n%d %d\n"
    path.write_bytes(header % (WIDTH, HEIGHT) + b"".join(row(y) for y in range(HEIGHT)))


def djvused(document, script):
    subprocess.run(["djvused", str(document), "-s", "-e", script], check=True)


def text_layer(words):
    """The hidden text of a page: one line, its words, from the bottom left."""
    zones = " ".join(f'(word {l} {HEIGHT - b} {r} {HEIGHT - t} "{text}")'
                     for text, (l, t, r, b) in words)
    return f"(page 0 0 {WIDTH} {HEIGHT} (line 0 0 {WIDTH} {HEIGHT} {zones}))"


def make(root):
    """A bundled document of three pages, one of them in colour and one
    turned; and the same as an indirect document (pages in files of their
    own), which a worker reads no further than its index."""
    pages = []
    for number, words in WORDS.items():
        page = root / f"page{number}.djvu"
        boxes = [box for _, box in words]
        if number == 2:
            picture = root / "page.ppm"
            bitmap(picture, boxes, colour=True)
            subprocess.run(["c44", "-dpi", str(DPI), str(picture), str(page)], check=True)
        else:
            picture = root / "page.pbm"
            bitmap(picture, boxes)
            subprocess.run(["cjb2", "-dpi", str(DPI), str(picture), str(page)], check=True)
        hidden = root / "text.sexp"
        hidden.write_text(text_layer(words))
        script = f"select 1; set-txt {hidden}; save"
        if number == 1:
            areas = root / "links.sexp"
            areas.write_text("\n".join(
                f'(maparea "{url}" "" (rect {l} {HEIGHT - b} {r - l} {b - t}))'
                for url, (l, t, r, b) in LINKS))
            script = f"select 1; set-txt {hidden}; set-ant {areas}; save"
        if number == 3:
            script = f"select 1; set-txt {hidden}; set-rotation 1; save"
        djvused(page, script)
        pages.append(page)
    document = root / "Synthetic scanned book.djvu"
    subprocess.run(["djvm", "-c", str(document), *map(str, pages)], check=True)
    outline = root / "outline.sexp"
    outline.write_text('(bookmarks ("Start" "#1" ("Last page" "#3")) ("Second" "#page2.djvu"))')
    djvused(document, f"set-outline {outline}; select 2; set-page-title ii; save")
    indirect = root / "indirect"
    indirect.mkdir()
    subprocess.run(["djvmcvt", "-i", str(document), str(indirect), "index.djvu"], check=True)
    return document, indirect / "index.djvu"


def expected_text(number):
    """A page's words as the reader's text: a space between two on a line,
    a line break between lines."""
    text = ""
    for i, (word, (_, top, _, bottom)) in enumerate(WORDS[number]):
        if i:
            _, (_, before_top, _, before_bottom) = WORDS[number][i - 1]
            text += " " if top < before_bottom and before_top < bottom else "\n"
        text += word
    return text


def ink(pixels, width, height, bounds):
    """Whether any pixel inside `bounds` (fractions of the page) is dark."""
    x0, y0 = max(0, int(bounds["x"] * width)), max(0, int(bounds["y"] * height))
    x1 = min(width, int((bounds["x"] + bounds["width"]) * width) + 1)
    y1 = min(height, int((bounds["y"] + bounds["height"]) * height) + 1)
    return any(min(pixels[(y * width + x) * 4:(y * width + x) * 4 + 3]) < 128
               for y in range(y0, y1) for x in range(x0, x1))


def inked(pixels, width, height):
    """The box around a page's dark pixels: left, top, right, bottom."""
    xs, ys = [], []
    for y in range(height):
        row = pixels[y * width * 4:(y + 1) * width * 4]
        dark = [x for x in range(width) if min(row[x * 4:x * 4 + 3]) < 128]
        if dark:
            xs += [dark[0], dark[-1]]
            ys.append(y)
    return min(xs), min(ys), max(xs) + 1, max(ys) + 1


def expected_ink(number, width, height):
    """Where page `number`'s boxes are once drawn at `width` × `height`
    (turned as page 3 is)."""
    l = min(box[0] for _, box in WORDS[number])
    t = min(box[1] for _, box in WORDS[number])
    r = max(box[2] for _, box in WORDS[number])
    b = max(box[3] for _, box in WORDS[number])
    if number == 3:
        # A quarter turn counterclockwise: the stored top is the left.
        l, t, r, b = t, WIDTH - r, b, WIDTH - l
        return l * width / HEIGHT, t * height / WIDTH, r * width / HEIGHT, b * height / WIDTH
    return l * width / WIDTH, t * height / HEIGHT, r * width / WIDTH, b * height / HEIGHT


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--worker", type=Path, required=True)
    parser.add_argument("--engine", choices=["djvulibre", "djvu-rs"], required=True)
    parser.add_argument("--verbose", action="store_true",
                        help="say where each page's ink lands against where it should")
    args = parser.parse_args()
    for tool in ("cjb2", "c44", "djvused", "djvm", "djvmcvt"):
        assert shutil.which(tool), f"{tool} is missing: DjVuLibre's tools make the fixtures"
    worker = str(args.worker.resolve())
    worst = 0.0
    with tempfile.TemporaryDirectory(prefix="skwir-djvu-check-") as directory:
        root = Path(directory)
        document, indirect = make(root)
        scratch = root / "scratch"
        scratch.mkdir()
        command = [worker, str(document), str(scratch)]
        with subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                              text=True) as process:
            info = reply(process, 0)["Ready"]
            assert info["pages"] == 3 and not info["reflowable"], info
            assert [(o["title"], o["depth"], o["page"]) for o in info["outlines"]] == [
                ("Start", 0, 1), ("Last page", 1, 3), ("Second", 0, 2)], info["outlines"]
            # DjVuLibre reads what the document calls its pages; djvu-rs
            # does not say.
            labels = ["", "ii", ""] if args.engine == "djvulibre" else []
            assert info["labels"] == labels, info["labels"]
            hits = send(process, {"Search": {"query": "BETA"}})["Matches"]
            assert [hit["page"] for hit in hits] == [1, 2], hits
            assert send(process, {"Search": {"query": "alpha beta"}})["Matches"][0]["page"] == 1
            assert send(process, {"Search": {"query": "  "}}) == {"Matches": []}
            assert send(process, {"Search": {"query": "absent"}}) == {"Matches": []}
            assert "Error" in send(process, {"Search": {"query": "x" * 5000}})
            for number in (1, 2, 3, 1):
                for side in (300, 600, 1500):
                    rendered = send(process, {"Render": {"page": number, "side": side}})["Page"]
                    page = rendered["content"]
                    width, height = rendered["width"], rendered["height"]
                    # At most the side asked: djvu-rs draws at the size it
                    # draws exactly below it (`exact_size`).
                    assert side * 0.93 <= max(width, height) <= side, (number, side, width, height)
                    # Turned, the third page is wider than high.
                    assert (width > height) == (number == 3), (number, width, height)
                    # Its own size is what 100% shows: 8.5 × 11 in.
                    across, down = (1056, 816) if number == 3 else (816, 1056)
                    # Whatever the size drawn, the page's own sides are kept.
                    assert abs(width / height - across / down) < 0.01, (width, height)
                    assert abs(page["width"] - across) < 1 and abs(page["height"] - down) < 1, page
                    pixels = (scratch / "page.rgba").read_bytes()
                    assert len(pixels) == width * height * 4
                    assert rendered["order"] == ("Bgra" if args.engine == "djvulibre" else "Rgba")
                    if rendered["order"] == "Bgra":
                        pixels = bytes(b for i in range(0, len(pixels), 4)
                                       for b in (pixels[i + 2], pixels[i + 1], pixels[i], pixels[i + 3]))
                    assert all(pixels[i] == 255 for i in range(3, len(pixels), 4)), "opaque"
                    text = "".join(char["text"] for char in page["chars"])
                    assert text == expected_text(number), text
                    for char in page["chars"]:
                        if char["text"].strip():
                            assert ink(pixels, width, height, char["bounds"]), (number, char)
                    # The ink lies where the page has it, within a hundredth
                    # of the page.
                    got = inked(pixels, width, height)
                    want = expected_ink(number, width, height)
                    off = max(abs(g - w) / max(width, height) for g, w in zip(got, want))
                    worst = max(worst, off)
                    if args.verbose:
                        print(f"page {number} at {side}: {width}×{height}, ink {got}, "
                              f"expected {tuple(round(w, 1) for w in want)}, off {off * 100:.2f}%")
                    assert off < 0.004, (number, side, got, want)
                    links = sorted((json.dumps(link["destination"]), link["bounds"]["x"] < 0.5)
                                   for link in page["links"])
                    if number == 1:
                        assert links == sorted([(json.dumps({"Page": 3}), True),
                                                (json.dumps({"Uri": "https://example.org/"}), True)]), links
                        for link in page["links"]:
                            assert ink(pixels, width, height, link["bounds"]), "link misses its words"
                    else:
                        assert links == [], links
            for side, expected in [(0, 64), (10000, 4096)]:
                rendered = send(process, {"Render": {"page": 100, "side": side}})["Page"]
                assert expected * 0.93 <= max(rendered["width"], rendered["height"]) <= expected
                assert rendered["content"]["page"] == 3
            assert send(process, "Cancel") == {"Error": "cancelled"}
            # A DjVu page never moves: nothing to locate.
            assert send(process, {"Locate": {"text": "Alpha", "near": 1}}) == {"Located": None}
            process.stdin.close()
            assert process.wait(timeout=5) == 0
        # For a thumbnail, its first page alone: its pages counted, no
        # outline nor labels read, the page drawn all the same.
        with subprocess.Popen(command + ["--first-page"], stdin=subprocess.PIPE,
                              stdout=subprocess.PIPE, text=True) as process:
            info = reply(process, 0)["Ready"]
            assert (info["pages"], info["outlines"], info["labels"]) == (3, [], []), info
            first = send(process, {"Render": {"page": 1, "side": 256}})["Page"]
            assert 256 * 0.93 <= max(first["width"], first["height"]) <= 256
            process.stdin.close()
            assert process.wait(timeout=5) == 0
        # An indirect document's pages are files of their own, which an
        # isolated worker cannot read: it says so, on the page asked.
        with subprocess.Popen([worker, str(indirect), str(scratch)], stdin=subprocess.PIPE,
                              stdout=subprocess.PIPE, text=True) as process:
            greeting = reply(process, 0)
            if "Ready" in greeting:
                (indirect.parent / "page1.djvu").unlink()
                assert "Error" in send(process, {"Render": {"page": 1, "side": 300}})
            else:
                assert "Error" in greeting, greeting
            process.stdin.close()
            process.wait(timeout=5)
        for broken in (b"not a DjVu document", document.read_bytes()[:200]):
            document.write_bytes(broken)
            with subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                  text=True) as process:
                greeting = reply(process, 0)
                if "Ready" in greeting:
                    assert "Error" in send(process, {"Render": {"page": 1, "side": 300}}), greeting
                else:
                    assert "Error" in greeting, greeting
                process.stdin.close()
                process.wait(timeout=5)
    print(f"PASS ({args.engine}): pages, colour, rotation, text geometry, links, outline, "
          f"labels, search, limits, first page alone, indirect and corrupt documents; "
          f"ink at most {worst * 100:.2f}% of the page from where it is")


if __name__ == "__main__":
    main()
