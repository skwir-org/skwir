"""skwird as skwir's packages carry it (tools/package/skwird.py, ADR-050
amended 2026-11-26 and 2026-11-27): the lock read, each archive checked
against its sum before it is read, and what Windows' zip and macOS's and
Linux's archives give, their licences and notices as they hold them,
nothing outside their folder taken and no link at all."""

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
        self.patch(skwird, "ATTEMPTS", 1)

    def patch(self, owner, name, value):
        kept = getattr(owner, name)
        setattr(owner, name, value)
        self.addCleanup(setattr, owner, name, kept)

    def lock(self, **archives):
        """A lock of v1.2.3 pinning the archives given by system (their
        bytes), and an opener serving them."""
        files = {f"skwird-1.2.3-{skwird.ARCHIVES[kind][0]}{skwird.ARCHIVES[kind][1]}": data
                 for kind, data in archives.items()}
        lines = ["# a lock", "release v1.2.3",
                 *(f"{name} {hashlib.sha256(data).hexdigest()}" for name, data in files.items())]
        path = self.folder / "skwird.lock"
        path.write_text("\n".join(lines) + "\n")
        return skwird.Lock(path), Served(files)


class TheLock(Case):
    def test_the_real_lock_pins_what_the_packages_carry(self):
        lock = skwird.Lock()
        self.assertTrue(lock.tag.startswith("v") and lock.version == lock.tag[1:])
        for kind in skwird.ARCHIVES:
            name, url, sum_ = lock.archive(kind)
            self.assertEqual(url, f"https://github.com/skwir-org/skwird/releases/download/{lock.tag}/{name}")
            self.assertEqual(len(sum_), 64)
        # Every package the repositories list is pinned, and none is marked:
        # skwird's release signs its `.rpm` (repository.py).
        self.assertTrue(any(name.endswith(".rpm") for name in lock.files), lock.files)
        self.assertEqual(lock.unsigned, set())

    def test_a_lock_without_a_sum_or_a_tag_or_with_a_line_not_understood_is_refused(self):
        path = self.folder / "bad.lock"
        for text in ("release v1.2.3\nskwird-1.2.3-windows-x86_64.zip abc\n",
                     f"skwird-1.2.3-windows-x86_64.zip {'0' * 64}\n",
                     # The source archive, which no package reads any more.
                     f"release v1.2.3\nsource v1.2.3.tar.gz {'0' * 64}\n"):
            path.write_text(text)
            with self.assertRaises(skwird.Refused, msg=text):
                skwird.Lock(path)

    def test_a_package_marked_unsigned_is_pinned_and_no_other_word_is_taken(self):
        path = self.folder / "marked.lock"
        pinned = "release v1.2.3\n"
        path.write_text(pinned + f"skwird-1.2.3-1.x86_64.rpm {'a' * 64} unsigned\nskwird_1.2.3-1_amd64.deb {'b' * 64}\n")
        lock = skwird.Lock(path)
        self.assertEqual(lock.files, {"skwird-1.2.3-1.x86_64.rpm": "a" * 64, "skwird_1.2.3-1_amd64.deb": "b" * 64})
        self.assertEqual(lock.unsigned, {"skwird-1.2.3-1.x86_64.rpm"})
        path.write_text(pinned + f"skwird-1.2.3-1.x86_64.rpm {'a' * 64} signed\n")
        with self.assertRaises(skwird.Refused):
            skwird.Lock(path)

    def test_an_archive_not_pinned_is_refused(self):
        lock, _ = self.lock(windows=b"pinned")
        with self.assertRaisesRegex(skwird.Refused, "pins no skwird-1.2.3-linux-x86_64.tar.gz"):
            lock.archive("linux")


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

    def test_linux_archive_is_fetched_from_the_release(self):
        lock, served = self.lock(linux=b"pinned")
        path = skwird.fetched(lock, "linux", self.folder, served)
        self.assertEqual(path.name, "skwird-1.2.3-linux-x86_64.tar.gz")
        self.assertEqual(served.asked, ["https://github.com/skwir-org/skwird/releases/download/v1.2.3/"
                                        "skwird-1.2.3-linux-x86_64.tar.gz"])


# What each system's archive holds, as skwird 0.3.0's do.
TEXTS = {"LICENSE-MIT": b"mit", "LICENSE-APACHE": b"apache", "THIRD-PARTY-NOTICES.txt": b"crates built in"}
WINDOWS = {"skwird-1.2.3-windows-x86_64/": b"", "skwird-1.2.3-windows-x86_64/skwird.exe": b"MZ engine",
           "skwird-1.2.3-windows-x86_64/skwirdw.exe": b"MZ windowless",
           **{f"skwird-1.2.3-windows-x86_64/{name}": text for name, text in TEXTS.items()}}
MACOS = {"skwird-1.2.3-macos-arm64": None, "skwird-1.2.3-macos-arm64/skwird*": b"\xcf\xfa\xed\xfe",
         **{f"skwird-1.2.3-macos-arm64/{name}": text for name, text in TEXTS.items()}}
LINUX = {"skwird-1.2.3-linux-x86_64": None, "skwird-1.2.3-linux-x86_64/systemd": None,
         "skwird-1.2.3-linux-x86_64/systemd/skwird.service": b"[Service]",
         "skwird-1.2.3-linux-x86_64/skwird*": b"\x7fELF", "skwird-1.2.3-linux-x86_64/INSTALL.txt": b"by hand",
         **{f"skwird-1.2.3-linux-x86_64/{name}": text for name, text in TEXTS.items()}}


class Carried(Case):
    def carried_texts(self, out):
        folder = out / "licenses/skwird"
        return {p.name: p.read_bytes() for p in folder.iterdir()}

    def test_windows_programs_licences_and_notices(self):
        lock, served = self.lock(windows=zipped(WINDOWS))
        out = self.folder / "out"
        # What an earlier run left goes.
        (out / "licenses/skwird").mkdir(parents=True)
        (out / "licenses/skwird/THIRD-PARTY-NOTICES").write_text("made by an older skwir from skwird's lock")
        skwird.carry("windows", out, lock=lock, opener=served)
        self.assertEqual((out / "skwird.exe").read_bytes(), b"MZ engine")
        self.assertEqual((out / "skwirdw.exe").read_bytes(), b"MZ windowless")
        self.assertEqual(self.carried_texts(out), TEXTS)

    def test_a_windows_zip_lacking_the_windowless_program_or_the_notices_is_refused(self):
        for left in ("skwirdw.exe", "THIRD-PARTY-NOTICES.txt"):
            members = {n: d for n, d in WINDOWS.items() if not n.endswith(left)}
            lock, served = self.lock(windows=zipped(members))
            with self.assertRaisesRegex(skwird.Refused, left):
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
        self.assertEqual(self.carried_texts(out), TEXTS)

    def test_a_program_not_executable_is_refused(self):
        for kind, members in (("macos", MACOS), ("linux", LINUX)):
            lock, served = self.lock(**{kind: tarred({n.rstrip("*"): d for n, d in members.items()})})
            with self.assertRaisesRegex(skwird.Refused, "not executable", msg=kind):
                skwird.carry(kind, self.folder / "out", lock=lock, opener=served)

    def test_a_link_in_a_tar_is_refused(self):
        for kind, members in (("macos", MACOS), ("linux", LINUX)):
            top = f"skwird-1.2.3-{skwird.ARCHIVES[kind][0]}"
            for link in (("link", "LICENSE-MIT"), ("hard", f"{top}/LICENSE-MIT")):
                lock, served = self.lock(**{kind: tarred({**members, f"{top}/extra": link})})
                with self.assertRaisesRegex(skwird.Refused, "a link", msg=(kind, link)):
                    skwird.carry(kind, self.folder / "out", lock=lock, opener=served)

    def test_linux_program_licences_and_notices_and_nothing_else(self):
        lock, served = self.lock(linux=tarred(LINUX))
        out = self.folder / "out"
        skwird.carry("linux", out, lock=lock, opener=served)
        self.assertEqual((out / "skwird").read_bytes(), b"\x7fELF")
        self.assertTrue((out / "skwird").stat().st_mode & 0o100)
        self.assertEqual(self.carried_texts(out), TEXTS)
        # Its systemd units and INSTALL.txt are for a skwird set up by hand.
        self.assertEqual(sorted(p.name for p in out.iterdir()), [".downloads", "licenses", "skwird"])

    def test_a_chain_of_links_writes_nothing_out_of_the_folder(self):
        # Each link, read as text from where it stands, stays in the folder:
        # `x/l1` is the folder itself, so `x/l1/l2` is the one above it, and
        # the file below would be written there (review P2-4).
        top = "skwird-1.2.3-linux-x86_64"
        lock, served = self.lock(linux=tarred([
            *((f"{top}/{n}", d) for n, d in TEXTS.items()), (f"{top}/skwird*", b"\x7fELF"), (f"{top}/x", None),
            (f"{top}/x/l1", ("link", "..")), (f"{top}/x/l1/l2", ("link", "..")),
            (f"{top}/x/l1/l2/escaped.txt", b"out of the folder")]))
        out = self.folder / "out"
        with self.assertRaisesRegex(skwird.Refused, "a link"):
            skwird.carry("linux", out, lock=lock, opener=served)
        self.assertFalse((out / "escaped.txt").exists(), "written out of the folder")
        self.assertEqual([p for p in self.folder.rglob("*") if p.is_symlink()], [])

    def test_a_special_file_is_refused(self):
        lock, served = self.lock(linux=tarred({**LINUX, "skwird-1.2.3-linux-x86_64/pipe": ("fifo",)}))
        with self.assertRaisesRegex(skwird.Refused, "not a file"):
            skwird.carry("linux", self.folder / "out", lock=lock, opener=served)


if __name__ == "__main__":
    unittest.main()
