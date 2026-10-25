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
import datetime
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
        drop = {"WAYLAND_DISPLAY", "DBUS_SESSION_BUS_ADDRESS", "AT_SPI_BUS_ADDRESS", "PS1"}
        env = {k: v for k, v in os.environ.items() if k not in drop}
        env.update(
            # The terminal's shell: a plain one, with nothing of the user's.
            SHELL="/bin/sh",
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

    def type(self, text):
        """Text typed in the window."""
        subprocess.run([self.args.xdotool, "type", "--delay", "60", text],
                       env=self.env(), check=True)
        time.sleep(0.3)

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


def text_of(node):
    """A text field's text and where its caret is."""
    count = Atspi.Text.get_character_count(node)
    return Atspi.Text.get_text(node, 0, count), Atspi.Text.get_caret_offset(node)


class Announcements:
    """What the window announces (`object:announcement`, a live region's
    name), as a screen reader hears it."""

    def __init__(self):
        self.said = []
        self.listener = Atspi.EventListener.new(self.heard)
        self.listener.register("object:announcement")

    def heard(self, event):
        try:
            self.said.append(event.source.get_name())
        except GLib.Error:
            pass

    def hears(self, text, seconds=20):
        context = GLib.MainContext.default()
        deadline = time.time() + seconds
        while time.time() < deadline:
            while context.iteration(False):
                pass
            if text in self.said:
                return True
            time.sleep(0.1)
        print(f"  announced: {self.said}")
        return False


def within(node):
    """The names of a node's ancestors, nearest first."""
    names = []
    node = node.get_parent() if node else None
    while node is not None and node.get_role_name() != "application":
        names.append(node.get_name())
        node = node.get_parent()
    return names


def selected_in(node):
    """What a text field has selected, as offsets."""
    if Atspi.Text.get_n_selections(node) == 0:
        return None
    selected = Atspi.Text.get_selection(node, 0)
    return selected.start_offset, selected.end_offset


def reads(w, expected, seconds=20):
    """Whether the focused field's text and caret are `expected` in time."""
    read = []

    def holds():
        read[:] = [text_of(w.focused())]
        return read[0] == expected

    ok = until(holds, seconds)
    if not ok:
        print(f"  read: {read[0] if read else 'nothing'}")
    return ok


def children_of(w, role, name):
    """The children of the node `role` `name`, wherever it is."""
    stack = [w.frame()]
    while stack:
        node = stack.pop()
        if node is None:
            continue
        if node.get_role_name() == role and node.get_name() == name:
            return [node.get_child_at_index(i) for i in range(node.get_child_count())]
        stack.extend(node.get_child_at_index(i) for i in range(node.get_child_count()))
    return []


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
        # A node gone while read, or no focus yet while a popup opens.
        except (GLib.Error, AttributeError, IndexError):
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

    def path():
        parts = children_of(w, "landmark", "Path")
        links = [p.get_name() for p in parts if p.get_role_name() == "link"]
        lefts = [p.get_extents(Atspi.CoordType.WINDOW).x for p in parts]
        return links, lefts == sorted(lefts) and len(lefts) > 1

    check("the path reads from its first folder to the one shown, as on screen",
          until(lambda: path() == (["Home", "files"], True)) or print("  read:", path()))

    w.keys("Down")
    check("Down: the entry under the cursor, its place and that it is selected",
          hears(w, f"list item {first!r} 1 of 4 selected focused"))
    w.keys("Down")
    check("Down again: the next one", hears(w, f"list item {second!r} 2 of 4 selected focused"))

    def status():
        stack, found = [w.frame()], []
        while stack:
            node = stack.pop()
            if node is None:
                continue
            if node.get_role_name() == "status bar":
                found.append(node.get_name())
            stack.extend(node.get_child_at_index(i) for i in range(node.get_child_count()))
        return found

    check("the footer is a status line saying the count and the selection",
          until(lambda: any(s.startswith("4 items") and "1 selected" in s for s in status()))
          or print("  read:", status()))

    w.keys("ctrl+l")
    check("Ctrl+L: the address, a text field named so", hears(w, "entry 'Address' focused"))
    shown = str(w.files)
    check("it holds the folder's path, all of it selected",
          reads(w, (shown, len(shown))) and selected_in(w.focused()) == (0, len(shown)))
    w.keys("End", "BackSpace", "Left")
    check("what is typed and where the caret goes are read back",
          reads(w, (shown[:-1], len(shown) - 2)))
    check("the names that go on with it are a list box beside the field",
          until(lambda: w.files.name in options_of(w, "Suggestions"))
          or print("  read:", options_of(w, "Suggestions")))
    w.keys("Down")
    check("Down: the suggestion reached, said as focused",
          until(lambda: said(w.focused()).startswith(f"list item {w.files.name!r}")
                and has(w.focused(), "selected")))
    w.keys("Escape")
    check("Escape: back to the files", hears(w, f"list item {second!r} 2 of 4 selected focused"))

    # Into the Trash of the window's own data folder, and back.
    third, fourth = "café list.md", "report 2026.txt"
    announced = Announcements()
    w.keys("Down", "Delete")
    check("a file trashed is announced", announced.hears(f"Moved “{third}” to Trash"))
    check("the cursor goes to the file after it, selected, as in Dolphin and Nautilus",
          hears(w, f"list item {fourth!r} 3 of 3 selected focused"))
    w.keys("ctrl+z")
    check("an undoing is announced too", announced.hears("Undid: move to trash")
          and until(lambda: (w.files / third).exists()))
    w.keys("Home")
    check("Home: the first entry", hears(w, f"list item {first!r} 1 of 4 selected focused"))

    w.keys("shift+F10")
    check("Shift+F10: the context menu, its first item said as focused",
          until(lambda: w.focused().get_role_name() == "menu item"
                and within(w.focused())[0] == "Context menu"))
    w.keys("Escape")
    check("Escape: back to the entry", hears(w, f"list item {first!r} 1 of 4 selected focused"))

    w.keys("ctrl+shift+p")
    check("Ctrl+Shift+P: the palette's highlighted command, in its dialog",
          until(lambda: w.focused().get_role_name() == "list item"
                and within(w.focused())[:2] == ["Commands", "Command palette"]))
    w.type("grid view")
    check("what is typed finds a command, said as focused",
          until(lambda: said(w.focused()).startswith("list item 'Grid view' 1 of")
                and has(w.focused(), "selected")))
    w.keys("Escape")

    w.cmd("ui.tags_prompt")
    check("the tag editor's field, in its dialog",
          until(lambda: w.focused().get_role_name() == "entry"
                and within(w.focused())[0] == "Tag editor"))
    w.type("draft")
    check("a tag typed is offered to add, said as focused",
          hears(w, "list item 'Add draft' 1 of 1 selected focused")
          and within(w.focused())[:2] == ["Tags", "Tag editor"])
    w.keys("Escape")

    w.cmd("ui.jump")
    check("Jump: its field, in its dialog",
          until(lambda: w.focused().get_role_name() == "entry"
                and within(w.focused())[0] == "Jump"))
    w.keys("Escape")
    check("Escape: back to the entry", hears(w, f"list item {first!r} 1 of 4 selected focused"))

    w.keys("F2")
    check("F2: the rename prompt's field, named by its question",
          hears(w, "entry 'Rename' focused") and within(w.focused())[0] == "Rename")
    w.keys("Escape")

    w.keys("ctrl+comma")
    check("Ctrl+,: the settings, their page said as the selected tab",
          hears(w, "page tab 'General' selected focused")
          and within(w.focused())[:2] == ["Pages", "Settings"])
    w.keys("Escape")
    check("Escape: back to the entry", hears(w, f"list item {first!r} 1 of 4 selected focused"))

    w.keys("End", "space")
    check("Space: Peek, a dialog named by the file, saying what it is and its keys",
          until(lambda: w.focused().get_role_name() == "dialog"
                and w.focused().get_name() == fourth
                and "Escape to close" in (w.focused().get_description() or "")))
    check("and the file's text, in a document named by it",
          until(lambda: "a report" in [n.get_name() for n in children_of(w, "document frame", fourth)]))
    w.keys("Escape")
    w.keys("F11")
    w.keys("F6")
    check("F6 with the preview open: the keyboard goes to what it shows, a document",
          hears(w, f"document frame {fourth!r} focused"))
    w.keys("F6")
    check("F6 again: the sidebar", until(lambda: within(w.focused())[:1] == ["Sidebar"]))
    w.keys("F6")
    check("and again: back to the files", hears(w, f"list item {fourth!r} 4 of 4 selected focused"))
    w.keys("F11", "Home")
    check("Escape: back to the entry", hears(w, f"list item {first!r} 1 of 4 selected focused"))

    w.keys("ctrl+2")
    check("the grid says the same entry", hears(w, f"list item {first!r} 1 of 4 selected focused"))

    w.keys("ctrl+3")
    check("the columns say it in its folder's list",
          hears(w, f"list item {first!r} 1 of 4 selected focused")
          and w.focused().get_parent().get_name() == "files")

    w.cmd("ui.view", {"id": "tree"})
    check("the tree says it as an item of the folder's tree",
          hears(w, f"tree item {first!r} selected focused")
          and w.focused().get_parent().get_role_name() == "tree")
    w.keys("Down", "Right", "Down")
    check("Right opens a folder in place: its file is said",
          hears(w, "tree item \"it's a draft.txt\" selected focused"))
    w.cmd("expand", {"mode": "collapse_all"})
    check("closed with the cursor inside, the folder takes the cursor",
          hears(w, f"tree item {second!r} selected focused"))
    w.keys("Up")

    w.cmd("ui.view", {"id": "board"})
    check("the board's columns are list boxes named by their folders",
          until(lambda: "it's a draft.txt" in options_of(w, "notes")))

    w.keys("ctrl+1", "Up", "Return")
    check("an empty folder says so", until(
        lambda: (lambda f: f is not None and f.get_role_name() == "panel"
                 and f.get_name() == "empty"
                 and "empty" in (f.get_description() or "").lower())(w.focused())))

    w.keys("F4")
    check("F4: the terminal, named so", hears(w, "terminal 'Terminal' focused"))
    w.type("echo hello-a11y")
    w.keys("Return")
    check("what it shows is read as text: the command and what it printed",
          until(lambda: text_of(w.focused())[0].count("hello-a11y") == 2, 30)
          or print("  read:", text_of(w.focused())))
    w.keys("F4")

    w.keys("F6")
    check("F6: the sidebar's first group, in the sidebar's tree",
          hears(w, "tree item 'Places' focused")
          and w.focused().get_parent().get_name() == "Sidebar")
    w.keys("Down")
    check("Down: its first place", hears(w, "tree item 'Home' focused"))

    # The fixture's files were all made today: one bar, a day, in words.
    months = ["January", "February", "March", "April", "May", "June", "July",
              "August", "September", "October", "November", "December"]
    today = datetime.date.today()
    day = f"{today.day} {months[today.month - 1]} {today.year}"
    # Out of the empty folder, back to the fixture's files.
    w.cmd("back")
    w.keys("ctrl+shift+l", "F6", "F6")
    check("Ctrl+Shift+L, F6 twice: the timeline, a list of its bars, once counted",
          hears(w, "list box 'Timeline' focused")
          and (until(lambda: options_of(w, "Timeline") == [day])
               or print("  options:", options_of(w, "Timeline"))))
    w.keys("Right")
    check("a key: the bar it is on, said as focused",
          hears(w, f"list item {day!r} 1 of 1 selected focused")
          and w.focused().get_parent().get_name() == "Timeline")
    check("and what it holds as its description",
          "files" in (w.focused().get_description() or ""))


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
