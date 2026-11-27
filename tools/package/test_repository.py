"""What tools/package/repository.py (ADR-142) refuses before it signs: a
bundle whose packages are not the release's, a key that cannot sign or will
not be valid long enough, skwird's packages that are not those its lock
pins. The repositories themselves are tried by repository-check.sh."""

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
import types
import unittest
import unittest.mock

HERE = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import repository  # noqa: E402
import skwird  # noqa: E402

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

    def test_a_key_signs_until_its_last_part_that_signs_expires(self):
        def key(caps, *subkeys):
            lines = [f"pub:-:255:22:{OLD[-16:]}:1795118515:::-:::{caps}:::::ed25519:::0:", f"fpr:::::::::{OLD}:"]
            for i, (validity, expires, sub_caps) in enumerate(subkeys):
                lines += [f"sub:{validity}:255:22:{i:016X}:1795118515:{expires or ''}:::::{sub_caps}:::::ed25519::",
                          f"fpr:::::::::{i:040X}:"]
            return "\n".join(lines) + "\n"
        # A primary that only certifies: its subkeys that sign, the last of them.
        self.assertEqual(repository.keys(key("cSC", ("-", 1900000000, "s"))), [(OLD, 1900000000)])
        self.assertEqual(repository.keys(key("cSC", ("-", 1900000000, "s"), ("-", 1950000000, "s"))),
                         [(OLD, 1950000000)])
        # One expired or revoked never signs; one that encrypts is not asked.
        self.assertEqual(repository.keys(key("cSC", ("-", 1900000000, "s"), ("r", None, "s"), ("-", None, "e"))),
                         [(OLD, 1900000000)])
        # A primary that signs goes on when its subkeys expire.
        self.assertEqual(repository.keys(key("scSC", ("-", 1900000000, "s"))), [(OLD, None)])

    def test_keys_retired_are_read_by_their_fingerprints_never_one_carried(self):
        with tempfile.TemporaryDirectory() as folder:
            listed = pathlib.Path(folder) / "retired"
            self.assertEqual(repository.retired_keys(listed, [NEW]), [])
            listed.write_text(f"# changed for {NEW}\n{OLD.lower()}  # the first\n\n")
            self.assertEqual(repository.retired_keys(listed, [NEW]), [OLD])
            for wrong, said in ((f"{OLD[:-1]}\n", "not a key's fingerprint"), (f"{NEW}\n", "skwir-repository carries it")):
                listed.write_text(wrong)
                with self.assertRaises(SystemExit) as refusal:
                    repository.retired_keys(listed, [NEW])
                self.assertIn(said, str(refusal.exception.code))

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
            self.assertIn(" 5y, and its subkeys that sign with '*'), export it again, raise"
                          " tools/package/repository/version", said[0])
        self.assertEqual(repository.unfit((OLD, later), (NEW, later), True, NOW), [])


DEB, UBUNTU, RPM = "skwird_1.0.0-1_amd64.deb", "skwird_1.0.0-1ubuntu24.04_amd64.deb", "skwird-1.0.0-1.x86_64.rpm"


def skwird_lock(files, unsigned=()):
    """What skwird.lock pins of a release (`files`: name and body), as
    skwird.py reads it."""
    return types.SimpleNamespace(tag="v1.0.0", unsigned=set(unsigned),
                                 files={name: hashlib.sha256(body).hexdigest() for name, body in files.items()})


class Skwird(unittest.TestCase):
    """skwird's packages listed beside skwir's (ADR-142, amended 2026-11-26)."""

    def listed(self, names, unsigned=()):
        return repository.skwird_listed(skwird_lock(dict.fromkeys(names, b""), unsigned))

    def test_each_suite_lists_its_own_skwird(self):
        listed, unlisted = self.listed([DEB, UBUNTU, RPM, "skwird-1.0.0-windows-x86_64.zip"])
        self.assertEqual(listed, {"debian": [DEB], "ubuntu": [UBUNTU], "fedora": [RPM]})
        self.assertEqual(unlisted, [])

    def test_an_rpm_marked_unsigned_is_pinned_and_listed_nowhere(self):
        listed, unlisted = self.listed([DEB, UBUNTU, RPM], unsigned=[RPM])
        self.assertEqual(listed, {"debian": [DEB], "ubuntu": [UBUNTU], "fedora": []})
        self.assertEqual(unlisted, [RPM])

    def test_a_lock_the_suites_cannot_list_is_refused(self):
        for names, unsigned, said in (([DEB, UBUNTU, RPM], [DEB], f"{DEB}: marked unsigned, which only an `.rpm` is"),
                                      ([DEB, RPM], [], "no package of skwird for Ubuntu 24.04"),
                                      ([UBUNTU, DEB], [], "no package of skwird for Fedora"),
                                      ([DEB, UBUNTU, RPM, "skwird_1.0.0-1_arm64.deb"], [],
                                       "skwird_1.0.0-1_arm64.deb: a package no suite")):
            with self.subTest(names=names, unsigned=unsigned), self.assertRaises(SystemExit) as refusal:
                self.listed(names, unsigned)
            self.assertIn(said, str(refusal.exception.code))

    def test_the_lock_pins_a_package_of_skwird_for_each_suite(self):
        listed, unlisted = repository.skwird_listed(skwird.Lock(repository.SKWIRD_LOCK))
        self.assertTrue(listed["debian"] and listed["ubuntu"])
        self.assertTrue(listed["fedora"] or unlisted)

    def fetch(self, kept, served, folder):
        """skwird's Debian package, `kept` in `folder` beforehand (or not),
        fetched where needed from a release serving `served`; the
        addresses asked."""
        asked = []

        def opener(url, timeout):
            asked.append(url)
            return io.BytesIO(served)
        if kept is not None:
            (folder / DEB).write_bytes(kept)
        paths = repository.skwird_files(skwird_lock({DEB: b"deb"}), [DEB], folder, opener)
        return paths, asked

    def test_a_package_kept_with_its_sum_is_not_fetched_again(self):
        with tempfile.TemporaryDirectory() as folder:
            paths, asked = self.fetch(b"deb", b"never", pathlib.Path(folder))
            self.assertEqual((paths, asked), ({DEB: pathlib.Path(folder) / DEB}, []))

    def test_a_package_missing_or_changed_is_fetched_from_its_release(self):
        for kept in (None, b"changed"):
            with self.subTest(kept=kept), tempfile.TemporaryDirectory() as folder:
                paths, asked = self.fetch(kept, b"deb", pathlib.Path(folder))
                self.assertEqual(asked, [f"https://github.com/skwir-org/skwird/releases/download/v1.0.0/{DEB}"])
                self.assertEqual(paths[DEB].read_bytes(), b"deb")

    def test_a_package_fetched_whose_sum_is_not_the_lock_s_is_refused_and_kept_nowhere(self):
        with tempfile.TemporaryDirectory() as folder:
            with self.assertRaises(SystemExit) as refusal:
                self.fetch(None, b"another", pathlib.Path(folder))
            self.assertIn(f"{DEB}: not the file skwird.lock pins", str(refusal.exception.code))
            self.assertEqual(list(pathlib.Path(folder).iterdir()), [])


# rpm's readings of a package (`rpmkeys --checksig --verbose`), the key
# that signs imported or not: rpm 4.20 (the flake's), then rpm 6.1.
SIGNED_4 = """skwird-0.3.0-1.x86_64.rpm:
    Header V4 EdDSA/SHA512 Signature, key ID 48004329: OK
    Header SHA256 digest: OK
    Header SHA1 digest: OK
    Payload SHA256 digest: OK
"""
UNSIGNED_4 = """skwird-0.2.0-1.x86_64.rpm:
    Header SHA256 digest: OK
    Header SHA1 digest: OK
    Payload SHA256 digest: OK
"""
SIGNED_6 = """skwird-0.3.0-1.x86_64.rpm:
    Header OpenPGP V4 EdDSA/SHA512 signature, key fingerprint: f6eb0dc17b1a3e878c482db30cf3a0ea48004329: OK
    Header SHA256 digest: OK
    Payload SHA256 digest: OK
"""
UNSIGNED_6 = """skwird-0.2.0-1.x86_64.rpm:
    Header OpenPGP DSA signature: NOTFOUND
    Header OpenPGP RSA signature: NOTFOUND
    Header OpenPGP signature: NOTFOUND
    Header SHA256 digest: OK
    Payload SHA256 digest: OK
    Legacy OpenPGP DSA signature: NOTFOUND
    Legacy OpenPGP RSA signature: NOTFOUND
"""


class Signatures(unittest.TestCase):
    """What rpm says of a package, read as dnf would take it: signed by the
    key that signs, or why not, whichever rpm read it."""

    def test_signed_by_the_key_is_taken_as_rpm_4_and_6_say_it(self):
        for said in (SIGNED_4, SIGNED_6):
            self.assertIsNone(repository.refusal(said, 0), said)

    def test_no_signature_is_refused_as_rpm_4_and_6_say_it(self):
        for said, code in ((UNSIGNED_4, 0), (UNSIGNED_6, 1)):
            self.assertEqual(repository.refusal(said, code), "unsigned", said)

    def test_another_key_is_named_as_rpm_4_and_6_say_it(self):
        other_4 = SIGNED_4.replace("48004329: OK", "48004329: NOKEY")
        other_6 = SIGNED_6.replace("key fingerprint: f6eb0dc17b1a3e878c482db30cf3a0ea48004329: OK",
                                   "key ID 0cf3a0ea48004329: NOKEY")
        self.assertEqual(repository.refusal(other_4, 1), "signed by key ID 48004329")
        self.assertEqual(repository.refusal(other_6, 1), "signed by key ID 0cf3a0ea48004329")

    def test_a_digest_that_fails_is_refused_whatever_rpm_says_after_it(self):
        bad = SIGNED_4.replace("Payload SHA256 digest: OK", "Payload SHA256 digest: BAD (Expected 1a != 2b)")
        self.assertEqual(repository.refusal(bad, 1), "a digest fails")
        self.assertEqual(repository.refusal(SIGNED_6, 1), "a digest fails")


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
