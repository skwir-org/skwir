"""tools/build/startup_order.py's arithmetic: which functions a trace ran,
and in which order the list puts them; and the X display it takes."""
import os
import sys
import tempfile
import types
import unittest
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parent))
import startup_order  # noqa: E402


class WhatRan(unittest.TestCase):
    SYMBOLS = [(1000, 64, "a"), (1064, 8, "tiny"), (1072, 120, "b"), (4096, 16, "far")]

    def test_a_block_names_every_function_it_overlaps(self):
        first = startup_order.first_runs(self.SYMBOLS, [(1056, 1.5)])
        self.assertEqual(first, {"a": 1.5, "tiny": 1.5, "b": 1.5},
                         "a small function may start in the middle of a block")

    def test_the_earliest_is_kept(self):
        first = startup_order.first_runs(self.SYMBOLS, [(1152, 3.0), (1088, 2.0)])
        self.assertEqual(first, {"b": 2.0})

    def test_a_block_before_any_function_names_none(self):
        self.assertEqual(startup_order.first_runs(self.SYMBOLS, [(100, 1.0)]), {})

    def test_a_function_of_no_size_counts_as_a_byte(self):
        first = startup_order.first_runs([(2048, 0, "zero")], [(2048, 1.0)])
        self.assertEqual(first, {"zero": 1.0})


class TheList(unittest.TestCase):
    def test_what_every_display_runs_comes_first(self):
        wayland = {"shared-late": 5.0, "shared-early": 1.0, "wl": 2.0}
        x11 = {"shared-late": 4.0, "shared-early": 3.0, "x": 0.5}
        self.assertEqual(startup_order.laid_out([{"wayland": wayland, "x11": x11}]),
                         ["shared-early", "shared-late", "wl", "x"])

    def test_a_later_program_follows_with_what_it_runs_beside(self):
        manager = {"wayland": {"shared": 1.0, "folder": 2.0, "wl": 3.0},
                   "x11": {"shared": 1.5, "folder": 2.5}}
        viewer = {"wayland": {"shared": 0.5, "text": 1.0, "wl": 0.2, "png-wl": 4.0},
                  "x11": {"shared": 0.5, "text": 2.0}}
        self.assertEqual(startup_order.laid_out([manager, viewer]),
                         ["shared", "folder", "wl", "text", "png-wl"],
                         "the first program as if alone, then the later one's own")
        self.assertEqual(startup_order.laid_out([manager]), ["shared", "folder", "wl"])

    def test_merged_keeps_the_earliest(self):
        self.assertEqual(startup_order.merged([{"a": 2.0}, {"a": 1.0, "b": 3.0}]),
                         {"a": 1.0, "b": 3.0})

    def test_equal_times_order_by_name(self):
        self.assertEqual(startup_order.laid_out([{"one": {"b": 1.0, "a": 1.0}}]), ["a", "b"])

    def test_a_picture_is_a_png(self):
        made = startup_order.picture(4, 3)
        self.assertTrue(made.startswith(b"\x89PNG\r\n\x1a\n"))
        self.assertIn(b"IHDR", made)
        self.assertTrue(made.endswith(b"IEND\xaeB`\x82"))


class TheDisplay(unittest.TestCase):
    """`start_x11` takes a display no other server holds, with a stand-in
    Xvfb and folders of its own for the sockets and locks."""

    # As Xvfb: a display whose lock is there is refused at once; else its
    # lock names it and its socket comes. :<first+1> is another's, which
    # makes its socket and lock as this Xvfb ends.
    XVFB = """#!/bin/sh
number="${1#:}"
lock="$LOCKS/.X$number-lock"
[ -e "$lock" ] && exit 1
if [ "$number" = "$OTHERS" ]; then
    echo 1 > "$lock"; : > "$SOCKETS/X$number"; exit 1
fi
echo "$$" > "$lock"; : > "$SOCKETS/X$number"
exec sleep 30
"""

    def test_a_display_held_or_taken_meanwhile_is_passed_over(self):
        with tempfile.TemporaryDirectory() as folder:
            folder = Path(folder)
            sockets, locks = folder / "sockets", folder / "locks"
            sockets.mkdir()
            locks.mkdir()
            xvfb = folder / "Xvfb"
            xvfb.write_text(self.XVFB)
            xvfb.chmod(0o755)
            first = 910 + os.getpid() % 90
            at = [910 + (first - 910 + n) % 90 for n in range(4)]
            # Held: a socket alone, a lock alone.
            (sockets / f"X{at[0]}").touch()
            (locks / f".X{at[1]}-lock").write_text("1\n")
            env = {"SOCKETS": str(sockets), "LOCKS": str(locks), "OTHERS": str(at[2])}
            args = types.SimpleNamespace(xvfb=str(xvfb), keep=False)
            with mock.patch.dict(os.environ, env), \
                    mock.patch.object(startup_order, "X11_SOCKETS", sockets), \
                    mock.patch.object(startup_order, "X11_LOCKS", locks):
                session = startup_order.Session(args)
                try:
                    self.assertEqual(session.start_x11(), {"DISPLAY": f":{at[3]}"},
                                     "not one held, nor one another took as it started")
                    self.assertEqual(len(session.procs), 1, "the Xvfb that ended is let go of")
                finally:
                    session.close()


if __name__ == "__main__":
    unittest.main()
