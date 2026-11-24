#!/usr/bin/env python3
"""A search in the real window through a real skwird engine (ADR-150,
11): the footer heard over AT-SPI as a screen reader reads it, and what
`entries` says; the whole disk shown as typed where plocate answers it
(13); Usage measured from the engine's folder totals as its own scan
measures it, and both timed (16); the two categories of what indexes
leave out on the Exclusions page (ADR-125, 15). Run by `just skwird-check`.

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
        # A tree for Usage to measure: 250 folders of 200 files.
        for folder in range(250):
            where = self.files / "tree" / f"part {folder:03}"
            where.mkdir(parents=True)
            for n in range(200):
                (where / f"f{n:03}.dat").write_bytes(b"x" * (n * 37 % 5000))
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

        # Until its root is read: a root being read is not covered
        # ("unread"), and the first search would walk it.
        def read():
            said = subprocess.run([self.args.skwird, "coverage", str(self.files)],
                                  env=self.env(), capture_output=True, text=True, timeout=60)
            return said.returncode == 0 and "unread" not in said.stdout.lower()
        if not until(read, 60):
            fail(f"the engine never read its root; see {self.root / 'skwird.log'}")
        self.spawn([self.args.skwir, str(self.files)], "skwir.log")
        os.environ["DBUS_SESSION_BUS_ADDRESS"] = self.bus
        os.environ.pop("AT_SPI_BUS_ADDRESS", None)
        Atspi.init()
        if not until(lambda: self.find_app() is not None, 60):
            fail(f"the window never came on the bus; see {self.root / 'skwir.log'}")
        # A `skwir cmd` before the window listens runs headless, doing
        # nothing in it: a debug build comes on the bus before it listens.
        if not until((self.runtime / "skwir" / "control.sock").exists, 60):
            fail(f"the window never listened for scripts; see {self.root / 'skwir.log'}")

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

    # The whole disk as typed (ADR-150, 13): plocate answers `/` at once,
    # so the address field's first row shows its search as typed.
    offered = w.answer("ui.suggest", {"text": "report", "scope": "/"})
    first = (offered.get("rows") or [{}])[0]
    check("the address field shows a search of / as typed where plocate answers it",
          first.get("live") is not None or print("  first row:", first))
    offered = w.answer("ui.suggest", {"text": "content:report", "scope": "/"})
    first = (offered.get("rows") or [{}])[0]
    check("one that reads the files still waits for Enter",
          first.get("live") is None or print("  first row:", first))

    # Usage from the engine's folder totals (ADR-150, 16), the fixture kept
    # live above, against its own scan of the disk with skwird turned off.
    w.cmd("navigate", {"location": str(files)})
    w.cmd("ui.view", {"id": "usage"})
    w.cmd("usage.scan", {"path": str(files)})

    def measured(accept=lambda s: True):
        found = []

        def done():
            s = w.answer("usage.status")
            if not s.get("scanning") and s.get("tree") and accept(s):
                found.append(s)
                return True
            return False
        until(done, 60)
        return found[0] if found else None

    indexed = measured()
    check("Usage measured the fixture", indexed is not None)
    w.shot("3b-usage-indexed")
    w.cmd("plugins.set_enabled", {"id": "skwird", "enabled": False})
    w.cmd("usage.scan", {"path": str(files), "force": True})
    # The scan's own answer, not the index's still shown.
    scanned = measured(lambda s: s.get("scanned_dirs", 0) > 0)
    check("Usage measured the fixture again, by its own scan", scanned is not None)
    w.cmd("plugins.set_enabled", {"id": "skwird", "enabled": True})

    def facts(node):
        rows = [(node["name"], node["size"], node["alloc"], node["files"], node["dirs"],
                 node["modified"])]
        rows += sorted((c["name"], c["size"], c["alloc"], c["files"], c["dirs"], c["modified"])
                       for c in node.get("children", []))
        return rows

    print(f"  from the index: {indexed['elapsed_ms']} ms, {indexed['scanned_dirs']} folders read;"
          f" its own scan: {scanned['elapsed_ms']} ms, {scanned['scanned_dirs']} folders read")
    check("the index's figures are the scan's, folder by folder",
          facts(indexed["tree"]) == facts(scanned["tree"])
          or print("  index:", facts(indexed["tree"])[:6], "\n  scan:", facts(scanned["tree"])[:6]))
    check("from the index, no folder was read", indexed["scanned_dirs"] == 0)
    check("its own scan read every folder", scanned["scanned_dirs"] >= 250)
    w.cmd("ui.view", {"id": "list"})

    # What indexes leave out, left out of searches too (ADR-125, 15).
    w.cmd("ui.settings", {"page": "exclusions"})
    check("the Exclusions page lists the kernel's filesystems and the Nix store",
          until(lambda: says(w, "Kernel filesystems") and says(w, "Nix store"), 30)
          or print("  read:", sorted({n for _, n in named(w) if n})[:60]))
    w.shot("3c-exclusions")
    w.cmd("ui.settings_close")

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

    # A tick pressed as a user presses it: the box shows what it is at
    # once, its keeper waking the window once the change is kept, and
    # pressed again it is back (ADR-150, 6; settings.md: the changes
    # queued in the order made).
    def docs_tick():
        boxes = [n for n in a11y.nodes_of(w, "check box")
                 if (n.get_description() or "").startswith("~/files/docs ·")]
        return boxes[0] if boxes else None

    def ticked():
        box = docs_tick()
        return None if box is None else a11y.has(box, "checked")

    def press(node):
        # gpui says where a node is in its window, whatever the coordinates
        # asked: the window's place on the display is X's.
        box = node.get_extents(Atspi.CoordType.WINDOW)
        xdotool = w.args.xdotool
        wid = subprocess.run([xdotool, "search", "--name", "skwir$"], env=w.env(),
                             capture_output=True, text=True).stdout.split()[0]
        at = dict(line.split("=", 1) for line in subprocess.run(
            [xdotool, "getwindowgeometry", "--shell", wid], env=w.env(),
            capture_output=True, text=True, check=True).stdout.split())
        x = int(at["X"]) + box.x + box.width // 2
        y = int(at["Y"]) + box.y + box.height // 2
        subprocess.run([xdotool, "mousemove", str(x), str(y)], env=w.env(), check=True)
        # Held: a press and its release within a frame may be lost.
        subprocess.run([xdotool, "mousedown", "1"], env=w.env(), check=True)
        time.sleep(0.3)
        subprocess.run([xdotool, "mouseup", "1"], env=w.env(), check=True)

    def kept_docs():
        listed = w.answer("skwird.kept_list")["records"]
        return [r["tick"]["on"] for r in listed if r["id"] == str(files / "docs")]

    check("the desktop's Documents is ticked", until(lambda: ticked() is True, 30)
          or print("  ticks:", [(n.get_name(), n.get_description())
                                for n in a11y.nodes_of(w, "check box")]))
    press(docs_tick())
    check("pressed, its box shows it unticked, nothing else asked to draw",
          until(lambda: ticked() is False, 10) or print("  ticked:", ticked()))
    check("its keeper says it unticked", kept_docs() == [False] or print("  said:", kept_docs()))
    w.shot("4c-index-unticked")
    press(docs_tick())
    check("pressed again, ticked again", until(lambda: ticked() is True, 10)
          or print("  ticked:", ticked()))
    check("its keeper says it ticked", kept_docs() == [True] or print("  said:", kept_docs()))
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
