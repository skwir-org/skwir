"""What tools/package/repository.py (ADR-142) refuses before it signs: a
bundle whose packages are not the release's, a key that cannot sign or will
not be valid long enough. The repositories themselves are tried by
repository-check.sh."""

import datetime
import hashlib
import io
import json
import os
import pathlib
import shutil
import subprocess
import sys
import tarfile
import tempfile
import unittest
import unittest.mock

HERE = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import repository  # noqa: E402

NOW = datetime.datetime(2026, 11, 20, tzinfo=datetime.UTC)
OLD = "30CE3EFEEAF349BA66124FFACF2B299583B07B74"
NEW = "5EC74D75F88F99F79FEB60961E7E8BED8F15497C"
SUBKEY = "8BE3663FE08B78EF8BE3663FE08B78EF8BE3663F"


def listing(*keys):
    """A `gpg --with-colons --show-keys` listing of `keys` (fingerprint,
    expiry), each with a subkey whose fingerprint is not the key's."""
    lines = []
    for fingerprint, expires in keys:
        lines += [f"pub:-:255:22:{fingerprint[-16:]}:1795118515:{expires or ''}::-:::scSC:::::ed25519:::0:",
                  f"fpr:::::::::{fingerprint}:",
                  "uid:-::::1795118515::50636F6975FC00D654B73AF65858340C20A5D1C0::Skwir repositories::::::::::0:",
                  f"sub:-:255:18:{SUBKEY[-16:]}:1795118515::::::e:::::cv25519::",
                  f"fpr:::::::::{SUBKEY}:"]
    return "\n".join(lines) + "\n"


class Bundle(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.bundle = pathlib.Path(self.directory.name) / "v1.2.3"
        self.bundle.mkdir()
        assets = {}
        for name, body in (("skwir_1.2.3-1_amd64.deb", b"deb"), ("skwir-1.2.3-1.x86_64.rpm", b"rpm"),
                           ("skwir-1.2.3-x86_64.AppImage", b"image")):
            (self.bundle / name).write_bytes(body)
            assets[name] = {"sha256": hashlib.sha256(body).hexdigest(), "bytes": len(body)}
        (self.bundle / "manifest.json").write_text(json.dumps({"tag": "v1.2.3", "assets": assets}))

    def tearDown(self):
        self.directory.cleanup()

    def refused(self):
        with self.assertRaises(SystemExit) as refusal:
            repository.released(self.bundle)
        return str(refusal.exception.code)

    def test_the_release_s_packages_are_listed(self):
        self.assertEqual(repository.released(self.bundle)["tag"], "v1.2.3")

    def test_a_package_changed_since_the_release_is_refused(self):
        (self.bundle / "skwir_1.2.3-1_amd64.deb").write_bytes(b"DEB")
        self.assertIn("skwir_1.2.3-1_amd64.deb: changed since the release built it", self.refused())

    def test_a_package_the_release_did_not_build_is_refused(self):
        (self.bundle / "skwir-extra_1.2.3-1_amd64.deb").write_bytes(b"deb")
        self.assertIn("skwir-extra_1.2.3-1_amd64.deb: not a file the release built", self.refused())

    def test_a_package_missing_is_refused(self):
        (self.bundle / "skwir-1.2.3-1.x86_64.rpm").unlink()
        self.assertIn("skwir-1.2.3-1.x86_64.rpm: recorded by its manifest.json, and missing", self.refused())

    def test_what_is_not_a_package_is_not_the_repositories(self):
        (self.bundle / "skwir-1.2.3-x86_64.AppImage").write_bytes(b"other")
        self.assertEqual(repository.released(self.bundle)["tag"], "v1.2.3")


class Keys(unittest.TestCase):
    def test_each_key_is_read_with_its_expiry_and_never_its_subkey(self):
        self.assertEqual(repository.keys(listing((OLD, None), (NEW, 1826654515))),
                         [(OLD, None), (NEW, 1826654515)])

    def test_the_key_signs_with_the_key_coming_beside_it(self):
        later = int((NOW + datetime.timedelta(days=400)).timestamp())
        self.assertEqual(repository.unfit((OLD, None), None, True, NOW), [])
        self.assertEqual(repository.unfit((OLD, None), (NEW, later), True, NOW), [])

    def test_a_key_whose_secret_part_is_not_here_never_signs(self):
        said = repository.unfit((OLD, None), (NEW, None), False, NOW)
        self.assertEqual(len(said), 1)
        self.assertTrue(said[0].startswith(f"{OLD}: its secret part is not in GnuPG's keyring"))

    def test_the_key_coming_is_another(self):
        said = repository.unfit((OLD, None), (OLD, None), True, NOW)
        self.assertEqual(said, [f"{OLD}: the key coming is the key that signs"])

    def test_a_key_that_expires_before_its_extension_can_reach_every_system_is_refused(self):
        soon = int((NOW + repository.LEAD - datetime.timedelta(days=1)).timestamp())
        later = int((NOW + repository.LEAD + datetime.timedelta(days=1)).timestamp())
        for signing, coming in (((OLD, soon), None), ((OLD, later), (NEW, soon))):
            said = repository.unfit(signing, coming, True, NOW)
            self.assertEqual(len(said), 1)
            self.assertIn(" 5y), export it again, raise tools/package/repository/version", said[0])
        self.assertEqual(repository.unfit((OLD, later), (NEW, later), True, NOW), [])


def deb(path, version, body=b""):
    """A `.deb` as nfpm makes one: its version in its control, `body` its
    data."""
    def member(name, data):
        return f"{name:<16}{0:<12}{0:<6}{0:<6}{100644:<8}{len(data):<10}`\n".encode() + data + b"\n" * (len(data) % 2)
    control = io.BytesIO()
    with tarfile.open(fileobj=control, mode="w:gz") as archive:
        text = f"Package: skwir-repository\nVersion: {version}\n".encode()
        info = tarfile.TarInfo("./control")
        info.size = len(text)
        archive.addfile(info, io.BytesIO(text))
    path.write_bytes(b"!<arch>\n" + member("debian-binary", b"2.0\n") + member("control.tar.gz", control.getvalue())
                     + member("data.tar.gz", body))


class Dated(unittest.TestCase):
    def test_skwir_repository_is_dated_by_its_version_file_whatever_the_shell_says(self):
        said = subprocess.run(["git", "-C", str(HERE.parents[1]), "log", "-1", "--format=%ct", "--",
                               "tools/package/repository/version"], capture_output=True, text=True, check=True)
        with unittest.mock.patch.dict(os.environ, {"SOURCE_DATE_EPOCH": "315532800"}):
            self.assertEqual(repository.epoch(), said.stdout.strip())

    def test_a_version_published_is_never_made_again_with_other_bytes(self):
        with tempfile.TemporaryDirectory() as folder:
            made, site = pathlib.Path(folder) / "made", pathlib.Path(folder) / "site"
            made.mkdir()
            site.mkdir()
            deb(made / "skwir-repository-debian.deb", "2-1", b"what it holds")
            deb(site / "skwir-repository-debian.deb", "2-1", b"what it holds")
            self.assertEqual(list(repository.changed_since(made, site)), [])
            deb(site / "skwir-repository-debian.deb", "1-1", b"what it held")
            self.assertEqual(list(repository.changed_since(made, site)), [])
            deb(site / "skwir-repository-debian.deb", "2-1", b"what it held")
            self.assertEqual(list(repository.changed_since(made, site)), [site / "skwir-repository-debian.deb"])
            self.assertEqual(list(repository.changed_since(made, site / "none")), [])


@unittest.skipUnless(shutil.which("gpg") and shutil.which("gpgv"), "needs gpg and gpgv")
class Proven(unittest.TestCase):
    """With gpg itself, in a keyring of the test's own: the key signs here
    and its file alone reads what it signs, or the publication stops."""

    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.folder = pathlib.Path(self.directory.name)
        home = self.folder / "gnupg"
        home.mkdir(mode=0o700)
        self.environment = {**os.environ, "GNUPGHOME": str(home)}
        self.addCleanup(self.directory.cleanup)
        # The agent gpg started for it, and its sockets' folder, gone after.
        self.addCleanup(subprocess.run, ["gpgconf", "--remove-socketdir"], env=self.environment, check=False)
        self.addCleanup(subprocess.run, ["gpgconf", "--kill", "all"], env=self.environment, check=False)
        # repository.py's gpg too.
        patched = unittest.mock.patch.dict(os.environ, {"GNUPGHOME": str(home)})
        patched.start()
        self.addCleanup(patched.stop)
        self.keys = {name: self.key(name) for name in ("signs", "other")}

    def gpg(self, *args, **kwargs):
        return subprocess.run(["gpg", "--batch", "--quiet", *args], env=self.environment, check=True,
                              capture_output=True, **kwargs).stdout

    def key(self, name):
        self.gpg("--passphrase", "", "--quick-generate-key", f"Skwir repositories ({name})", "ed25519", "sign",
                 "never")
        listing = self.gpg("--with-colons", "--list-keys", f"=Skwir repositories ({name})", text=True)
        fingerprint = next(line.split(":")[9] for line in listing.splitlines() if line.startswith("fpr:"))
        public = self.folder / f"{name}.asc"
        public.write_bytes(self.gpg("--armor", "--export", fingerprint))
        return fingerprint, public

    def refused(self, fingerprint, public):
        with self.assertRaises(SystemExit) as refusal:
            repository.proven(fingerprint, public)
        return str(refusal.exception.code)

    def test_a_key_that_signs_and_its_file_read_it(self):
        fingerprint, public = self.keys["signs"]
        repository.proven(fingerprint, public)

    def test_a_file_that_does_not_read_what_the_key_signs_is_refused(self):
        fingerprint, _ = self.keys["signs"]
        _, other = self.keys["other"]
        self.assertIn(f"{other}: does not read what {fingerprint} signs", self.refused(fingerprint, other))

    def test_a_key_gpg_cannot_sign_with_is_refused(self):
        fingerprint, public = self.keys["signs"]
        self.gpg("--yes", "--delete-secret-keys", fingerprint)
        self.assertIn(f"{fingerprint}: gpg cannot sign with it here", self.refused(fingerprint, public))


if __name__ == "__main__":
    unittest.main()
