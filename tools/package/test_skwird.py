"""skwird as skwir's packages carry it (tools/package/skwird.py, ADR-050
amended 2026-11-26): the lock read, each archive checked against its sum
before it is read, and what Windows' zip, macOS's archive and the source
give, nothing outside their folder taken and no link at all."""

import hashlib
import io
import pathlib
import sys
import tarfile
import tempfile
import unittest
import zipfile

HERE = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import skwird  # noqa: E402


def zipped(members, links=()):
    """A zip of `members`: name -> bytes, the names in `links` marked as
    links (Unix mode 0120777), their bytes the target."""
    out = io.BytesIO()
    with zipfile.ZipFile(out, "w") as archive:
        for name, data in members.items():
            info = zipfile.ZipInfo(name)
            if name in links:
                info.external_attr = 0o120777 << 16
            archive.writestr(info, data)
    return out.getvalue()


# When an archive's members were made.
MADE = 1_700_000_000


def tarred(members):
    """A gzipped tar of `members`, in their order: name -> bytes (a file,
    mode 0644, or 0755 if the name ends in `*`), None (a folder), ("link",
    target), ("hard", target) or ("fifo",), each dated MADE. `members` may
    be a list of (name, what), to name a member twice."""
    out = io.BytesIO()
    with tarfile.open(fileobj=out, mode="w:gz") as archive:
        for name, what in (members.items() if isinstance(members, dict) else members):
            info = tarfile.TarInfo(name.rstrip("*"))
            info.mtime = MADE
            if what is None:
                info.type, info.mode = tarfile.DIRTYPE, 0o755
                archive.addfile(info)
            elif isinstance(what, tuple) and what[0] in ("link", "hard"):
                info.type = tarfile.SYMTYPE if what[0] == "link" else tarfile.LNKTYPE
                info.linkname = what[1]
                archive.addfile(info)
            elif isinstance(what, tuple):
                info.type = tarfile.FIFOTYPE
                archive.addfile(info)
            else:
                info.size, info.mode = len(what), 0o755 if name.endswith("*") else 0o644
                archive.addfile(info, io.BytesIO(what))
    return out.getvalue()


class Served:
    """An opener serving `files` by the last part of their address, and
    counting what it was asked."""

    def __init__(self, files):
        self.files, self.asked = files, []

    def __call__(self, url, timeout):
        self.asked.append(url)
        return io.BytesIO(self.files[url.rsplit("/", 1)[1]])


class Case(unittest.TestCase):
    def setUp(self):
        self.folder = pathlib.Path(tempfile.mkdtemp(prefix="skwird-carried-"))
        self.addCleanup(lambda: __import__("shutil").rmtree(self.folder))
        notices = self.folder / "notices"
        for kind, where in skwird.NOTICE.items():
            (notices / where).mkdir(parents=True)
            (notices / where / "THIRD-PARTY-NOTICES").write_text(f"crates of skwird for {kind}\n")
        self.patch(skwird, "NOTICES", notices)
        self.patch(skwird, "ATTEMPTS", 1)

    def patch(self, owner, name, value):
        kept = getattr(owner, name)
        setattr(owner, name, value)
        self.addCleanup(setattr, owner, name, kept)

    def lock(self, windows=None, macos=None, source=None):
        """A lock of v1.2.3 pinning the archives given (their bytes), and
        an opener serving them."""
        files = {"skwird-1.2.3-windows-x86_64.zip": windows, "skwird-1.2.3-macos-arm64.tar.gz": macos,
                 "v1.2.3.tar.gz": source}
        files = {name: data for name, data in files.items() if data is not None}
        lines = ["# a lock", "release v1.2.3"]
        for name, data in files.items():
            sum_ = hashlib.sha256(data).hexdigest()
            lines.append(f"source {name} {sum_}" if name == "v1.2.3.tar.gz" else f"{name} {sum_}")
        if source is None:
            lines.append(f"source v1.2.3.tar.gz {'0' * 64}")
        path = self.folder / "skwird.lock"
        path.write_text("\n".join(lines) + "\n")
        return skwird.Lock(path), Served(files)


class TheLock(Case):
    def test_the_real_lock_pins_what_the_packages_carry(self):
        lock = skwird.Lock()
        self.assertTrue(lock.tag.startswith("v") and lock.version == lock.tag[1:])
        for kind in ("windows", "macos", "source"):
            name, url, sum_ = lock.archive(kind)
            self.assertTrue(url.startswith("https://github.com/skwir-org/skwird/"), url)
            self.assertEqual(len(sum_), 64)
        self.assertEqual(lock.archive("source")[0], f"skwird-{lock.tag}.tar.gz")

    def test_the_notices_of_each_system_carrying_it_are_made(self):
        for where in skwird.NOTICE.values():
            notices = HERE / "notices" / where / "THIRD-PARTY-NOTICES"
            self.assertTrue(notices.is_file(), f"{notices}: run `just notices`")
            self.assertIn(f"skwird {skwird.Lock().version}", notices.read_text())

    def test_a_lock_without_a_sum_or_a_source_is_refused(self):
        path = self.folder / "bad.lock"
        path.write_text("release v1.2.3\nskwird-1.2.3-windows-x86_64.zip abc\nsource v1.2.3.tar.gz " + "0" * 64 + "\n")
        with self.assertRaises(skwird.Refused):
            skwird.Lock(path)
        path.write_text("release v1.2.3\n")
        with self.assertRaises(skwird.Refused):
            skwird.Lock(path)

    def test_a_package_marked_unsigned_is_pinned_and_no_other_word_is_taken(self):
        path = self.folder / "marked.lock"
        pinned = "release v1.2.3\nsource v1.2.3.tar.gz " + "0" * 64 + "\n"
        path.write_text(pinned + f"skwird-1.2.3-1.x86_64.rpm {'a' * 64} unsigned\nskwird_1.2.3-1_amd64.deb {'b' * 64}\n")
        lock = skwird.Lock(path)
        self.assertEqual(lock.files, {"skwird-1.2.3-1.x86_64.rpm": "a" * 64, "skwird_1.2.3-1_amd64.deb": "b" * 64})
        self.assertEqual(lock.unsigned, {"skwird-1.2.3-1.x86_64.rpm"})
        path.write_text(pinned + f"skwird-1.2.3-1.x86_64.rpm {'a' * 64} signed\n")
        with self.assertRaises(skwird.Refused):
            skwird.Lock(path)


class Fetching(Case):
    def test_an_archive_whose_sum_differs_is_refused_and_not_kept(self):
        lock, served = self.lock(windows=b"pinned")
        served.files["skwird-1.2.3-windows-x86_64.zip"] = b"another"
        with self.assertRaises(skwird.Refused):
            skwird.fetched(lock, "windows", self.folder / "downloads", served)
        self.assertEqual(list((self.folder / "downloads").iterdir()), [])

    def test_an_archive_kept_is_read_again_only_if_it_changed(self):
        lock, served = self.lock(windows=b"pinned")
        downloads = self.folder / "downloads"
        path = skwird.fetched(lock, "windows", downloads, served)
        self.assertEqual(path.read_bytes(), b"pinned")
        skwird.fetched(lock, "windows", downloads, served)
        self.assertEqual(len(served.asked), 1)
        path.write_bytes(b"changed")
        skwird.fetched(lock, "windows", downloads, served)
        self.assertEqual((len(served.asked), path.read_bytes()), (2, b"pinned"))
        self.assertEqual(served.asked[0], "https://github.com/skwir-org/skwird/releases/download/v1.2.3/"
                                          "skwird-1.2.3-windows-x86_64.zip")

    def test_the_source_is_fetched_from_its_tag(self):
        lock, served = self.lock(source=tarred({"skwird-1.2.3/LICENSE-MIT": b"m"}))
        path = skwird.fetched(lock, "source", self.folder, served)
        self.assertEqual(path.name, "skwird-v1.2.3.tar.gz")
        self.assertEqual(served.asked, ["https://github.com/skwir-org/skwird/archive/refs/tags/v1.2.3.tar.gz"])


WINDOWS = {"skwird-1.2.3-windows-x86_64/": b"", "skwird-1.2.3-windows-x86_64/skwird.exe": b"MZ engine",
           "skwird-1.2.3-windows-x86_64/skwirdw.exe": b"MZ windowless",
           "skwird-1.2.3-windows-x86_64/LICENSE-MIT": b"mit", "skwird-1.2.3-windows-x86_64/LICENSE-APACHE": b"apache"}
MACOS = {"skwird-1.2.3-macos-arm64": None, "skwird-1.2.3-macos-arm64/skwird*": b"\xcf\xfa\xed\xfe",
         "skwird-1.2.3-macos-arm64/LICENSE-MIT": b"mit", "skwird-1.2.3-macos-arm64/LICENSE-APACHE": b"apache"}


class Carried(Case):
    def test_windows_programs_and_licences(self):
        lock, served = self.lock(windows=zipped(WINDOWS))
        out = self.folder / "out"
        # What an earlier run left goes.
        (out / "licenses/skwird").mkdir(parents=True)
        (out / "licenses/skwird/OLD").write_text("taken by an older lock")
        skwird.carry("windows", out, lock=lock, opener=served)
        self.assertEqual((out / "skwird.exe").read_bytes(), b"MZ engine")
        self.assertEqual((out / "skwirdw.exe").read_bytes(), b"MZ windowless")
        self.assertEqual(sorted(p.name for p in (out / "licenses/skwird").iterdir()),
                         ["LICENSE-APACHE", "LICENSE-MIT", "THIRD-PARTY-NOTICES"])
        self.assertEqual((out / "licenses/skwird/THIRD-PARTY-NOTICES").read_text(), "crates of skwird for windows\n")

    def test_a_windows_zip_lacking_the_windowless_program_is_refused(self):
        members = {n: d for n, d in WINDOWS.items() if not n.endswith("skwirdw.exe")}
        lock, served = self.lock(windows=zipped(members))
        with self.assertRaisesRegex(skwird.Refused, "skwirdw.exe"):
            skwird.carry("windows", self.folder / "out", lock=lock, opener=served)

    def test_a_member_outside_the_folder_is_refused(self):
        lock, served = self.lock(windows=zipped({**WINDOWS, "skwird-1.2.3-windows-x86_64/../evil.exe": b"x"}))
        with self.assertRaises(skwird.Refused):
            skwird.carry("windows", self.folder / "out", lock=lock, opener=served)
        self.assertFalse((self.folder / "evil.exe").exists())

    def test_a_link_in_the_windows_zip_is_refused(self):
        # A link standing for the program would be carried as its target's
        # name, written as the program.
        name = "skwird-1.2.3-windows-x86_64/skwird.exe"
        lock, served = self.lock(windows=zipped({**WINDOWS, name: b"../../elsewhere.exe"}, links={name}))
        with self.assertRaisesRegex(skwird.Refused, "a link"):
            skwird.carry("windows", self.folder / "out", lock=lock, opener=served)
        self.assertFalse((self.folder / "out/skwird.exe").exists())

    def test_macos_program_executable(self):
        lock, served = self.lock(macos=tarred(MACOS))
        out = self.folder / "out"
        skwird.carry("macos", out, lock=lock, opener=served)
        self.assertEqual((out / "skwird").read_bytes(), b"\xcf\xfa\xed\xfe")
        self.assertTrue((out / "skwird").stat().st_mode & 0o100)
        self.assertEqual((out / "licenses/skwird/THIRD-PARTY-NOTICES").read_text(), "crates of skwird for macos\n")

    def test_a_macos_program_not_executable_is_refused(self):
        members = {n.rstrip("*"): d for n, d in MACOS.items()}
        lock, served = self.lock(macos=tarred(members))
        with self.assertRaisesRegex(skwird.Refused, "not executable"):
            skwird.carry("macos", self.folder / "out", lock=lock, opener=served)

    def test_a_link_in_the_macos_archive_is_refused(self):
        for link in (("link", "LICENSE-MIT"), ("hard", "skwird-1.2.3-macos-arm64/LICENSE-MIT")):
            lock, served = self.lock(macos=tarred({**MACOS, "skwird-1.2.3-macos-arm64/extra": link}))
            with self.assertRaisesRegex(skwird.Refused, "a link", msg=link):
                skwird.carry("macos", self.folder / "out", lock=lock, opener=served)


class Source(Case):
    def test_the_tree_and_its_modes(self):
        lock, served = self.lock(source=tarred({
            "skwird-1.2.3": None, "skwird-1.2.3/Cargo.toml": b"[workspace]\n",
            "skwird-1.2.3/LICENSE-MIT": b"mit", "skwird-1.2.3/LICENSE-APACHE": b"apache",
            "skwird-1.2.3/tools/gate.sh*": b"#!/bin/sh\n", "skwird-1.2.3/AGENTS.md": b"rules"}))
        out = self.folder / "out"
        tree = skwird.source(out, lock=lock, opener=served)
        self.assertEqual(tree, out / "skwird-1.2.3")
        self.assertEqual((tree / "Cargo.toml").read_text(), "[workspace]\n")
        self.assertTrue((tree / "tools/gate.sh").stat().st_mode & 0o100)
        self.assertFalse((tree / "Cargo.toml").stat().st_mode & 0o100)
        # Dated as the archive dates them: a build finds its crates unchanged.
        self.assertEqual((tree / "Cargo.toml").stat().st_mtime, MADE)
        self.assertEqual((tree / "AGENTS.md").read_text(), "rules")
        self.assertEqual((out / "licenses/skwird/THIRD-PARTY-NOTICES").read_text(), "crates of skwird for source\n")
        # Unpacked anew: what an earlier run left in the tree goes.
        (tree / "stale.rs").write_text("")
        skwird.source(out, lock=lock, opener=served, notices=False)
        self.assertFalse((tree / "stale.rs").exists())

    def test_every_link_is_refused_and_none_is_made(self):
        # Inside the tree or out of it, symbolic or hard: skwird's tree
        # holds none.
        for link in (("link", "AGENTS.md"), ("link", "../../outside"), ("link", "/etc/passwd"),
                     ("link", "sub/../../.."), ("hard", "skwird-1.2.3/LICENSE-MIT")):
            lock, served = self.lock(source=tarred({
                "skwird-1.2.3/LICENSE-MIT": b"m", "skwird-1.2.3/LICENSE-APACHE": b"a",
                "skwird-1.2.3/AGENTS.md": b"rules", "skwird-1.2.3/linked": link}))
            out = self.folder / "out"
            with self.assertRaisesRegex(skwird.Refused, "a link", msg=link):
                skwird.source(out, lock=lock, opener=served)
            self.assertFalse((out / "skwird-1.2.3/linked").is_symlink(), link)
            self.assertFalse((out / "skwird-1.2.3/linked").exists(), link)

    def test_a_chain_of_links_writes_nothing_out_of_the_tree(self):
        # Each link, read as text from where it stands, stays in the tree:
        # `x/l1` is the tree itself, so `x/l1/l2` is the folder above it,
        # and the file below would be written there (review P2-4).
        lock, served = self.lock(source=tarred([
            ("skwird-1.2.3/LICENSE-MIT", b"m"), ("skwird-1.2.3/LICENSE-APACHE", b"a"), ("skwird-1.2.3/x", None),
            ("skwird-1.2.3/x/l1", ("link", "..")), ("skwird-1.2.3/x/l1/l2", ("link", "..")),
            ("skwird-1.2.3/x/l1/l2/escaped.txt", b"out of the tree")]))
        out = self.folder / "out"
        refused = None
        try:
            skwird.source(out, lock=lock, opener=served)
        except skwird.Refused as error:
            refused = error
        self.assertFalse((out / "escaped.txt").exists(), "written out of the tree")
        self.assertEqual([p for p in out.rglob("*") if p.is_symlink()], [])
        self.assertRegex(str(refused), "a link")

    def test_a_special_file_is_refused(self):
        lock, served = self.lock(source=tarred({"skwird-1.2.3/LICENSE-MIT": b"m", "skwird-1.2.3/pipe": ("fifo",)}))
        with self.assertRaisesRegex(skwird.Refused, "not a file"):
            skwird.source(self.folder / "out", lock=lock, opener=served)


if __name__ == "__main__":
    unittest.main()
