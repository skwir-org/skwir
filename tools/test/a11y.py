#!/usr/bin/env python3
"""What a screen reader hears of the real window: skwir started on an X
display and a D-Bus session of its own, with the AT-SPI registry a screen
reader talks to, told that one listens (as Orca does when it starts), then
driven by keys and read back over AT-SPI at every step
(`docs/invariants/accessibility.md`).

    just a11y-check                    # finds every tool through Nix
    tools/test/a11y.py --xvfb X --icd I --atspi A --dbus D --xdotool T
                       [--skwir B] [--keep]

Needs PyGObject and the Atspi typelib (`just a11y-check` sets PYTHONPATH and
GI_TYPELIB_PATH). It never touches the user's display, session bus, files,
settings or Trash: everything lives under ~/.cache/skwir-tests/a11y-*.
`--keep` leaves the window and its folder for a look.
"""

import argparse
import json
import os
import pathlib
import shutil
import signal
import subprocess
import tempfile
import time

import gi

gi.require_version("Atspi", "2.0")
gi.require_version("Gio", "2.0")
from gi.repository import Atspi, Gio, GLib  # noqa: E402

REPO = pathlib.Path(__file__).resolve().parents[2]

# libatspi refuses the signature of AccessKit's RemoveAccessible signal, once
# per node removed (docs/invariants/accessibility.md, known gaps).
GLib.log_set_handler("dbind", GLib.LogLevelFlags.LEVEL_WARNING, lambda *_: None, None)


def fail(why):
    raise SystemExit(f"a11y: {why}")


class Window:
    def __init__(self, args):
        base = pathlib.Path.home() / ".cache" / "skwir-tests"
        base.mkdir(parents=True, exist_ok=True)
        # Short: a Unix socket's path has room for 108 bytes.
        self.root = pathlib.Path(tempfile.mkdtemp(prefix="a11y-", dir=base))
        self.home = self.root / "home"
        self.files = self.home / "files"
        self.runtime = self.root / "rt"
        self.args = args
        self.procs = []
        self.display = None
        self.bus = None
        self.app = None

    def env(self):
        drop = {"WAYLAND_DISPLAY", "DBUS_SESSION_BUS_ADDRESS", "AT_SPI_BUS_ADDRESS"}
        env = {k: v for k, v in os.environ.items() if k not in drop}
        env.update(
            DISPLAY=self.display,
            DBUS_SESSION_BUS_ADDRESS=self.bus,
            HOME=str(self.home),
            VK_ICD_FILENAMES=self.args.icd,
            XDG_CONFIG_HOME=str(self.root / "config"),
            XDG_CACHE_HOME=str(self.root / "cache"),
            XDG_STATE_HOME=str(self.root / "state"),
            XDG_DATA_HOME=str(self.root / "data"),
            XDG_RUNTIME_DIR=str(self.runtime),
        )
        return env

    def spawn(self, argv, log):
        proc = subprocess.Popen(
            argv, env=self.env(), stdin=subprocess.DEVNULL,
            stdout=open(self.root / log, "w"), stderr=subprocess.STDOUT,
            start_new_session=True,
        )
        self.procs.append(proc)
        return proc

    def start(self):
        (self.files / "notes").mkdir(parents=True)
        (self.files / "empty").mkdir()
        (self.files / "notes" / "it's a draft.txt").write_text("draft\n")
        (self.files / "café list.md").write_text("# List\n- bread\n")
        (self.files / "report 2026.txt").write_text("a report\n")
        self.runtime.mkdir(mode=0o700)
        config = self.root / "config" / "skwir"
        config.mkdir(parents=True)
        (config / "settings.toml").write_text("[plugins.control]\nenabled = true\n")
        self.display = self.start_display()
        self.start_bus()
        self.spawn([self.args.skwir, str(self.files)], "skwir.log")
        # Atspi reads the session bus it talks to once, when it starts.
        os.environ["DBUS_SESSION_BUS_ADDRESS"] = self.bus
        os.environ.pop("AT_SPI_BUS_ADDRESS", None)
        Atspi.init()
        if not until(lambda: self.find_app() is not None, 60):
            fail(f"the window never came on the bus; see {self.root / 'skwir.log'}")

    def start_display(self):
        """A display of its own, from :90 up, never one the desktop may use."""
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

    def start_bus(self):
        """A session bus of its own, the accessibility bus behind it, its
        registry, and the flag a screen reader raises when it starts."""
        socket = self.runtime / "bus"
        self.bus = f"unix:path={socket}"
        self.spawn([f"{self.args.dbus}/bin/dbus-daemon", "--session", "--nofork",
                    f"--address={self.bus}"], "dbus.log")
        if not until(socket.exists, 10):
            fail("the session bus did not start")
        libexec = pathlib.Path(self.args.atspi) / "libexec"
        self.spawn([str(libexec / "at-spi-bus-launcher"), "--launch-immediately"], "atspi.log")
        session = Gio.DBusConnection.new_for_address_sync(
            self.bus,
            Gio.DBusConnectionFlags.AUTHENTICATION_CLIENT
            | Gio.DBusConnectionFlags.MESSAGE_BUS_CONNECTION,
            None, None,
        )

        def owned():
            reply = session.call_sync(
                "org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus",
                "NameHasOwner", GLib.Variant("(s)", ("org.a11y.Bus",)), None, 0, -1, None,
            )
            return reply.unpack()[0]

        if not until(owned, 10):
            fail("the accessibility bus did not start")
        # The registry is a systemd service on a desktop; started here.
        self.spawn([str(libexec / "at-spi2-registryd")], "registryd.log")
        for flag in ("IsEnabled", "ScreenReaderEnabled"):
            session.call_sync(
                "org.a11y.Bus", "/org/a11y/bus", "org.freedesktop.DBus.Properties", "Set",
                GLib.Variant("(ssv)", ("org.a11y.Status", flag, GLib.Variant("b", True))),
                None, 0, -1, None,
            )

    def find_app(self):
        desktop = Atspi.get_desktop(0)
        for i in range(desktop.get_child_count()):
            app = desktop.get_child_at_index(i)
            if app is not None and app.get_name() == "skwir" and app.get_child_count() > 0:
                return app
        return None

    def frame(self):
        app = self.find_app()
        return app.get_child_at_index(0) if app else None

    def cmd(self, command, args=None):
        """A command run in the window, as a script runs it."""
        argv = [self.args.skwir, "cmd", command]
        if args is not None:
            argv.append(json.dumps(args))
        run = subprocess.run(argv, env=self.env(), capture_output=True, text=True, timeout=60)
        if run.returncode != 0:
            fail(f"`skwir cmd {command}`: {run.stderr.strip() or run.stdout.strip()}")

    def keys(self, *keys):
        """Keys pressed in the window, as a user presses them."""
        xdotool = self.args.xdotool
        env = self.env()
        wid = subprocess.run([xdotool, "search", "--name", "skwir$"], env=env,
                             capture_output=True, text=True).stdout.split()
        if not wid:
            fail("no window to send keys to")
        # No window manager: the window is given the keyboard's focus.
        subprocess.run([xdotool, "windowfocus", "--sync", wid[0]], env=env, check=True)
        for key in keys:
            subprocess.run([xdotool, "key", "--window", wid[0], key], env=env, check=True)
            time.sleep(0.3)

    def focused(self):
        """What a screen reader says has focus: the focused node that has no
        focused node under it."""
        frame = self.frame()
        if frame is None:
            return None
        found = None
        stack = [frame]
        while stack:
            node = stack.pop()
            if node is None:
                continue
            try:
                if has(node, "focused"):
                    found = node
                for i in range(node.get_child_count()):
                    stack.append(node.get_child_at_index(i))
            except GLib.Error:
                continue
        return found

    def stop(self):
        for proc in reversed(self.procs):
            if proc.poll() is None:
                os.killpg(proc.pid, signal.SIGTERM)
        for proc in self.procs:
            try:
                proc.wait(timeout=10)
            except subprocess.TimeoutExpired:
                os.killpg(proc.pid, signal.SIGKILL)


def has(node, state):
    return any(s.value_nick == state for s in node.get_state_set().get_states())


def said(node):
    """A node as a screen reader names it: role, name, states, position."""
    if node is None:
        return "nothing"
    attrs = node.get_attributes() or {}
    where = f" {attrs['posinset']} of {attrs['setsize']}" if "posinset" in attrs else ""
    states = [s for s in ("selected", "focused") if has(node, s)]
    return f"{node.get_role_name()} {node.get_name()!r}{where} {' '.join(states)}".strip()


def options_of(w, name):
    """The names of the options of list box `name`, wherever it is."""
    stack = [w.frame()]
    while stack:
        node = stack.pop()
        if node is None:
            continue
        if node.get_role_name() == "list box" and node.get_name() == name:
            return [node.get_child_at_index(i).get_name() for i in range(node.get_child_count())]
        stack.extend(node.get_child_at_index(i) for i in range(node.get_child_count()))
    return []


def check(what, holds):
    if not holds:
        fail(f"failed: {what}")
    print(f"ok: {what}")


def until(holds, seconds=20):
    deadline = time.time() + seconds
    while time.time() < deadline:
        try:
            if holds():
                return True
        except GLib.Error:
            pass
        time.sleep(0.2)
    return False


def hears(w, expected, seconds=20):
    """Whether the focus is `expected` (as `said` says it) within the time."""
    heard = []

    def holds():
        heard[:] = [said(w.focused())]
        return heard[0] == expected

    ok = until(holds, seconds)
    if not ok:
        print(f"  heard: {heard[0] if heard else 'nothing'}")
    return ok


def run(w):
    # The order the list shows: folders first, then files by name.
    first, second = "empty", "notes"
    check("the window is named by the folder it shows",
          until(lambda: w.frame().get_name() == "files — skwir"))

    w.keys()
    check("the file list has the focus, named by its folder",
          hears(w, "list box 'files' focused"))

    w.keys("Down")
    check("Down: the entry under the cursor, its place and that it is selected",
          hears(w, f"list item {first!r} 1 of 4 selected focused"))
    w.keys("Down")
    check("Down again: the next one", hears(w, f"list item {second!r} 2 of 4 selected focused"))

    w.keys("ctrl+2")
    check("the grid says the same entry", hears(w, f"list item {second!r} 2 of 4 selected focused"))

    w.keys("ctrl+3")
    check("the columns say it in its folder's list",
          hears(w, f"list item {second!r} 2 of 4 selected focused")
          and w.focused().get_parent().get_name() == "files")

    w.cmd("ui.view", {"id": "tree"})
    check("the tree says it as an item of the folder's tree",
          hears(w, f"tree item {second!r} selected focused")
          and w.focused().get_parent().get_role_name() == "tree")

    w.cmd("ui.view", {"id": "board"})
    check("the board's columns are list boxes named by their folders",
          until(lambda: "it's a draft.txt" in options_of(w, "notes")))

    w.keys("ctrl+1", "Up", "Return")
    check("an empty folder says so", until(
        lambda: (lambda f: f is not None and f.get_role_name() == "panel"
                 and f.get_name() == "empty"
                 and "empty" in (f.get_description() or "").lower())(w.focused())))


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--xvfb", required=True, help="the Xvfb program")
    parser.add_argument("--icd", required=True, help="a Vulkan ICD file (lavapipe's)")
    parser.add_argument("--atspi", required=True, help="at-spi2-core's store path")
    parser.add_argument("--dbus", required=True, help="dbus's store path")
    parser.add_argument("--xdotool", required=True, help="the xdotool program")
    parser.add_argument("--skwir", default=str(REPO / "target/release/skwir"))
    parser.add_argument("--keep", action="store_true", help="leave the window running")
    args = parser.parse_args()
    if not pathlib.Path(args.skwir).exists():
        fail(f"no skwir at {args.skwir}: `cargo build --release -p skwir`")
    w = Window(args)
    try:
        w.start()
        run(w)
        print("a11y: passed")
    finally:
        if args.keep:
            groups = " ".join(f"-{p.pid}" for p in w.procs)
            print(f"a11y: kept in {w.root} on DISPLAY={w.display}, "
                  f"DBUS_SESSION_BUS_ADDRESS={w.bus}; stop with: kill -- {groups}")
        else:
            w.stop()
            shutil.rmtree(w.root, ignore_errors=True)


if __name__ == "__main__":
    main()
