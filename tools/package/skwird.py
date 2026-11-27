#!/usr/bin/env python3
"""skwird as skwir's packages carry it (ADR-050, amended 2026-11-26 and
2026-11-27): the release of skwir-org/skwird that tools/package/skwird.lock
pins, each archive checked against its sum before anything is read of it.

    tools/package/skwird.py windows <out>   # Windows' zip: skwird.exe, skwirdw.exe
    tools/package/skwird.py macos <out>     # macOS's archive: skwird, arm64, signed ad hoc
    tools/package/skwird.py linux <out>     # Linux's plain build: skwird, x86-64, glibc 2.34 or later
    tools/package/skwird.py fetch <windows|macos|linux> <downloads>   # fetched and checked alone

What is carried goes in <out> (the programs) and <out>/licenses/skwird:
skwird's licences (LICENSE-MIT, LICENSE-APACHE) and its notices
(THIRD-PARTY-NOTICES.txt: the crates built into its program for that
system, Rust's standard library, on Windows MinGW-w64's runtime), as its
archive holds them, byte for byte. A package puts the programs beside
skwir's own program, where skwir looks for them after PATH: Windows' zip
and installer, macOS's app and Linux's tarball (the AppImage carries
none, ADR-052). Nothing else of an archive is carried (Linux's systemd
units and INSTALL.txt are for a skwird set up by hand).

The archives are kept in <out>/.downloads, or the folder --downloads
names, and fetched again only when missing or no longer matching their
sums: a build that may not fetch (the AppImage's root) finds them there,
fetched first by `fetch`. What <out> held of an earlier run is made again.
"""

import argparse
import hashlib
import http.client
import pathlib
import shutil
import stat
import sys
import tarfile
import time
import urllib.error
import urllib.request
import zipfile

HERE = pathlib.Path(__file__).resolve().parent
LOCK = HERE / "skwird.lock"
REPOSITORY = "skwir-org/skwird"
# What every archive gives beside its programs, carried in licenses/skwird.
LICENCES = ("LICENSE-MIT", "LICENSE-APACHE", "THIRD-PARTY-NOTICES.txt")
# Each system's archive: what its name says of the system, its kind of
# archive, and the programs it gives.
ARCHIVES = {
    "windows": ("windows-x86_64", ".zip", ("skwird.exe", "skwirdw.exe")),
    "macos": ("macos-arm64", ".tar.gz", ("skwird",)),
    "linux": ("linux-x86_64", ".tar.gz", ("skwird",)),
}
ATTEMPTS = 3


class Refused(Exception):
    """What the lock or an archive holds is not what is carried."""


class Lock:
    """What skwird.lock pins: the release's tag and each of its files'
    sums."""

    def __init__(self, path=LOCK):
        self.tag, self.files = None, {}
        # The packages marked `unsigned`: an `.rpm` the repositories' key did
        # not sign, which Fedora's repository does not list (repository.py).
        self.unsigned = set()
        for line in pathlib.Path(path).read_text().splitlines():
            if not line.strip() or line.startswith("#"):
                continue
            words = line.split()
            if words[0] == "release" and len(words) == 2:
                self.tag = words[1]
            elif len(words) == 2 or (len(words) == 3 and words[2] == "unsigned"):
                self.files[words[0]] = words[1]
                if len(words) == 3:
                    self.unsigned.add(words[0])
            else:
                raise Refused(f"{path}: a line not understood: {line}")
        for name, sum_ in self.files.items():
            if len(sum_) != 64 or any(c not in "0123456789abcdef" for c in sum_):
                raise Refused(f"{path}: {name}'s sum is not a SHA-256")
        if not self.tag or not self.tag.startswith("v"):
            raise Refused(f"{path}: no release tag")
        self.version = self.tag[1:]

    def archive(self, kind):
        """The archive of `kind` (windows, macos or linux): its name, where
        it is published and its sum."""
        name = self.top(kind) + ARCHIVES[kind][1]
        if name not in self.files:
            raise Refused(f"skwird.lock pins no {name}")
        return name, f"https://github.com/{REPOSITORY}/releases/download/{self.tag}/{name}", self.files[name]

    def top(self, kind):
        """The one folder an archive of `kind` holds."""
        return f"skwird-{self.version}-{ARCHIVES[kind][0]}"


def sha256(path):
    digest = hashlib.sha256()
    with open(path, "rb") as file:
        for block in iter(lambda: file.read(1 << 20), b""):
            digest.update(block)
    return digest.hexdigest()


def download(url, partial, opener):
    """`url` into `partial`, tried again when the connection fails, not
    when the server refuses."""
    for attempt in range(1, ATTEMPTS + 1):
        try:
            with opener(url, timeout=120) as answer, open(partial, "wb") as file:
                while block := answer.read(1 << 20):
                    file.write(block)
            return
        except urllib.error.HTTPError as error:
            if error.code < 500 and error.code != 429:
                raise Refused(f"{url}: {error}") from error
            failure = error
        except (OSError, http.client.HTTPException) as error:
            failure = error
        if attempt == ATTEMPTS:
            raise Refused(f"{url}: {failure} ({ATTEMPTS} attempts)")
        time.sleep(5 * attempt)


def fetched(lock, kind, downloads, opener=urllib.request.urlopen):
    """The archive of `kind`, kept in `downloads` or fetched there, its sum
    the one pinned."""
    name, url, sum_ = lock.archive(kind)
    path = pathlib.Path(downloads) / name
    if path.is_file() and sha256(path) == sum_:
        return path
    path.parent.mkdir(parents=True, exist_ok=True)
    partial = path.with_name(path.name + ".part")
    download(url, partial, opener)
    if sha256(partial) != sum_:
        partial.unlink()
        raise Refused(f"{name}: its sum is not the one skwird.lock pins")
    partial.replace(path)
    return path


def member_path(name, top):
    """A member's path below the archive's one folder, refused if it
    leaves it."""
    path = pathlib.PurePosixPath(name)
    if path.is_absolute() or ".." in path.parts or not path.parts or path.parts[0] != top:
        raise Refused(f"{name}: outside the archive's folder {top}")
    return pathlib.PurePosixPath(*path.parts[1:])


def unpacked_zip(archive, top):
    """Each file a zip holds below its folder: path -> bytes. A link (a
    member whose Unix mode says so) is refused, as in a tar."""
    out = {}
    with zipfile.ZipFile(archive) as zipped:
        for info in zipped.infolist():
            path = member_path(info.filename.rstrip("/"), top)
            if stat.S_ISLNK(info.external_attr >> 16):
                raise Refused(f"{info.filename}: a link, which skwird's archives hold none of")
            if not info.is_dir() and path.parts:
                out[path] = zipped.read(info)
    return out


def unpacked_tar(archive, top):
    """Each file a gzipped tar holds below its folder: path -> (bytes,
    executable). Nothing else is taken: a link, symbolic or hard, is
    refused, as a device or a pipe is; skwird's archives hold none."""
    out = {}
    with tarfile.open(archive, "r:gz") as tar:
        for member in tar:
            path = member_path(member.name.rstrip("/"), top)
            if member.issym() or member.islnk():
                raise Refused(f"{member.name}: a link, which skwird's archives hold none of")
            if member.isdir():
                continue
            if not member.isfile():
                raise Refused(f"{member.name}: not a file or a folder")
            out[path] = (tar.extractfile(member).read(), bool(member.mode & 0o111))
    return out


def taken(files, name, kind):
    if pathlib.PurePosixPath(name) not in files:
        raise Refused(f"skwird's {kind} archive holds no {name}")
    return files[pathlib.PurePosixPath(name)]


def clear(out, names):
    for name in names:
        path = pathlib.Path(out) / name
        if path.is_dir() and not path.is_symlink():
            shutil.rmtree(path)
        elif path.exists() or path.is_symlink():
            path.unlink()


def carry(kind, out, downloads=None, lock=None, opener=urllib.request.urlopen):
    """skwird's programs for `kind` (windows, macos, linux) in `out`, its
    licences and notices in <out>/licenses/skwird."""
    lock = lock or Lock()
    out = pathlib.Path(out)
    archive = fetched(lock, kind, downloads or out / ".downloads", opener)
    top, programs = lock.top(kind), ARCHIVES[kind][2]
    clear(out, [*programs, "licenses/skwird"])
    if kind == "windows":
        # A zip keeps no mode Windows reads: each file is taken as it is.
        files = {path: (data, True) for path, data in unpacked_zip(archive, top).items()}
    else:
        files = unpacked_tar(archive, top)
    licences = out / "licenses/skwird"
    licences.mkdir(parents=True, exist_ok=True)
    for program in programs:
        data, executable = taken(files, program, kind)
        if not executable:
            raise Refused(f"skwird's {kind} archive holds {program} not executable")
        (out / program).write_bytes(data)
        (out / program).chmod(0o755)
    for name in LICENCES:
        (licences / name).write_bytes(taken(files, name, kind)[0])
    return out


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("what", choices=[*ARCHIVES, "fetch"])
    parser.add_argument("args", nargs="+")
    parser.add_argument("--downloads", type=pathlib.Path)
    args = parser.parse_args()
    try:
        if args.what == "fetch":
            if len(args.args) != 2 or args.args[0] not in ARCHIVES:
                parser.error(f"fetch <{'|'.join(ARCHIVES)}> <downloads>")
            print(fetched(Lock(), args.args[0], pathlib.Path(args.args[1])))
            return
        if len(args.args) != 1:
            parser.error(f"{args.what} <out>")
        carry(args.what, pathlib.Path(args.args[0]), args.downloads)
    except Refused as refused:
        print(f"skwird.py: {refused}", file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()
