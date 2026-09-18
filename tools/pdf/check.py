#!/usr/bin/env python3
"""Exercise the optional native worker with generated, non-personal PDFs."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile
from fixtures import reader


def send(process, request):
    process.stdin.write(json.dumps(request) + "\n")
    process.stdin.flush()
    return json.loads(process.stdout.readline())


def has_ink(pixels, width, height, bounds):
    x0, y0 = max(0, int(bounds["x"] * width)), max(0, int(bounds["y"] * height))
    x1 = min(width, int((bounds["x"] + bounds["width"]) * width) + 1)
    y1 = min(height, int((bounds["y"] + bounds["height"]) * height) + 1)
    return any(min(pixels[(y * width + x) * 4:(y * width + x) * 4 + 3]) < 128
               for y in range(y0, y1) for x in range(x0, x1))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--worker", type=Path, default=Path("target/release/bibli-pdf-worker"))
    parser.add_argument("--library", type=Path, required=True)
    args = parser.parse_args()
    environment = dict(os.environ, BIBLI_PDFIUM_LIBRARY=str(args.library.resolve()))
    with tempfile.TemporaryDirectory(prefix="bibli-reader-check-") as directory:
        root = Path(directory)
        pdf = root / "Synthetic reader's document.pdf"
        reader(pdf, crop=True)
        command = [str(args.worker.resolve()), str(pdf), str(root)]
        with subprocess.Popen(command, env=environment, stdin=subprocess.PIPE,
                              stdout=subprocess.PIPE, text=True) as process:
            info = json.loads(process.stdout.readline())["Ready"]
            assert info["pages"] == 3
            assert [entry["page"] for entry in info["outlines"]] == [1, 2]
            hits = send(process, {"Search": {"query": "needle"}})["Matches"]
            assert [hit["page"] for hit in hits] == [1, 2, 3]
            assert send(process, {"Search": {"query": ""}}) == {"Matches": []}
            assert send(process, {"Search": {"query": "absent"}}) == {"Matches": []}
            for number in [1, 2, 3, 1]:
                rendered = send(process, {"Render": {"page": number, "side": 600}})["Page"]
                page = rendered["content"]
                width, height = rendered["width"], rendered["height"]
                assert max(width, height) == 600
                assert (width > height) == (number == 2), "intrinsic rotation"
                assert page["page"] == number
                assert "needle alpha beta" in "".join(char["text"] for char in page["chars"])
                pixels = (root / "page.rgba").read_bytes()
                assert len(pixels) == width * height * 4
                for char in page["chars"]:
                    if char["text"].strip():
                        assert has_ink(pixels, width, height, char["bounds"]), char
                for bounds in hits[number - 1]["bounds"]:
                    assert has_ink(pixels, width, height, bounds), "search highlight misses text"
                if number == 1:
                    assert [link["destination"] for link in page["links"]] == [
                        {"Uri": "https://example.org/"}, {"Page": 2}]
            assert "Error" in send(process, {"Search": {"query": "x" * 5000}})
            process.stdin.close()
            assert process.wait(timeout=5) == 0
        missing = dict(environment, BIBLI_PDFIUM_LIBRARY=str(root / "missing-library"))
        assert "Unavailable" in json.loads(subprocess.check_output(command, env=missing, text=True))
        pdf.write_bytes(b"not a PDF")
        assert "Error" in json.loads(subprocess.check_output(command, env=environment, text=True))
    print("PASS: pages, rotation/crop, text geometry, links, outlines, search, limits, missing library, corrupt PDF")


if __name__ == "__main__":
    main()
