#!/usr/bin/env python3
"""Exercise the optional native worker with generated, non-personal PDFs."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile
from fixtures import reader, write_pdf


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


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--worker", type=Path, default=Path("target/release/bibli-pdf-worker"))
    parser.add_argument("--library", type=Path)
    parser.add_argument("--engine", choices=["pdfium", "poppler"], default="pdfium")
    args = parser.parse_args()
    environment = dict(os.environ)
    if args.library:
        environment["BIBLI_PDFIUM_LIBRARY"] = str(args.library.resolve())
    with tempfile.TemporaryDirectory(prefix="bibli-reader-check-") as directory:
        root = Path(directory)
        pdf = root / "Synthetic reader's document.pdf"
        for angle in (0, 90, 180, 270):
            reader(pdf, crop=True, angle=angle, named=angle == 180)
            command = [str(args.worker.resolve()), str(pdf), str(root)]
            with subprocess.Popen(command, env=environment, stdin=subprocess.PIPE,
                                  stdout=subprocess.PIPE, text=True) as process:
                info = reply(process, 0)["Ready"]
                assert info["pages"] == 3
                assert [entry["page"] for entry in info["outlines"]] == [1, 2]
                hits = send(process, {"Search": {"query": "needle"}})["Matches"]
                assert [hit["page"] for hit in hits] == [1, 2, 3]
                assert [hit["page"] for hit in send(process, {"Search": {"query": "NEEDLE"}})["Matches"]] == [1, 2, 3]
                assert send(process, {"Search": {"query": ""}}) == {"Matches": []}
                assert send(process, {"Search": {"query": "absent"}}) == {"Matches": []}
                for number in [1, 2, 3, 1]:
                    rendered = send(process, {"Render": {"page": number, "side": 600}})["Page"]
                    page = rendered["content"]
                    width, height = rendered["width"], rendered["height"]
                    assert max(width, height) == 600
                    assert (width > height) == (number == 2 and angle in (90, 270)), "intrinsic rotation"
                    assert page["page"] == number
                    assert "needle alpha beta" in "".join(char["text"] for char in page["chars"])
                    pixels = (root / "page.rgba").read_bytes()
                    assert len(pixels) == width * height * 4
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
            assert reply(process, 0) == {"Ready": {"pages": 1, "outlines": []}}
            for side, expected in [(0, 64), (10000, 2048)]:
                page = send(process, {"Render": {"page": 100, "side": side}})["Page"]
                assert max(page["width"], page["height"]) == expected
                assert page["content"]["page"] == 1
                assert page["content"]["chars"] == page["content"]["links"] == []
            assert send(process, {"Search": {"query": "needle"}}) == {"Matches": []}
            process.stdin.close()
            assert process.wait(timeout=5) == 0
        if args.engine == "pdfium":
            missing = dict(environment, BIBLI_PDFIUM_LIBRARY=str(root / "missing-library"))
            assert "Unavailable" in json.loads(subprocess.check_output(command, env=missing, text=True))["body"]
        pdf.write_bytes(b"not a PDF")
        assert "Error" in json.loads(subprocess.check_output(command, env=environment, text=True))["body"]
    print(f"PASS ({args.engine}): pages, rotation/crop, text geometry, links, outlines, search, limits, corrupt PDF")


if __name__ == "__main__":
    main()
