#!/usr/bin/env python3
"""A smoke test of the real window: bibli started on an X display of its
own (Xvfb, software Vulkan), with a home of synthetic files, driven through
`bibli cmd` as a script drives it, every step checked on disk.

    just smoke                                  # finds Xvfb and Mesa through Nix
    tools/test/smoke.py --xvfb X --icd I [--bibli B] [--keep]

Each step goes through the open window: list a folder, search it, tag a
file, rename it, make a folder and move the file into it, trash it, then
undo each in order and check the folder is as it began. It never touches
the user's display, files, settings or Trash: everything lives under
~/.cache/bibli-tests/smoke-*, with XDG folders and a runtime folder of
its own. `--keep` leaves the window and its folder for a look.
"""

import argparse
import json
import os
import pathlib
import shutil
import signal
import subprocess
import sys
import tempfile
import time

REPO = pathlib.Path(__file__).resolve().parents[2]


def fail(why):
    raise SystemExit(f"smoke: {why}")


def tags_of(path):
    try:
        return os.getxattr(path, "user.xdg.tags").decode()
    except OSError:
        return ""


class Window:
    def __init__(self, args):
        base = pathlib.Path.home() / ".cache" / "bibli-tests"
        base.mkdir(parents=True, exist_ok=True)
        # Short: a Unix socket's path has room for 108 bytes.
        self.root = pathlib.Path(tempfile.mkdtemp(prefix="smoke-", dir=base))
        self.home = self.root / "home"
        self.files = self.home / "files"
        self.runtime = self.root / "rt"
        self.args = args
        self.procs = []
        self.display = None

    def env(self):
        env = {k: v for k, v in os.environ.items() if k != "WAYLAND_DISPLAY"}
        env.update(
            DISPLAY=self.display,
            HOME=str(self.home),
            VK_ICD_FILENAMES=self.args.icd,
            XDG_CONFIG_HOME=str(self.root / "config"),
            XDG_CACHE_HOME=str(self.root / "cache"),
            XDG_DATA_HOME=str(self.root / "data"),
            XDG_RUNTIME_DIR=str(self.runtime),
        )
        return env

    def start(self):
        (self.files / "notes").mkdir(parents=True)
        (self.files / "photos").mkdir()
        (self.files / "notes" / "it's a draft.txt").write_text("draft\n")
        (self.files / "notes" / "café list.md").write_text("# List\n- bread\n")
        (self.files / "notes" / "report 2026.txt").write_text("needle in a report\n")
        (self.files / "photos" / "sea.txt").write_text("not a picture\n")
        self.runtime.mkdir(mode=0o700)
        config = self.root / "config" / "bibli"
        config.mkdir(parents=True)
        (config / "settings.toml").write_text("[plugins.control]\nenabled = true\n")
        self.display = self.start_display()
        log = open(self.root / "bibli.log", "w")
        window = subprocess.Popen(
            [self.args.bibli, str(self.files)], env=self.env(),
            stdin=subprocess.DEVNULL, stdout=log, stderr=log, start_new_session=True,
        )
        self.procs.append(window)
        sock = self.runtime / "bibli" / "control.sock"
        deadline = time.time() + 60
        while not sock.exists():
            if window.poll() is not None or time.time() > deadline:
                fail(f"the window did not start; see {self.root / 'bibli.log'}")
            time.sleep(0.2)

    def start_display(self):
        """A display of its own, from :90 up, never one the desktop may
        use (`-displayfd` would take the first free, :0 where XWayland is
        elsewhere)."""
        for number in range(90, 200):
            if pathlib.Path(f"/tmp/.X{number}-lock").exists() or pathlib.Path(
                f"/tmp/.X11-unix/X{number}"
            ).exists():
                continue
            xvfb = subprocess.Popen(
                [self.args.xvfb, f":{number}", "-screen", "0", "1280x860x24", "-nolisten", "tcp"],
                stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, start_new_session=True,
            )
            socket = pathlib.Path(f"/tmp/.X11-unix/X{number}")
            deadline = time.time() + 10
            while not socket.exists() and xvfb.poll() is None and time.time() < deadline:
                time.sleep(0.1)
            if socket.exists() and xvfb.poll() is None:
                self.procs.append(xvfb)
                return f":{number}"
            if xvfb.poll() is None:
                os.killpg(xvfb.pid, signal.SIGTERM)
        fail("no display free from :90 to :199")

    def cmd(self, command, args=None, apply=False):
        argv = [self.args.bibli, "cmd"] + (["--apply"] if apply else []) + [command]
        if args is not None:
            argv.append(json.dumps(args))
        run = subprocess.run(argv, env=self.env(), capture_output=True, text=True, timeout=60)
        if run.returncode != 0:
            fail(f"`bibli cmd {command}`: {run.stderr.strip() or run.stdout.strip()}")
        return json.loads(run.stdout) if run.stdout.strip() else None

    def stop(self):
        for proc in reversed(self.procs):
            if proc.poll() is None:
                os.killpg(proc.pid, signal.SIGTERM)
        for proc in self.procs:
            try:
                proc.wait(timeout=10)
            except subprocess.TimeoutExpired:
                os.killpg(proc.pid, signal.SIGKILL)


def tree(folder):
    return sorted(str(p.relative_to(folder)) for p in folder.rglob("*"))


def check(what, holds):
    if not holds:
        fail(f"failed: {what}")
    print(f"ok: {what}")


def until(holds, seconds=20):
    deadline = time.time() + seconds
    while not holds() and time.time() < deadline:
        time.sleep(0.2)
    return holds()


def run(w):
    notes = w.files / "notes"
    draft = notes / "it's a draft.txt"
    before = tree(w.files)

    listed = w.cmd("entries", {"location": str(notes)})
    names = {e["name"] for e in listed["entries"]}
    check("a folder is listed", {"it's a draft.txt", "café list.md", "report 2026.txt"} <= names)

    w.cmd("query.search", {"expr": "content:needle", "root": str(w.files)})
    seen = []

    def found():
        seen[:] = [e["name"] for e in w.cmd("entries")["entries"]]
        return seen == ["report 2026.txt"]

    check("a search finds the file holding the word", until(found, 60) or print("listed:", seen))

    w.cmd("tags.add", {"entries": [str(draft)], "tags": ["smoke"]}, apply=True)
    check("a tag is written on the file", "smoke" in tags_of(draft))

    renamed = notes / "it's done.txt"
    w.cmd("rename", {"entries": [str(draft)], "new_name": renamed.name}, apply=True)
    check("a file is renamed, its tag with it",
          renamed.exists() and not draft.exists() and "smoke" in tags_of(renamed))

    w.cmd("mkdir", {"in": str(notes), "name": "kept"}, apply=True)
    w.cmd("move", {"entries": [str(renamed)], "dest": str(notes / "kept")}, apply=True)
    moved = notes / "kept" / renamed.name
    check("a file is moved into a new folder", moved.exists() and not renamed.exists())

    w.cmd("trash", {"entries": [str(moved)]}, apply=True)
    check("a file goes to the Trash", not moved.exists())

    # Five plans: the tag, the rename, the folder, the move, the trash.
    for _ in range(5):
        w.cmd("undo")
    check("undoing each step brings the folder back as it was",
          until(lambda: tree(w.files) == before) and tags_of(draft) == "")


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--xvfb", required=True, help="the Xvfb program")
    parser.add_argument("--icd", required=True, help="a Vulkan ICD file (lavapipe's)")
    parser.add_argument("--bibli", default=str(REPO / "target/release/bibli"))
    parser.add_argument("--keep", action="store_true", help="leave the window running")
    args = parser.parse_args()
    if not pathlib.Path(args.bibli).exists():
        fail(f"no bibli at {args.bibli}: `cargo build --release -p bibli`")
    w = Window(args)
    try:
        w.start()
        run(w)
        print("smoke: passed")
    finally:
        if args.keep:
            groups = " ".join(f"-{p.pid}" for p in w.procs)
            print(f"smoke: kept in {w.root} on DISPLAY={w.display}; stop with: kill -- {groups}")
        else:
            w.stop()
            shutil.rmtree(w.root, ignore_errors=True)


if __name__ == "__main__":
    main()
