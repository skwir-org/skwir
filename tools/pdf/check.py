#!/usr/bin/env python3
"""Exercise the optional native worker with generated, non-personal PDFs."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile
from fixtures import book, reader, write_pdf


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


def has_ink(pixels, width, height, bounds):
    x0, y0 = max(0, int(bounds["x"] * width)), max(0, int(bounds["y"] * height))
    x1 = min(width, int((bounds["x"] + bounds["width"]) * width) + 1)
    y1 = min(height, int((bounds["y"] + bounds["height"]) * height) + 1)
    return any(min(pixels[(y * width + x) * 4:(y * width + x) * 4 + 3]) < 128
               for y in range(y0, y1) for x in range(x0, x1))


def page_text(process, number):
    page = send(process, {"Render": {"page": number, "side": 600}})["Page"]["content"]
    return " ".join("".join(char["text"] for char in page["chars"]).split())


def check_book(worker, root, environment):
    """A reflowable book is laid out at the size asked for, and a page's
    words are found again after it is laid out at another text size."""
    epub = root / "Synthetic book.epub"
    book(epub)

    def start(em):
        command = [str(worker), str(epub), str(root), f"--layout=420,595,{em}"]
        return subprocess.Popen(command, env=environment, stdin=subprocess.PIPE,
                                stdout=subprocess.PIPE, text=True)

    with start(9) as small:
        info = reply(small, 0)["Ready"]
        assert info["reflowable"] and info["pages"] > 3, info
        pages = info["pages"]
        read = pages // 2 + 1
        anchor = " ".join(page_text(small, read).split()[:6])
        small.stdin.close()
        assert small.wait(timeout=5) == 0
    with start(18) as large:
        info = reply(large, 0)["Ready"]
        assert info["reflowable"] and info["pages"] > pages, (info, pages)
        near = int((read - 1) / pages * info["pages"]) + 1
        found = send(large, {"Locate": {"text": anchor, "near": near}})["Located"]
        assert found is not None and anchor in page_text(large, found), (anchor, found)
        # From a worse estimate the same page is found, nearest first.
        for off in (max(1, found - 3), min(info["pages"], found + 3)):
            assert send(large, {"Locate": {"text": anchor, "near": off}}) == {"Located": found}
        assert send(large, {"Locate": {"text": "absent words", "near": near}}) == {"Located": None}
        assert send(large, {"Locate": {"text": "", "near": 1}}) == {"Located": None}
        large.stdin.close()
        assert large.wait(timeout=5) == 0
    for bad in ["--layout=1,595,12", "--layout=420,595", "--layout=420,595,200", "--other"]:
        command = [str(worker), str(epub), str(root), bad]
        assert "Error" in json.loads(subprocess.check_output(command, env=environment, text=True))["body"]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--worker", type=Path, default=Path("target/release/skwir-pdf-worker"))
    parser.add_argument("--library", type=Path)
    parser.add_argument("--engine", choices=["pdfium", "poppler", "mupdf"], default="pdfium")
    args = parser.parse_args()
    environment = dict(os.environ)
    if args.library:
        environment["SKWIR_PDFIUM_LIBRARY"] = str(args.library.resolve())
    with tempfile.TemporaryDirectory(prefix="skwir-reader-check-") as directory:
        root = Path(directory)
        pdf = root / "Synthetic reader's document.pdf"
        for angle in (0, 90, 180, 270):
            labelled = angle == 270
            reader(pdf, crop=True, angle=angle, named=angle == 180, labelled=labelled)
            command = [str(args.worker.resolve()), str(pdf), str(root)]
            with subprocess.Popen(command, env=environment, stdin=subprocess.PIPE,
                                  stdout=subprocess.PIPE, text=True) as process:
                info = reply(process, 0)["Ready"]
                assert info["pages"] == 3
                # The outline's entries in order, each at its depth.
                assert [(entry["title"], entry["depth"], entry["page"]) for entry in info["outlines"]] == [
                    ("Introduction", 0, 1), ("First section", 1, 1), ("First paragraph", 2, 2),
                    ("Second chapter", 0, 2), ("Notes", 1, None)], info["outlines"]
                # Pages named otherwise than by their number carry their names.
                assert info["labels"] == (["i", "1", "2"] if labelled else []), info["labels"]
                hits = send(process, {"Search": {"query": "needle"}})["Matches"]
                assert [hit["page"] for hit in hits] == [1, 2, 3]
                process.stdin.close()
                assert process.wait(timeout=5) == 0
            # For a thumbnail, its first page alone: its pages counted, no
            # outline nor labels read, the page drawn all the same.
            with subprocess.Popen(command + ["--first-page"], env=environment,
                                  stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                  text=True) as process:
                info = reply(process, 0)["Ready"]
                assert (info["pages"], info["outlines"], info["labels"]) == (3, [], []), info
                first = send(process, {"Render": {"page": 1, "side": 300}})["Page"]
                assert max(first["width"], first["height"]) == 300
                process.stdin.close()
                assert process.wait(timeout=5) == 0
            with subprocess.Popen(command, env=environment, stdin=subprocess.PIPE,
                                  stdout=subprocess.PIPE, text=True) as process:
                assert reply(process, 0)["Ready"]["pages"] == 3
                assert [hit["page"] for hit in send(process, {"Search": {"query": "NEEDLE"}})["Matches"]] == [1, 2, 3]
                assert send(process, {"Search": {"query": ""}}) == {"Matches": []}
                assert send(process, {"Search": {"query": "absent"}}) == {"Matches": []}
                for number in [1, 2, 3, 1]:
                    # A page's own size does not depend on the side rendered.
                    small = send(process, {"Render": {"page": number, "side": 300}})["Page"]["content"]
                    rendered = send(process, {"Render": {"page": number, "side": 600}})["Page"]
                    page = rendered["content"]
                    assert abs(small["width"] - page["width"]) < 0.5 and abs(small["height"] - page["height"]) < 0.5
                    assert (page["width"] > page["height"]) == (rendered["width"] > rendered["height"])
                    width, height = rendered["width"], rendered["height"]
                    assert max(width, height) == 600
                    assert (width > height) == (number == 2 and angle in (90, 270)), "intrinsic rotation"
                    assert page["page"] == number
                    assert "needle alpha beta" in "".join(char["text"] for char in page["chars"])
                    pixels = (root / "page.rgba").read_bytes()
                    assert len(pixels) == width * height * 4
                    # Every engine draws in the window's order.
                    assert rendered.get("order") == "Bgra", rendered.get("order")
                    for char in page["chars"]:
                        if char["text"].strip():
                            assert has_ink(pixels, width, height, char["bounds"]), char
                    for bounds in hits[number - 1]["bounds"]:
                        assert has_ink(pixels, width, height, bounds), "search highlight misses text"
                    assert sorted(json.dumps(link["destination"]) for link in page["links"]) == sorted(
                        json.dumps(dest) for dest in [{"Uri": "https://example.org/"}, {"Page": 2}])
                    for link in page["links"]:
                        assert has_ink(pixels, width, height, link["bounds"]), "link misses text"
                assert "Error" in send(process, {"Search": {"query": "x" * 5000}})
                assert send(process, "Cancel") == {"Error": "cancelled"}
                assert send(process, {"Search": {"query": "absent"}}) == {"Matches": []}
                # A PDF's pages never move: nothing to locate.
                assert send(process, {"Locate": {"text": "needle", "near": 1}}) == {"Located": None}
                process.stdin.close()
                assert process.wait(timeout=5) == 0
        # Empty pages and absent outlines exercise nullable native allocations.
        write_pdf(pdf, [
            b"<< /Type /Catalog /Pages 2 0 R >>",
            b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>",
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 600 800] >>",
        ])
        with subprocess.Popen(command, env=environment, stdin=subprocess.PIPE,
                              stdout=subprocess.PIPE, text=True) as process:
            assert reply(process, 0) == {"Ready": {"pages": 1, "outlines": [], "reflowable": False, "labels": []}}
            for side, expected in [(0, 64), (10000, 4096)]:
                page = send(process, {"Render": {"page": 100, "side": side}})["Page"]
                assert max(page["width"], page["height"]) == expected
                assert page["content"]["page"] == 1
                assert page["content"]["chars"] == page["content"]["links"] == []
                # 600 x 800 points is 800 x 1066.7 CSS pixels, whatever the side.
                assert abs(page["content"]["width"] - 800) < 1 and abs(page["content"]["height"] - 1066.67) < 1
            assert send(process, {"Search": {"query": "needle"}}) == {"Matches": []}
            process.stdin.close()
            assert process.wait(timeout=5) == 0
        if args.engine == "pdfium":
            missing = dict(environment, SKWIR_PDFIUM_LIBRARY=str(root / "missing-library"))
            assert "Unavailable" in json.loads(subprocess.check_output(command, env=missing, text=True))["body"]
        if args.engine == "mupdf":
            check_book(args.worker.resolve(), root, environment)
        pdf.write_bytes(b"not a PDF")
        assert "Error" in json.loads(subprocess.check_output(command, env=environment, text=True))["body"]
    extra = ", book layout and locate" if args.engine == "mupdf" else ""
    print(f"PASS ({args.engine}): pages, first page alone, rotation/crop, text geometry, links, outlines, search, limits, corrupt PDF{extra}")


if __name__ == "__main__":
    main()
