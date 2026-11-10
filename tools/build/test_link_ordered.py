"""tools/build/link-ordered with a linker and nm stood in for: what it links
twice, in which order, and what it leaves when it cannot."""
import os
import stat
import subprocess
import tempfile
import unittest
from pathlib import Path

WRAPPER = Path(__file__).resolve().parent / "link-ordered"

# The symbols of the program being linked: nm's two views of them.
MANGLED = """\
0000000000001000 T _RNvCsAAA_3app5first
0000000000002000 t _RNvCsAAA_3app6second
0000000000003000 W _RINvCsBBB_3lib7genericE
0000000000004000 D _RNvCsAAA_3app4DATA
0000000000005000 t _RNvCsAAA_3app6second.llvm.1
"""
DEMANGLED = """\
0000000000001000 T app::first
0000000000002000 t app::second
0000000000003000 W lib::generic::<u8, alloc::string::String>
0000000000004000 D app::DATA
0000000000005000 t app::second
"""


def script(path: Path, text: str):
    path.write_text(f"#!/bin/sh\n{text}")
    path.chmod(path.stat().st_mode | stat.S_IEXEC)


class Linking(unittest.TestCase):
    def setUp(self):
        held = tempfile.TemporaryDirectory()
        self.addCleanup(held.cleanup)
        self.root = Path(held.name)
        self.calls = self.root / "calls"
        self.deps = self.root / "deps"
        self.deps.mkdir()
        self.list = self.root / "order.txt"
        self.list.write_text("# made by hand\nlib::generic::<u8, alloc::string::String>\n"
                             "gone::since\napp::second\napp::DATA\n")
        (self.root / "mangled").write_text(MANGLED)
        (self.root / "demangled").write_text(DEMANGLED)
        script(self.root / "nm", f"""
case " $* " in
*" -C "*) cat {self.root}/demangled ;;
*) cat {self.root}/mangled ;;
esac
""")
        self.linker(fail_on="")

    def linker(self, fail_on):
        """A linker that records its arguments, copies any ordering file it
        is given, writes its output, and fails given `fail_on`."""
        script(self.root / "cc", f"""
echo "$*" >> {self.calls}
for a in "$@"; do
    case $a in
    -Wl,--symbol-ordering-file=*) cp "${{a#-Wl,--symbol-ordering-file=}}" {self.root}/order-given ;;
    esac
done
out=
while [ $# -gt 0 ]; do [ "$1" = -o ] && out=$2; shift; done
echo "linked by call $(wc -l < {self.calls})" > "$out"
if [ -n "{fail_on}" ] && grep -q "{fail_on}" {self.calls}; then exit 1; fi
""")

    def link(self, *args, name="skwir-0123456789abcdef", env=None):
        out = self.deps / name
        environment = dict(os.environ, SKWIR_REAL_LINKER=str(self.root / "cc"),
                           NM=str(self.root / "nm"), SKWIR_STARTUP_ORDER=str(self.list),
                           TMPDIR=str(self.root))
        environment.update(env or {})
        done = subprocess.run([str(WRAPPER), "-fuse-ld=lld", *args, "-o", str(out), "a.o"],
                              env=environment, capture_output=True, text=True, check=False)
        return done, out

    def calls_made(self):
        return self.calls.read_text().splitlines()

    def test_skwir_is_linked_again_in_the_order_of_the_list(self):
        done, out = self.link()
        self.assertEqual(done.returncode, 0, done.stderr)
        calls = self.calls_made()
        self.assertEqual(len(calls), 2)
        self.assertNotIn("symbol-ordering", calls[0])
        self.assertIn("-Wl,--no-warn-symbol-ordering", calls[1])
        self.assertEqual((self.root / "order-given").read_text().splitlines(), [
            "_RINvCsBBB_3lib7genericE",
            "_RNvCsAAA_3app6second",
            "_RNvCsAAA_3app6second.llvm.1",
        ], "functions alone, every symbol of a name, what is gone left out")
        self.assertEqual(out.read_text(), "linked by call 2\n")
        self.assertEqual(done.stderr, "", "rustc shows what a linker says as a warning")
        self.assertEqual([p.name for p in self.root.glob("skwir-order.*")], [])

    def test_a_name_an_older_nm_leaves_mangled_is_read_without_its_suffix(self):
        """Ubuntu 22.04's nm, where the AppImage is made, leaves a Rust name
        mangled when LLVM gave it a suffix; c++filt reads it without."""
        line = "0000000000006000 t _RNvCsAAA_3app5third.llvm.7\n"
        (self.root / "mangled").write_text(MANGLED + line)
        (self.root / "demangled").write_text(DEMANGLED + line)
        script(self.root / "c++filt", """
[ "$1" = -i ] || exit 2
sed 's/^_RNvCsAAA_3app5third$/app::third/'
""")
        self.list.write_text("app::third\napp::second\n")
        done, _ = self.link(env={"CXXFILT": str(self.root / "c++filt")})
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertEqual((self.root / "order-given").read_text().splitlines(), [
            "_RNvCsAAA_3app5third.llvm.7",
            "_RNvCsAAA_3app6second",
            "_RNvCsAAA_3app6second.llvm.1",
        ])
        # Without c++filt, the rest is in order still, and nothing is said.
        done, _ = self.link(env={"CXXFILT": str(self.root / "no-c++filt")})
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertEqual(done.stderr, "")
        self.assertEqual((self.root / "order-given").read_text().splitlines(), [
            "_RNvCsAAA_3app6second",
            "_RNvCsAAA_3app6second.llvm.1",
        ])

    def test_anything_else_is_the_real_linkers_alone(self):
        for name in ("skwir-bench-0123456789abcdef", "skwir-plugin-remote-0123", "libfoo.so",
                     "build_script_build-0123456789abcdef", "skwir-"):
            self.calls.unlink(missing_ok=True)
            done, out = self.link(name=name)
            self.assertEqual(done.returncode, 0, done.stderr)
            self.assertEqual(len(self.calls_made()), 1, name)
            self.assertEqual(out.read_text(), "linked by call 1\n", name)

    def test_a_failed_first_link_fails(self):
        self.linker(fail_on="a.o")
        done, _ = self.link()
        self.assertEqual(done.returncode, 1)
        self.assertEqual(len(self.calls_made()), 1)

    def test_a_failed_second_link_leaves_the_first(self):
        self.linker(fail_on="symbol-ordering")
        done, out = self.link()
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertEqual(out.read_text(), "linked by call 1\n")
        self.assertIn("second link failed", done.stderr)

    def test_no_list_or_no_lld_or_no_symbols_links_as_it_comes(self):
        cases = [("no list", {"SKWIR_STARTUP_ORDER": str(self.root / "none")}, ()),
                 ("no lld", {}, ("-fuse-ld=mold",)),
                 ("no nm", {"NM": str(self.root / "no-nm")}, ())]
        for what, env, args in cases:
            self.calls.unlink(missing_ok=True)
            if what == "no lld":
                out = self.deps / "skwir-0123456789abcdef"
                done = subprocess.run(
                    [str(WRAPPER), *args, "-o", str(out), "a.o"],
                    env=dict(os.environ, SKWIR_REAL_LINKER=str(self.root / "cc"),
                             NM=str(self.root / "nm"), SKWIR_STARTUP_ORDER=str(self.list),
                             TMPDIR=str(self.root)),
                    capture_output=True, text=True, check=False)
            else:
                done, out = self.link(env=env)
            self.assertEqual(done.returncode, 0, what)
            self.assertEqual(len(self.calls_made()), 1, what)
            self.assertEqual(out.read_text(), "linked by call 1\n", what)
            self.assertIn("linked as it comes", done.stderr, what)

    def test_a_list_naming_nothing_links_as_it_comes(self):
        self.list.write_text("nothing::here\n")
        done, _ = self.link()
        self.assertEqual(done.returncode, 0)
        self.assertEqual(len(self.calls_made()), 1)
        self.assertIn("names nothing", done.stderr)

    def test_arguments_in_a_response_file_are_read(self):
        out = self.deps / "skwir-0123456789abcdef"
        response = self.root / "args"
        response.write_text(f"-fuse-ld=lld\n-o\n{out}\na.o\n")
        script(self.root / "cc", f"""
echo "$*" >> {self.calls}
for a in "$@"; do
    case $a in
    -Wl,--symbol-ordering-file=*) cp "${{a#-Wl,--symbol-ordering-file=}}" {self.root}/order-given ;;
    esac
done
echo linked > {out}
""")
        done = subprocess.run([str(WRAPPER), f"@{response}"],
                              env=dict(os.environ, SKWIR_REAL_LINKER=str(self.root / "cc"),
                                       NM=str(self.root / "nm"),
                                       SKWIR_STARTUP_ORDER=str(self.list),
                                       TMPDIR=str(self.root)),
                              capture_output=True, text=True, check=False)
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertEqual(len(self.calls_made()), 2, done.stderr)
        self.assertTrue((self.root / "order-given").exists())


if __name__ == "__main__":
    unittest.main()
