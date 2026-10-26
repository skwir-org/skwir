"""tools/build/prune.py on a made-up target folder: what is kept and what
goes, never asking Cargo."""
import os
import sys
import tempfile
import time
import unittest
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parent))
import prune  # noqa: E402

KEPT = "0123456789abcdef"
STALE = "fedcba9876543210"
OTHER = "00112233aabbccdd"


class Pruning(unittest.TestCase):
    def setUp(self):
        self.held = tempfile.TemporaryDirectory()
        self.profile = Path(self.held.name) / "debug"
        for folder in ("deps", "build", ".fingerprint", "incremental"):
            (self.profile / folder).mkdir(parents=True)
        patch = mock.patch.object(prune, "PROFILE", self.profile)
        patch.start()
        self.addCleanup(patch.stop)
        self.addCleanup(self.held.cleanup)

    def touch(self, relative, data=b"", executable=False):
        path = self.profile / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(data)
        if executable:
            path.chmod(0o755)
        return path

    def test_a_unit_is_its_hash_wherever_cargo_put_it(self):
        for name, unit in [
            (f"libskwir_core-{KEPT}.rlib", ("skwir_core", KEPT)),
            (f"skwir_core-{KEPT}.skwir_core.ab12-cgu.0.rcgu.dwo", ("skwir_core", KEPT)),
            (f"it-s-a-crate-{KEPT}", ("it-s-a-crate", KEPT)),
        ]:
            match = prune.UNIT.match(name)
            self.assertEqual((match.group(1), match.group(2)), unit, name)
        self.assertIsNone(prune.UNIT.match("skwir"), "a program placed has no hash")

    def test_a_program_placed_is_found_by_its_link(self):
        unit = self.touch(f"deps/skwir_pdf_worker-{KEPT}", b"x", executable=True)
        self.touch(f"deps/skwir_pdf_worker-{STALE}", b"old", executable=True)
        placed = self.profile / "skwir-pdf-worker"
        os.link(unit, placed)
        self.assertEqual(prune.linked_unit(placed), [KEPT])
        # Copied rather than linked: every unit of its name is kept.
        copied = self.profile / "skwir-pdf-worker-copy"
        copied.write_bytes(b"x")
        self.assertEqual(prune.linked_unit(copied), [])
        placed.unlink()
        placed.write_bytes(b"x")
        self.assertEqual(sorted(prune.linked_unit(placed)), sorted([KEPT, STALE]))

    def test_what_no_build_uses_goes_but_what_is_built_apart_stays(self):
        kept = [
            self.touch(f"deps/libskwir_core-{KEPT}.rlib"),
            self.touch(f"deps/skwir_core-{KEPT}.skwir_core.a-cgu.0.rcgu.dwo"),
            self.touch(f".fingerprint/skwir-core-{KEPT}/lib-skwir_core"),
            self.touch(f"build/serde-{KEPT}/out/x"),
            # A crate only the native workers use, built apart.
            self.touch(f"deps/libpoppler-{STALE}.rlib"),
            self.touch("deps/no-hash-here"),
        ]
        stale = [
            self.touch(f"deps/libskwir_core-{STALE}.rlib"),
            self.touch(f"deps/skwir_core-{STALE}.skwir_core.b-cgu.0.rcgu.dwo"),
            self.touch(f"deps/skwir_core-{STALE}.d"),
        ]
        stale_folders = [self.profile / f".fingerprint/skwir-core-{STALE}", self.profile / f"build/serde-{STALE}"]
        for folder in stale_folders:
            folder.mkdir(parents=True)
        going = set(prune.stale_units({KEPT}, {"poppler"}))
        self.assertEqual(going, set(stale) | set(stale_folders))
        self.assertTrue(going.isdisjoint(kept))

    def test_debug_info_no_artifact_names_goes(self):
        named = f"skwir_core-{KEPT}.skwir_core.a-cgu.0.rcgu.dwo"
        dead = f"skwir_core-{KEPT}.skwir_core.z-cgu.0.rcgu.dwo"
        of_a_unit_going = f"skwir_core-{STALE}.skwir_core.y-cgu.0.rcgu.dwo"
        self.touch(f"deps/libskwir_core-{KEPT}.rlib", b"... " + named.encode() + b" ...")
        # An artifact going names nothing that stays.
        going = self.touch(f"deps/libskwir_core-{STALE}.rlib", b"... " + dead.encode() + b" ...")
        for name in (named, dead, of_a_unit_going):
            self.touch(f"deps/{name}")
        with mock.patch.object(prune.shutil, "which", return_value=None):
            found = prune.dead_dwo([going, self.profile / "deps" / of_a_unit_going])
        self.assertEqual([p.name for p in found], [dead])

    def test_old_sessions_and_old_units_go_but_one_being_written_stays(self):
        now = time.time()

        def session(unit, stamp, age_days):
            path = self.profile / "incremental" / unit / f"s-{stamp}-abc-def"
            path.mkdir(parents=True)
            (path.parent / f"s-{stamp}-abc.lock").write_bytes(b"")
            os.utime(path, (now - age_days * 86400, now - age_days * 86400))
            return path

        old = session("skwir_core-1a", "hnk1", 0)
        newest = session("skwir_core-1a", "hnk2", 0)
        stale_unit = session("skwir_ui-2b", "hnk1", prune.STALE_SESSION_DAYS + 1)
        written = session("skwir_platform-3c", "hnk1", 30)
        (written.parent / "s-hnk9-xyz-working").mkdir()
        going = set(prune.stale_incremental(now))
        self.assertIn(old, going)
        self.assertIn(old.parent / "s-hnk1-abc.lock", going)
        self.assertNotIn(newest, going)
        self.assertIn(stale_unit.parent, going)
        self.assertFalse(any(written.parent in (p, p.parent) for p in going), "being written")

    def test_a_size_reads_as_people_write_it(self):
        self.assertEqual(prune.size_of("40G"), 40 * 2**30)
        self.assertEqual(prune.size_of("512MiB"), 512 * 2**20)
        with self.assertRaises(Exception):
            prune.size_of("lots")


if __name__ == "__main__":
    unittest.main()
