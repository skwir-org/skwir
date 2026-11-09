"""tools/build/startup_order.py's arithmetic: which functions a trace ran,
and in which order the list puts them."""
import sys
import unittest
from pathlib import Path

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
        self.assertEqual(startup_order.laid_out({"wayland": wayland, "x11": x11}),
                         ["shared-early", "shared-late", "wl", "x"])

    def test_merged_keeps_the_earliest(self):
        self.assertEqual(startup_order.merged([{"a": 2.0}, {"a": 1.0, "b": 3.0}]),
                         {"a": 1.0, "b": 3.0})

    def test_equal_times_order_by_name(self):
        self.assertEqual(startup_order.laid_out({"one": {"b": 1.0, "a": 1.0}}), ["a", "b"])


if __name__ == "__main__":
    unittest.main()
