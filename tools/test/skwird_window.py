#!/usr/bin/env python3
"""A search in the real window through a real skwird engine (ADR-150,
11): the footer heard over AT-SPI as a screen reader reads it, and what
`entries` says. Run by `just skwird-check`.

The window is the a11y harness's (tools/test/a11y.py): an X display from :90
up, a D-Bus session of its own, every XDG folder and HOME under
~/.cache/skwir-tests/a11y-*. Before it, an engine of skwird's is started
there on the fixture alone (`skwird serve --root`), in the window's own five
folders (runtime, cache, state, config, home), where skwir's plugin finds a
user's engine: nothing of the user's is read, no engine started by skwir. A
stand-in plocate on the window's PATH answers `/`.

    skwird_window.py <a11y.py's options> --skwird <skwird> [--shots <folder> --magick <magick>]
"""

import argparse
import json
import os
import pathlib
import shutil
import subprocess
import sys
import time

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import a11y  # noqa: E402
from a11y import Atspi, Window, check, fail, until  # noqa: E402

# When the stand-in plocate's database was last brought up to date.
UPDATED = 1_800_000_007


class Checked(Window):
    def __init__(self, args):
        super().__init__(args)
        self.bin = self.root / "bin"

    def env(self):
        env = super().env()
        env["PATH"] = f"{self.bin}:{env['PATH']}"
        env.pop("LOCATE_PATH", None)
        # The engine stays while the check runs; no client of skwird's
        # (`skwird watch`) starts one of its own.
        env["SKWIRD_IDLE"] = "600"
        env["SKWIRD_NO_START"] = "1"
        return env

    def start(self):
        (self.files / "docs").mkdir(parents=True)
        (self.files / "plain").mkdir()
        (self.files / "extra").mkdir()
        for name in ["report 2026.txt", "docs/q3 report.pdf", "notes.txt"]:
            (self.files / name).write_text("fixture\n")
        self.runtime.mkdir(mode=0o700)
        config = self.root / "config" / "skwir"
        config.mkdir(parents=True)
        (config / "settings.toml").write_text("[plugins.control]\nenabled = true\n")
        # The desktop's folders and a bookmark, which Kept live follows
        # (ADR-150, 6): Documents is the fixture's `docs`, its `plain`
        # bookmarked; the fixture itself is kept live by none, so a search
        # of it is checked until the step that keeps it live.
        (self.root / "config" / "user-dirs.dirs").write_text(
            'XDG_DOCUMENTS_DIR="$HOME/files/docs"\n'
        )
        (self.root / "config" / "gtk-3.0").mkdir(parents=True)
        (self.root / "config" / "gtk-3.0" / "bookmarks").write_text(
            f"file://{self.files / 'plain'} plain\n"
        )
        # plocate's stand-in: its help names its database, dated; asked, it
        # names the fixture's report.
        self.bin.mkdir()
        db = self.root / "plocate.db"
        db.write_text("")
        os.utime(db, (UPDATED, UPDATED))
        plocate = self.bin / "plocate"
        plocate.write_text(
            "#!/bin/sh\n"
            'if [ "$1" = --help ]; then\n'
            f"  echo '  (default is {db})'\n"
            "  exit 0\nfi\n"
            f"printf '%s\\0' '{self.files / 'report 2026.txt'}'\n"
        )
        plocate.chmod(0o755)
        self.display = self.start_display()
        self.start_bus()
        socket = self.runtime / "skwird" / "socket"
        self.spawn([self.args.skwird, "serve", "--root", str(self.files), "--idle", "600"],
                   "skwird.log")
        if not until(socket.exists, 30):
            fail(f"no engine; see {self.root / 'skwird.log'}")
        self.spawn([self.args.skwir, str(self.files)], "skwir.log")
        os.environ["DBUS_SESSION_BUS_ADDRESS"] = self.bus
        os.environ.pop("AT_SPI_BUS_ADDRESS", None)
        Atspi.init()
        if not until(lambda: self.find_app() is not None, 60):
            fail(f"the window never came on the bus; see {self.root / 'skwir.log'}")

    def answer(self, command, args=None):
        """What a command answers, as a script reads it."""
        argv = [self.args.skwir, "cmd", command]
        if args is not None:
            argv.append(json.dumps(args))
        run = subprocess.run(argv, env=self.env(), capture_output=True, text=True, timeout=60)
        if run.returncode != 0:
            fail(f"`skwir cmd {command}`: {run.stderr.strip() or run.stdout.strip()}")
        return json.loads(run.stdout)

    def shot(self, name):
        """The display, where asked (`--shots`)."""
        if not self.args.shots:
            return
        out = pathlib.Path(self.args.shots) / f"{name}.png"
        subprocess.run([self.args.magick, "import", "-window", "root", str(out)],
                       env=self.env(), check=True, timeout=60)
        print(f"  shot: {out}")


def named(w):
    """Every node of the window's and its dialogs': its role and its name."""
    app = w.find_app()
    stack, found = [app], []
    while stack:
        node = stack.pop()
        if node is None:
            continue
        found.append((node.get_role_name(), node.get_name() or ""))
        # What a node is described by, heard after its name.
        if node.get_description():
            found.append(("description", node.get_description()))
        stack.extend(node.get_child_at_index(i) for i in range(node.get_child_count()))
    return found


def says(w, text):
    return any(text in name for _, name in named(w))


def status(w):
    stack, found = [w.frame()], []
    while stack:
        node = stack.pop()
        if node is None:
            continue
        if node.get_role_name() == "status bar":
            found.append(node.get_name())
        stack.extend(node.get_child_at_index(i) for i in range(node.get_child_count()))
    return found


def search(w, expr, root):
    w.cmd("query.search", {"expr": expr, "root": str(root)})


def run(w):
    w.keys()
    files = w.files

    search(w, "report", files)
    check("a search skwird checked says so in the footer, as a screen reader reads it",
          until(lambda: any("index checked" in s for s in status(w)))
          or print("  read:", status(w)))
    print("  read:", status(w))
    answer = w.answer("entries")
    print("  entries' index:", answer.get("index"), "as_of:", answer.get("as_of"))
    found = sorted(e["name"] for e in answer["entries"])
    check("what it found is the fixture's reports", found == ["q3 report.pdf", "report 2026.txt"]
          or print("  found:", found))
    check("entries says checked, by skwird's index",
          (answer.get("index") or {}).get("proof") == "checked"
          and answer["index"].get("by") == "skwird's index")
    w.shot("1-checked")

    # Kept live by a client of skwird's own, as Kept live will (step E).
    watch = subprocess.run([w.args.skwird, "watch", str(files)], env=w.env(),
                           capture_output=True, text=True, timeout=60)
    print("  skwird watch:", watch.stdout.strip() or watch.stderr.strip())
    # Another text, with no term a lens keeps: `type:file` would stay with
    # the pane (ADR-107) and follow it to `/`, which plocate then declines.
    search(w, "report 2026", files)
    check("live, the footer says the index is up to date",
          until(lambda: any("1 result" in s and "index up to date" in s for s in status(w)))
          or print("  read:", status(w)))
    print("  read:", status(w))
    answer = w.answer("entries")
    print("  entries' index:", answer.get("index"))
    check("entries says live", (answer.get("index") or {}).get("proof") == "live")
    w.shot("2-live")

    # `/`: skwird's engine says outside, plocate answers as of its update.
    search(w, "report hidden:yes", "/")
    check("the whole disk, plocate's answer: the index as of its update",
          until(lambda: any("index as of" in s for s in status(w)))
          or print("  read:", status(w)))
    print("  read:", status(w))
    answer = w.answer("entries")
    print("  entries' index:", answer.get("index"), "as_of:", answer.get("as_of"))
    check("entries says as of, by plocate's database, as it always did",
          (answer.get("as_of") or {}).get("updated") == UPDATED
          and (answer.get("index") or {}).get("proof") == "as_of")
    w.shot("3-as-of")

    # The Index page (ADR-150, 6 and 8): Kept live's groups, their
    # switches, its rows ticked and saying what skwird says, the share.
    w.cmd("ui.settings", {"page": "index"})
    check("the Index page shows Kept live's groups, the desktop's and the bookmarks'",
          until(lambda: says(w, "Your folders") and says(w, "Bookmarks"), 30)
          or print("  read:", sorted({n for _, n in named(w) if n})[:80]))
    w.shot("4-index")
    check("a row says its folder is live, as skwird says",
          until(lambda: says(w, "~/files/docs · live"), 30)
          or print("  read:", [n for _, n in named(w) if "·" in n]))
    # Under the list, below the fold, where the dialog's list draws (and
    # exposes) nothing until scrolled to: asked of the list's owner.
    check("the share's use is said under the list",
          until(lambda: "Watches used:" in str(w.answer("skwird.kept_list").get("note")), 30)
          or print("  said:", w.answer("skwird.kept_list").get("note")))
    w.shot("4b-index-note")
    roles = {(role, name) for role, name in named(w)}
    print("  switches and ticks:", sorted(r for r in roles if r[0] in ("check box", "toggle button", "switch")))
    w.cmd("skwird.kept_add", {"path": str(files / "extra")})
    check("a folder added shows under Added by you",
          until(lambda: says(w, "Added by you") and says(w, "~/files/extra"), 30)
          or print("  read:", [n for _, n in named(w) if "extra" in n or "Added" in n]))
    menu = w.answer("skwird.keep_live_menu", {"path": str(files / "docs")})
    print("  the folder's menu:", menu)
    check("a folder's menu offers Keep Live for Searches, ticked while it is",
          [i.get("checked") for i in menu.get("items", [])] == ["on"])
    # The list's end in view: the wheel over the dialog's page.
    if w.args.shots:
        xdotool = w.args.xdotool
        subprocess.run([xdotool, "mousemove", "760", "500"], env=w.env(), check=True)
        subprocess.run([xdotool, "click", "--repeat", "15", "--delay", "40", "5"],
                       env=w.env(), check=True)
        time.sleep(0.5)
    w.shot("5-index-added")


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    for name in ["xvfb", "icd", "atspi", "dbus", "xdotool", "skwird"]:
        parser.add_argument(f"--{name}", required=True)
    parser.add_argument("--skwir", default=str(a11y.REPO / "target/release/skwir"))
    parser.add_argument("--shots", help="a folder for screenshots of each step")
    parser.add_argument("--magick", help="ImageMagick's program, for --shots")
    parser.add_argument("--keep", action="store_true", help="leave the window running")
    args = parser.parse_args()
    w = Checked(args)
    try:
        w.start()
        run(w)
        print("skwird window: passed")
    finally:
        if args.keep:
            groups = " ".join(f"-{p.pid}" for p in w.procs)
            print(f"kept in {w.root} on DISPLAY={w.display}; stop with: kill -- {groups}")
        else:
            w.stop()
            shutil.rmtree(w.root, ignore_errors=True)


if __name__ == "__main__":
    main()
