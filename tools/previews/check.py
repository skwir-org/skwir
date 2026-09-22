#!/usr/bin/env python3
"""Generate synthetic previews and exercise the optional archive/audio helpers.

Usage: python3 tools/previews/check.py --root /tmp/bibli-preview-fixtures
No personal media, mail or documents are used. Generated files stay outside git.
"""
import argparse
import importlib.util
import io
import json
import os
from pathlib import Path
import shutil
import struct
import subprocess
import tarfile
import tempfile
import time
import zipfile
import zlib


def png(width=640, height=360):
    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))
    rows = b"".join(b"\0" + bytes([40, 110 + y % 100, 180]) * width for y in range(height))
    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0))
            + chunk(b"IDAT", zlib.compress(rows)) + chunk(b"IEND", b""))


def generate(root):
    root.mkdir(parents=True, exist_ok=True)
    (root / "cover.png").write_bytes(png())
    (root / "01 Reading.md").write_text("""# Native reading mode

A paragraph with **bold**, *italic*, and a [web link](https://example.invalid).

## Lists and images

- [x] A completed task
- [ ] Another task

![Synthetic cover](cover.png)

## Code

```rust
fn main() {
    println!("Synthetic preview");
}
```

| Name | Value |
| --- | --- |
| Example | 42 |
""")
    (root / "02 Table.csv").write_text('Name,Value,Notes\nFirst,42,"A quoted, field"\nSecond,73,"Two\nlines"\n')
    (root / "03 Data.json").write_text(json.dumps({"project": "Synthetic", "items": [{"name": "First", "size": 42}, {"name": "Second", "size": 73}]}, indent=2))
    (root / "04 Bytes.bin").write_bytes(bytes(range(256)) * 40)
    (root / "05 Message.eml").write_text("From: Example <sender@example.invalid>\nTo: Reader <reader@example.invalid>\nSubject: Synthetic preview\nMIME-Version: 1.0\nContent-Type: text/html; charset=UTF-8\n\n<h1>A sample message</h1><p>This is <strong>formatted email</strong>.</p><img src='https://example.invalid/never-fetched.png'>")
    with zipfile.ZipFile(root / "06 Archive's.zip", "w", compression=zipfile.ZIP_DEFLATED) as archive:
        archive.writestr("folder/example.txt", "Synthetic archive member")
        archive.writestr("folder/cover.png", png())
        archive.writestr("folder.ext", "A sibling must not interrupt folder children")
        archive.writestr("../outside.txt", "A name, never an extraction path")
    with zipfile.ZipFile(root / "07 Comic.cbz", "w", compression=zipfile.ZIP_DEFLATED) as archive:
        for number in [10, 2, 1]:
            archive.writestr(f"page{number}.png", png(400, 600))
    with zipfile.ZipFile(root / "08 Book.epub", "w", compression=zipfile.ZIP_DEFLATED) as archive:
        archive.writestr("mimetype", "application/epub+zip")
        archive.writestr("META-INF/container.xml", '<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="EPUB/book.opf" media-type="application/oebps-package+xml"/></rootfiles></container>')
        archive.writestr("EPUB/book.opf", '<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="id">urn:example:synthetic</dc:identifier><dc:title>Synthetic Book</dc:title><dc:language>en</dc:language></metadata><manifest><item id="one" href="one.xhtml" media-type="application/xhtml+xml"/><item id="two" href="two.xhtml" media-type="application/xhtml+xml"/><item id="cover" href="cover.png" media-type="image/png"/></manifest><spine><itemref idref="one"/><itemref idref="two"/></spine></package>')
        archive.writestr("EPUB/one.xhtml", '<html xmlns="http://www.w3.org/1999/xhtml"><body><h1>First chapter</h1><p>Reflowing native text.</p><img src="cover.png"/><a href="two.xhtml">Second chapter</a></body></html>')
        archive.writestr("EPUB/two.xhtml", '<html xmlns="http://www.w3.org/1999/xhtml"><body><h1>Second chapter</h1><p>More synthetic text.</p></body></html>')
        archive.writestr("EPUB/cover.png", png())
    if shutil.which("fc-match"):
        font = subprocess.check_output(["fc-match", "-f", "%{file}", "sans"], text=True)
        shutil.copyfile(font, root / "09 Font.ttf")
    subprocess.run(["ffmpeg", "-y", "-nostdin", "-v", "error", "-f", "lavfi", "-i", "sine=frequency=440:sample_rate=48000", "-i", str(root / "cover.png"), "-map", "0:a", "-map", "1:v", "-t", "8", "-c:a", "flac", "-c:v", "png", "-disposition:v", "attached_pic", "-metadata", "title=Synthetic tone", "-metadata", "artist=Example performer", str(root / "10 Audio.flac")], check=True)


# The helper takes every bound from its caller (bibli-platform's shared limits);
# these mirror bibli_platform::inspection::limits for the direct checks below.
MEMBER_BYTES = 16 * 1024 * 1024
LISTING = ["--members", "4000", "--names", str(256 * 1024)]


def check_archive(executable, root):
    path = root / "06 Archive's.zip"
    entries = json.loads(subprocess.check_output([executable, str(path), *LISTING]))
    assert len(entries) == 4
    with tempfile.TemporaryDirectory() as tmp:
        output = Path(tmp) / "member"
        target = next(e for e in entries if e["name"] == "../outside.txt")
        read = [executable, str(path), str(target["index"]), str(output), "--name", target["name"], "--bytes", str(MEMBER_BYTES)]
        subprocess.run(read, check=True, stdout=subprocess.DEVNULL)
        assert output.read_text() == "A name, never an extraction path"
        assert not (Path(tmp).parent / "outside.txt").exists()
        read[read.index("--name") + 1] = "folder/example.txt"
        result = subprocess.run(read, capture_output=True)
        assert result.returncode != 0 and b"changed" in result.stderr, "an index names the member it was listed with"
        bomb = Path(tmp) / "large.zip"
        with zipfile.ZipFile(bomb, "w", compression=zipfile.ZIP_DEFLATED) as archive:
            archive.writestr("large", bytes(17 * 1024 * 1024))
        result = subprocess.run([executable, str(bomb), "0", str(output), "--name", "large", "--bytes", str(MEMBER_BYTES)], capture_output=True)
        assert result.returncode != 0 and b"preview limit" in result.stderr
        result = subprocess.run([executable, str(bomb), "0", str(output), "--name", "large"], capture_output=True)
        assert result.returncode != 0 and b"--bytes" in result.stderr, "a member read without a bound is refused"
        result = subprocess.run([executable, str(path), "--members", "2", "--names", "4096"], capture_output=True)
        assert result.returncode != 0 and b"2 members" in result.stderr, "listing bounds come from the caller"
        accented = Path(tmp) / "accents.zip"
        with zipfile.ZipFile(accented, "w") as archive:
            archive.writestr("Cr\u00e8me/r\u00e9sum\u00e9.txt", "caf\u00e9")
        clean = {"PATH": os.environ.get("PATH", "")}
        names = json.loads(subprocess.check_output([executable, str(accented), *LISTING], env=clean))
        assert [e["name"] for e in names] == ["Cr\u00e8me/r\u00e9sum\u00e9.txt"], "names convert in any locale"
        tar = Path(tmp) / "links.tar"
        with tarfile.open(tar, "w") as archive:
            entry = tarfile.TarInfo("link")
            entry.type = tarfile.SYMTYPE
            entry.linkname = "../outside"
            archive.addfile(entry)
        result = subprocess.run([executable, str(tar), "0", str(output), "--name", "link", "--bytes", str(MEMBER_BYTES)], capture_output=True)
        assert result.returncode != 0 and b"regular" in result.stderr
    print("Archive: ZIP listing, selected bytes, traversal names, caller-provided bounds and symlink rejection passed")


def check_audio(executable, root):
    spec = importlib.util.spec_from_file_location("media_check", Path(__file__).resolve().parents[1] / "media" / "check.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    os.environ["BIBLI_MEDIA_AUDIO_ONLY"] = "1"
    worker = module.Worker(executable, root / "10 Audio.flac")
    try:
        worker.until(lambda: worker.state.get("duration") is not None)
        assert worker.state["status"] == "Paused"
        worker.control({"Play": True})
        worker.until(lambda: worker.state.get("position", 0) > .3 and len(worker.state.get("spectrum", [])) == 32)
        assert worker.state["title"] == "Synthetic tone"
        assert max(worker.state["spectrum"]) > .1
        worker.control({"Seek": 3.0})
        worker.until(lambda: worker.state.get("position", 0) >= 3.0)
        worker.control({"Play": False})
        worker.until(lambda: worker.state.get("status") == "Paused")
        time.sleep(.15)
        start = module.ticks(worker.process.pid)
        time.sleep(1)
        cpu = (module.ticks(worker.process.pid) - start) / os.sysconf("SC_CLK_TCK") * 100
        print(f"Audio: paused startup, tags, spectrum, playback, seeking and pause passed; paused CPU {cpu:.1f}%; worker PSS {module.pss(worker.process.pid):.1f} MiB")
    finally:
        worker.close()
        os.environ.pop("BIBLI_MEDIA_AUDIO_ONLY", None)


def check_asf(executable, root):
    spec = importlib.util.spec_from_file_location("media_check", Path(__file__).resolve().parents[1] / "media" / "check.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    # These files deliberately have the same extension/container MIME. Let the
    # worker discover their streams instead of supplying the audio-only hint.
    hint = os.environ.pop("BIBLI_MEDIA_AUDIO_ONLY", None)
    try:
        for audio_only in [False, True]:
            path = root / ("Synthetic audio.asf" if audio_only else "Synthetic video.asf")
            command = ["ffmpeg", "-nostdin", "-v", "error", "-y"]
            if not audio_only:
                command += ["-f", "lavfi", "-i", "testsrc2=size=160x90:rate=10"]
            command += ["-f", "lavfi", "-i", "sine=frequency=440:sample_rate=44100", "-t", "6", "-c:a", "wmav2"]
            if not audio_only:
                command += ["-c:v", "wmv2", "-threads", "1"]
            subprocess.run(command + [str(path)], check=True)
            worker = module.Worker(executable, path)
            try:
                worker.until(lambda: worker.state.get("duration") is not None)
                assert worker.state["audio_only"] == audio_only
                assert worker.state["audio_tracks"]
                worker.control({"Play": True})
                worker.until(lambda: worker.state.get("position", 0) > .3 and (len(worker.state.get("spectrum", [])) == 32 if audio_only else worker.frames > 0))
                worker.control({"Seek": 3.0})
                worker.until(lambda: worker.state.get("position", 0) >= 2.8)
                worker.control({"Play": False})
                worker.until(lambda: worker.state.get("status") == "Paused")
                if audio_only:
                    assert worker.frames == 0
                else:
                    assert not worker.state.get("spectrum")
            finally:
                worker.close()
        print("ASF: video and audio-only stream detection, playback, seek/pause and audio spectrum passed")
    finally:
        if hint is not None:
            os.environ["BIBLI_MEDIA_AUDIO_ONLY"] = hint


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path)
    parser.add_argument("--temporary", action="store_true")
    parser.add_argument("--generate-only", action="store_true")
    parser.add_argument("--archive-worker", default="target/debug/bibli-archive-worker")
    parser.add_argument("--media-worker", default="target/debug/bibli-media-worker")
    args = parser.parse_args()
    if not args.root and not args.temporary:
        parser.error("choose --root or --temporary")
    with tempfile.TemporaryDirectory(prefix="bibli-preview-check-") as temporary:
        root = args.root or Path(temporary)
        generate(root)
        if not args.generate_only:
            check_archive(args.archive_worker, root)
            check_audio(args.media_worker, root)
            check_asf(args.media_worker, root)


if __name__ == "__main__":
    main()
