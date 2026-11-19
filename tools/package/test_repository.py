"""What tools/package/repository.py (ADR-142) refuses before it signs: a
bundle whose packages are not the release's, a key that cannot sign or will
not be valid long enough. The repositories themselves are tried by
repository-check.sh."""

import datetime
import hashlib
import json
import pathlib
import sys
import tempfile
import unittest

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


if __name__ == "__main__":
    unittest.main()
