"""tools/deny/vendored.py: which crates it holds to the advisories, and the
manifest it checks them in."""
import pathlib
import tempfile
import tomllib
import unittest

import vendored


class Vendored(unittest.TestCase):
    def test_every_crate_patched_by_path_or_from_git(self):
        with tempfile.TemporaryDirectory() as folder:
            root = pathlib.Path(folder)
            (root / "Cargo.toml").write_text(
                "[patch.crates-io]\n"
                'kept = { path = "vendor/kept" }\n'
                'tagged = { path = "vendor/tagged" }\n'
                'forked = { git = "https://example.invalid/forked", rev = "0" }\n'
                'renamed = { git = "https://example.invalid/other.git", '
                'package = "other" }\n'
                'unused = { git = "https://example.invalid/unused", rev = "0" }\n'
            )
            for name, version in [("kept", "0.3.2"), ("tagged", "0.5.2+bat-0.26.1")]:
                (root / "vendor" / name).mkdir(parents=True)
                (root / "vendor" / name / "Cargo.toml").write_text(
                    f'[package]\nname = "{name}"\nversion = "{version}"\n'
                )
            # The fork as locked from its repository; the release of the
            # same name from the registry, and a crate gpui takes from git
            # with no patch, are not patches.
            (root / "Cargo.lock").write_text(
                "version = 4\n"
                '[[package]]\nname = "forked"\nversion = "4.7.1"\n'
                'source = "git+https://example.invalid/forked?rev=0#0123"\n'
                '[[package]]\nname = "forked"\nversion = "4.6.0"\n'
                'source = "registry+https://github.com/rust-lang/crates.io-index"\n'
                '[[package]]\nname = "other"\nversion = "1.2.0"\n'
                'source = "git+https://example.invalid/other.git#4567"\n'
                '[[package]]\nname = "pinned"\nversion = "30.0.0"\n'
                'source = "git+https://example.invalid/pinned?rev=1#89ab"\n'
            )
            self.assertEqual(
                vendored.vendored(root),
                [
                    ("forked", "4.7.1"),
                    ("kept", "0.3.2"),
                    ("other", "1.2.0"),
                    ("tagged", "0.5.2"),
                ],
            )

    def test_the_repository_s_own(self):
        crates = dict(vendored.vendored())
        self.assertIn("jpeg-decoder", crates)
        self.assertIn("oo7", crates)
        self.assertNotIn("+", "".join(crates.values()))
        # Copies of crates published nowhere stand for no release.
        self.assertNotIn("skwird-client", crates)
        self.assertNotIn("skwird-protocol", crates)

    def test_the_manifest_names_each_release_exactly(self):
        text = vendored.manifest([("jpeg-decoder", "0.3.2"), ("two-face", "0.5.2")])
        dependencies = tomllib.loads(text)["dependencies"]
        self.assertEqual(
            dependencies["jpeg-decoder"], {"version": "=0.3.2", "default-features": False}
        )
        self.assertEqual(dependencies["two-face"]["version"], "=0.5.2")
        self.assertIn("workspace", tomllib.loads(text))


if __name__ == "__main__":
    unittest.main()
