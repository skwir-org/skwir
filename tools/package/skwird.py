#!/usr/bin/env python3
"""skwird as skwir's packages carry it (ADR-050, amended 2026-11-26): the
release of skwir-org/skwird that tools/package/skwird.lock pins, each file
checked against its sum before anything is read of it.

    tools/package/skwird.py windows <out>   # Windows' zip: skwird.exe, skwirdw.exe
    tools/package/skwird.py macos <out>     # macOS's archive: skwird, arm64, signed ad hoc
    tools/package/skwird.py source <out>    # the source archive: <out>/skwird-<version>, said
    tools/package/skwird.py fetch <windows|macos|source> <downloads>   # fetched and checked alone

What is carried goes in <out> (the programs) and <out>/licenses/skwird
(skwird's licences, LICENSE-MIT and LICENSE-APACHE, and the notices of
the crates its program links for that system: tools/package/notices,
`<system>/skwird` for Windows and macOS, `skwird` for Linux's tarball,
made from skwird's own Cargo.lock by notices.py). A package puts the
programs beside skwir's own program, where skwir looks for them after
PATH. The source is the tarball's, built in its root
(tools/package/appimage/skwird.sh): the AppImage carries no skwird.

The archives are kept in <out>/.downloads, or the folder --downloads
names, and fetched again only when missing or no longer matching their
sums: a build that may not fetch (the AppImage's root) finds them there,
fetched first by `fetch`. What <out> held of an earlier run is made again.
"""

import argparse
import hashlib
import http.client
import os
import pathlib
import shutil
import sys
import tarfile
import time
import urllib.error
import urllib.request
import zipfile

HERE = pathlib.Path(__file__).resolve().parent
LOCK = HERE / "skwird.lock"
NOTICES = HERE / "notices"
REPOSITORY = "skwir-org/skwird"
LICENCES = ("LICENSE-MIT", "LICENSE-APACHE")
# What each system's archive gives (beside the licences), and the notices
# of the crates its programs link.
PROGRAMS = {"windows": ("skwird.exe", "skwirdw.exe"), "macos": ("skwird",)}
NOTICE = {"windows": "windows/skwird", "macos": "macos/skwird", "source": "skwird"}
ATTEMPTS = 3


class Refused(Exception):
    """What the lock or an archive holds is not what is carried."""


class Lock:
    """What skwird.lock pins: the release's tag, each of its files' sums,
    and its source archive's name and sum, as GitHub serves it."""

    def __init__(self, path=LOCK):
        self.tag, self.files, self.source = None, {}, None
        # The packages marked `unsigned`: an `.rpm` the repositories' key did
        # not sign, which Fedora's repository does not list (repository.py).
        self.unsigned = set()
        for line in pathlib.Path(path).read_text().splitlines():
            if not line.strip() or line.startswith("#"):
                continue
            words = line.split()
            if words[0] == "release" and len(words) == 2:
                self.tag = words[1]
            elif words[0] == "source" and len(words) == 3:
                self.source = (words[1], words[2])
            elif len(words) == 2 or (len(words) == 3 and words[2] == "unsigned"):
                self.files[words[0]] = words[1]
                if len(words) == 3:
                    self.unsigned.add(words[0])
            else:
                raise Refused(f"{path}: a line not understood: {line}")
        for name, sum_ in [*self.files.items(), *([self.source] if self.source else [])]:
            if len(sum_) != 64 or any(c not in "0123456789abcdef" for c in sum_):
                raise Refused(f"{path}: {name}'s sum is not a SHA-256")
        if not self.tag or not self.tag.startswith("v") or not self.source:
            raise Refused(f"{path}: no release tag or no source archive")
        self.version = self.tag[1:]

    def archive(self, kind):
        """The archive of `kind` (windows, macos or source): the name it is
        kept under, where it is published and its sum."""
        if kind == "source":
            name, sum_ = self.source
            return (f"skwird-{name}", f"https://github.com/{REPOSITORY}/archive/refs/tags/{name}", sum_)
        name = {"windows": f"skwird-{self.version}-windows-x86_64.zip",
                "macos": f"skwird-{self.version}-macos-arm64.tar.gz"}[kind]
        if name not in self.files:
            raise Refused(f"skwird.lock pins no {name}")
        return name, f"https://github.com/{REPOSITORY}/releases/download/{self.tag}/{name}", self.files[name]

    def top(self, kind):
        """The one folder an archive of `kind` holds."""
        return {"windows": f"skwird-{self.version}-windows-x86_64",
                "macos": f"skwird-{self.version}-macos-arm64",
                "source": f"skwird-{self.version}"}[kind]


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
    """Each file a zip holds below its folder: path -> bytes."""
    out = {}
    with zipfile.ZipFile(archive) as zipped:
        for info in zipped.infolist():
            path = member_path(info.filename.rstrip("/"), top)
            if not info.is_dir() and path.parts:
                out[path] = zipped.read(info)
    return out


def unpacked_tar(archive, top, into=None):
    """Each file a gzipped tar holds below its folder: path -> (bytes,
    executable), or, with `into`, the whole tree written there: its
    folders, files (their modes executable or not, their dates the
    archive's, so that a build of a tree unpacked anew finds its crates
    unchanged) and relative links that stay in it. Nothing else is taken
    (no device, no link out)."""
    out = {}
    with tarfile.open(archive, "r:gz") as tar:
        for member in tar:
            path = member_path(member.name.rstrip("/"), top)
            if member.isdir():
                if into is not None:
                    (into / path).mkdir(parents=True, exist_ok=True)
                continue
            if member.issym():
                target = pathlib.PurePosixPath(member.linkname)
                if target.is_absolute() or _escapes(path.parent, target):
                    raise Refused(f"{member.name}: a link out of the archive's folder")
                if into is not None:
                    (into / path).parent.mkdir(parents=True, exist_ok=True)
                    (into / path).symlink_to(member.linkname)
                continue
            if not member.isfile():
                raise Refused(f"{member.name}: not a file, a folder or a link")
            data = tar.extractfile(member).read()
            executable = bool(member.mode & 0o111)
            if into is None:
                out[path] = (data, executable)
            else:
                (into / path).parent.mkdir(parents=True, exist_ok=True)
                (into / path).write_bytes(data)
                (into / path).chmod(0o755 if executable else 0o644)
                os.utime(into / path, (member.mtime, member.mtime))
    return out


def _escapes(folder, target):
    """Whether `target`, a link's, read from `folder`, leaves the tree."""
    depth = len(folder.parts)
    for part in target.parts:
        depth += -1 if part == ".." else 0 if part == "." else 1
        if depth < 0:
            return True
    return False


def taken(files, name, kind):
    if name not in files:
        raise Refused(f"skwird's {kind} archive holds no {name}")
    return files[name]


def licences(out, kind, licence_texts):
    """skwird's licences and its crates' notices for `kind`, in
    <out>/licenses/skwird."""
    folder = pathlib.Path(out) / "licenses/skwird"
    folder.mkdir(parents=True, exist_ok=True)
    for name, text in licence_texts.items():
        (folder / name).write_bytes(text)
    notices = NOTICES / NOTICE[kind] / "THIRD-PARTY-NOTICES"
    if not notices.is_file():
        raise Refused(f"no notices of skwird's crates in {notices}: run `just notices`")
    shutil.copyfile(notices, folder / "THIRD-PARTY-NOTICES")


def clear(out, names):
    for name in names:
        path = pathlib.Path(out) / name
        if path.is_dir() and not path.is_symlink():
            shutil.rmtree(path)
        elif path.exists() or path.is_symlink():
            path.unlink()


def carry(kind, out, downloads=None, lock=None, opener=urllib.request.urlopen):
    """skwird's programs for `kind` (windows, macos) in `out`, its licences
    in <out>/licenses/skwird."""
    lock = lock or Lock()
    out = pathlib.Path(out)
    archive = fetched(lock, kind, downloads or out / ".downloads", opener)
    top = lock.top(kind)
    clear(out, [*PROGRAMS[kind], "licenses/skwird"])
    out.mkdir(parents=True, exist_ok=True)
    if kind == "windows":
        files = unpacked_zip(archive, top)
        for program in PROGRAMS[kind]:
            (out / program).write_bytes(taken(files, pathlib.PurePosixPath(program), kind))
            (out / program).chmod(0o755)
        texts = {name: taken(files, pathlib.PurePosixPath(name), kind) for name in LICENCES}
    else:
        files = unpacked_tar(archive, top)
        for program in PROGRAMS[kind]:
            data, executable = taken(files, pathlib.PurePosixPath(program), kind)
            if not executable:
                raise Refused(f"skwird's {kind} archive holds {program} not executable")
            (out / program).write_bytes(data)
            (out / program).chmod(0o755)
        texts = {name: taken(files, pathlib.PurePosixPath(name), kind)[0] for name in LICENCES}
    licences(out, kind, texts)
    return out


def source(out, downloads=None, lock=None, opener=urllib.request.urlopen, notices=True):
    """skwird's source tree, <out>/skwird-<version>, unpacked anew; with
    `notices`, its licences and its crates' notices for Linux in
    <out>/licenses/skwird, for the program built from it."""
    lock = lock or Lock()
    out = pathlib.Path(out)
    archive = fetched(lock, "source", downloads or out / ".downloads", opener)
    top = lock.top("source")
    clear(out, [top, "licenses/skwird"])
    tree = out / top
    tree.mkdir(parents=True)
    unpacked_tar(archive, top, into=tree)
    for name in LICENCES:
        if not (tree / name).is_file():
            raise Refused(f"skwird's source holds no {name}")
    if notices:
        licences(out, "source", {name: (tree / name).read_bytes() for name in LICENCES})
    return tree


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("what", choices=["windows", "macos", "source", "fetch"])
    parser.add_argument("args", nargs="+")
    parser.add_argument("--downloads", type=pathlib.Path)
    args = parser.parse_args()
    try:
        if args.what == "fetch":
            if len(args.args) != 2 or args.args[0] not in ("windows", "macos", "source"):
                parser.error("fetch <windows|macos|source> <downloads>")
            print(fetched(Lock(), args.args[0], pathlib.Path(args.args[1])))
            return
        if len(args.args) != 1:
            parser.error(f"{args.what} <out>")
        if args.what == "source":
            print(source(pathlib.Path(args.args[0]), args.downloads))
        else:
            carry(args.what, pathlib.Path(args.args[0]), args.downloads)
    except Refused as refused:
        print(f"skwird.py: {refused}", file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()
