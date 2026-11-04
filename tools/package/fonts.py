#!/usr/bin/env python3
"""Skwir's own fonts as its packages carry them (ADR-118): the archives
pinned in tools/package/fonts.lock, checked against their sums, and the
static files skwir draws with taken from them into <out>:

    <out>/fonts/          Inter (regular, italic, medium, semibold, bold,
                          bold italic) and JetBrains Mono (regular, italic,
                          bold, bold italic), as .ttf
    <out>/licenses/fonts/ each family's licence (SIL Open Font License)

A package puts <out>/fonts in skwir's data folder, `share/skwir/fonts`
(beside the program on Windows, above it on Linux, in a Mac's
Resources), where skwir finds them (`skwir_platform::fonts::bundled`).
The archives are kept in <out>/.downloads and fetched again only when
missing or no longer matching their sums: a build that may not fetch (the
Nix package's) puts them there first.

    tools/package/fonts.py <out>
"""

import hashlib
import pathlib
import sys
import urllib.request
import zipfile

LOCK = pathlib.Path(__file__).resolve().parent / "fonts.lock"

# What each archive gives: its member, and where it goes under <out>.
TAKEN = {
    "Inter-4.1.zip": [
        *(f"extras/ttf/Inter-{style}.ttf" for style in
          ("Regular", "Italic", "Medium", "SemiBold", "Bold", "BoldItalic")),
        ("LICENSE.txt", "licenses/fonts/Inter/LICENSE.txt"),
    ],
    "JetBrainsMono-2.304.zip": [
        *(f"fonts/ttf/JetBrainsMono-{style}.ttf" for style in
          ("Regular", "Italic", "Bold", "BoldItalic")),
        ("OFL.txt", "licenses/fonts/JetBrainsMono/OFL.txt"),
    ],
}


def fail(message):
    print(f"fonts.py: {message}", file=sys.stderr)
    sys.exit(1)


def pinned():
    """The archives fonts.lock names: (name, url, sum)."""
    for line in LOCK.read_text().splitlines():
        if line.strip() and not line.startswith("#"):
            name, url, sum_ = line.split()
            yield name, url, sum_


def sha256(path):
    digest = hashlib.sha256()
    with open(path, "rb") as file:
        for block in iter(lambda: file.read(1 << 20), b""):
            digest.update(block)
    return digest.hexdigest()


def fetched(downloads, name, url, sum_):
    """The archive `name`, kept or fetched, matching its sum."""
    path = downloads / name
    if path.is_file() and sha256(path) == sum_:
        return path
    partial = path.with_suffix(".part")
    with urllib.request.urlopen(url, timeout=120) as answer, open(partial, "wb") as file:
        while block := answer.read(1 << 20):
            file.write(block)
    if sha256(partial) != sum_:
        partial.unlink()
        fail(f"{name}: its sum is not the one pinned")
    partial.replace(path)
    return path


def main():
    if len(sys.argv) != 2:
        print(f"usage: {sys.argv[0]} <out>", file=sys.stderr)
        sys.exit(2)
    out = pathlib.Path(sys.argv[1])
    downloads = out / ".downloads"
    downloads.mkdir(parents=True, exist_ok=True)
    (out / "fonts").mkdir(exist_ok=True)
    for name, url, sum_ in pinned():
        if name not in TAKEN:
            fail(f"{name}: fonts.lock names an archive fonts.py takes nothing from")
        with zipfile.ZipFile(fetched(downloads, name, url, sum_)) as archive:
            for member in TAKEN[name]:
                source, dest = member if isinstance(member, tuple) else (
                    member, f"fonts/{pathlib.PurePosixPath(member).name}")
                target = out / dest
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_bytes(archive.read(source))


if __name__ == "__main__":
    main()
