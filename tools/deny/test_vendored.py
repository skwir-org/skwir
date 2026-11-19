"""tools/deny/vendored.py: which crates it holds to the advisories, and the
manifest it checks them in."""
import pathlib
import tempfile
import tomllib
import unittest

import vendored


class Vendored(unittest.TestCase):
    def test_every_crate_taken_by_path_and_only_those(self):
        with tempfile.TemporaryDirectory() as folder:
            root = pathlib.Path(folder)
            (root / "Cargo.toml").write_text(
                "[patch.crates-io]\n"
                'kept = { path = "vendor/kept" }\n'
                'tagged = { path = "vendor/tagged" }\n'
                'forked = { git = "https://example.invalid/forked", rev = "0" }\n'
            )
            for name, version in [("kept", "0.3.2"), ("tagged", "0.5.2+bat-0.26.1")]:
                (root / "vendor" / name).mkdir(parents=True)
                (root / "vendor" / name / "Cargo.toml").write_text(
                    f'[package]\nname = "{name}"\nversion = "{version}"\n'
                )
            self.assertEqual(
                vendored.vendored(root), [("kept", "0.3.2"), ("tagged", "0.5.2")]
            )

    def test_the_repository_s_own(self):
        crates = dict(vendored.vendored())
        self.assertIn("jpeg-decoder", crates)
        self.assertNotIn("+", "".join(crates.values()))

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
