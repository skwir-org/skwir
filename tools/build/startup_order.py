#!/usr/bin/env python3
"""Make tools/build/startup-order.txt: the functions a start of skwir runs,
in the order it first runs them (ADR-128).

    just startup-order                 # builds `dist`, finds its tools through Nix
    tools/build/startup_order.py --binary B --sway S --xvfb X --icd I --perf P

Starts the real window several times, on a Wayland compositor and an X
server of its own (software Vulkan, a session bus and folders of its own:
it never touches the user's display, files or settings), and records each
start to idle with Intel Processor Trace, filtered to the binary's own
code. That sees every branch taken, which sampling cannot: what is run once
is what this list is for. Needs a CPU with Intel PT; nothing else does, as
the packages only read the list this writes.

Two programs start: the file manager, on a folder, then a viewer's own
(`skwir view`, ADR-151), on a text file, a picture and a Markdown file in
turn. The file manager's code comes first, laid out as if it were alone:
its start is what ADR-000 budgets. What a viewer's start runs beside it
follows.

A list made here goes stale as the code moves (a list 74 commits old kept
half of the gain): run this before tagging a release, after the last fix.
"""
import argparse
import bisect
import concurrent.futures
import os
import pathlib
import re
import shutil
import signal
import struct
import subprocess
import sys
import tempfile
import time
import zlib

REPO = pathlib.Path(__file__).resolve().parents[2]
PT = pathlib.Path("/sys/bus/event_source/devices/intel_pt")
HEADER = """\
# The functions a start of skwir runs, by first execution (ADR-128): the
# file manager's, then what a viewer's own program runs beside it.
# Made by `just startup-order`; read by tools/build/link-ordered.
"""

# perf's branches, as "file offset (64-byte block), first seen" (seconds
# since the first branch): the code run between a branch's target and the
# next branch taken on its thread, within the program's text.
COVER = r"""
BEGIN { base = strtonum(ENVIRON["MAPSTART"]); end_ = base + strtonum(ENVIRON["MAPSIZE"])
        pgoff = strtonum(ENVIRON["PGOFF"]) }
function mark(g) { if (!(g in first)) first[g] = t }
{
    tid = $1; t = $2 + 0; if (!t0) t0 = t
    src = strtonum("0x" $3); dst = strtonum("0x" $5)
    if (src >= base && src < end_) {
        if ((tid in last) && src >= last[tid] && src - last[tid] < 262144)
            for (g = int((last[tid] - base) / 64); g <= int((src - base) / 64); g++) mark(g)
        else mark(int((src - base) / 64))
    }
    if (dst >= base && dst < end_) { mark(int((dst - base) / 64)); last[tid] = dst }
    else delete last[tid]
}
END { for (g in first) printf "%d %.6f\n", g * 64 + pgoff, first[g] - t0 }
"""


def fail(why):
    raise SystemExit(f"startup-order: {why}")


def c_locale():
    return dict(os.environ, LC_ALL="C")


def text_segment(binary):
    """The code's (file offset, address, size) in `binary`."""
    out = subprocess.run(["readelf", "-lW", str(binary)], capture_output=True, text=True,
                         check=True, env=c_locale()).stdout
    for line in out.splitlines():
        fields = line.split()
        if fields[:1] == ["LOAD"] and "E" in fields[-3:-1]:
            return int(fields[1], 16), int(fields[2], 16), int(fields[5], 16)
    fail(f"{binary} has no code segment")


def symbols(binary, delta):
    """Its functions as (file offset, size, demangled name), by offset."""
    out = subprocess.run(["nm", "-S", "-C", "--defined-only", str(binary)], capture_output=True,
                         text=True, check=True, env=c_locale()).stdout
    found = []
    for line in out.splitlines():
        parts = line.split(maxsplit=3)
        if len(parts) == 4 and parts[2] in "tTwW":
            found.append((int(parts[0], 16) - delta, int(parts[1], 16), parts[3]))
    found.sort()
    if not found:
        fail(f"{binary} has no symbol table")
    return found


def first_runs(syms, blocks):
    """Each function run, by name, with when it first ran. A block names
    every function it overlaps: a small one may start in the middle of it,
    and a function missed costs a whole 64 KiB window."""
    starts = [s[0] for s in syms]
    first = {}
    for offset, when in blocks:
        at = max(bisect.bisect_right(starts, offset) - 1, 0)
        while at < len(syms) and syms[at][0] < offset + 64:
            start, size, name = syms[at]
            if start + max(size, 1) > offset and when < first.get(name, float("inf")):
                first[name] = when
            at += 1
    return first


def merged(firsts):
    """One view of several starts: each function at its earliest."""
    first = {}
    for each in firsts:
        for name, when in each.items():
            if when < first.get(name, float("inf")):
                first[name] = when
    return first


def laid_out(programs):
    """The functions of every program's starts, in the order for the list,
    a program after another (each a display's first runs, by display): of
    each, what was not placed before it, what its starts on every display
    run, by first execution, then what each display's run alone. Mixed, a
    start on one display would keep windows of the other's code; the first
    program's code is laid out as if it were alone, and a later one's
    follows it."""
    placed = []
    taken = set()

    def add(names, first):
        placed.extend(sorted(names, key=lambda name: (first[name], name)))

    for by_display in programs:
        displays = list(by_display)
        everywhere = set.intersection(*(set(by_display[d]) for d in displays)) - taken
        add(everywhere, merged(by_display.values()))
        taken |= everywhere
        for display in displays:
            alone = set(by_display[display]) - taken
            add(alone, by_display[display])
            taken |= alone
    return placed


def picture(width, height):
    """A PNG of `width` × `height`, a gradient, made here: no picture
    library is asked."""

    def chunk(kind, data):
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body))

    rows = b"".join(b"\0" + bytes(((x * 255 // width), (y * 255 // height), 128)[c]
                                    for x in range(width) for c in range(3))
                    for y in range(height))
    header = struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0)
    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", header)
            + chunk(b"IDAT", zlib.compress(rows)) + chunk(b"IEND", b""))


def write_list(names, output):
    output.write_text(HEADER + "".join(f"{name}\n" for name in names))


class Session:
    """A display, a session bus and folders of their own."""

    def __init__(self, args):
        base = pathlib.Path.home() / ".cache" / "skwir-tests"
        base.mkdir(parents=True, exist_ok=True)
        # Short: a Unix socket's path has room for 108 bytes.
        self.root = pathlib.Path(tempfile.mkdtemp(prefix="so-", dir=base))
        self.runtime = self.root / "rt"
        self.runtime.mkdir(mode=0o700)
        self.home = self.root / "home"
        self.folder = self.home / "files"
        (self.folder / "photos").mkdir(parents=True)
        (self.folder / "notes.txt").write_text("alpha\n")
        (self.folder / "readme.md").write_text("beta\n")
        (self.folder / "data.bin").write_bytes(bytes(4096))
        (self.folder / "photos" / "dune.png").write_bytes(picture(160, 120))
        # What a viewer's starts show, in turn: a text file, a picture, a
        # Markdown file.
        self.viewed = [self.folder / "notes.txt", self.folder / "photos" / "dune.png",
                       self.folder / "readme.md"]
        for name in ("config", "cache", "state", "data"):
            (self.root / name).mkdir()
        self.args = args
        self.procs = []
        self.bus = None

    def spawn(self, command, log, env=None):
        with open(self.root / log, "w") as out:
            process = subprocess.Popen(command, env=env, stdout=out, stderr=subprocess.STDOUT,
                                       stdin=subprocess.DEVNULL, start_new_session=True)
        self.procs.append(process)
        return process

    def wait_for(self, path, what):
        deadline = time.monotonic() + 20
        while time.monotonic() < deadline:
            if path.exists():
                return
            time.sleep(0.05)
        fail(f"{what} did not start; see {self.root}")

    def start_bus(self):
        socket = self.runtime / "bus"
        config = self.root / "bus.conf"
        # No service folders: nothing of the user's desktop is started.
        config.write_text(f"""<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <type>session</type>
  <listen>unix:path={socket}</listen>
  <policy context="default">
    <allow send_destination="*" eavesdrop="true"/>
    <allow eavesdrop="true"/>
    <allow own="*"/>
  </policy>
</busconfig>
""")
        self.spawn([self.args.dbus, "--nofork", f"--config-file={config}"], "dbus.log")
        self.wait_for(socket, "the session bus")
        self.bus = f"unix:path={socket}"

    def clean_env(self):
        drop = ("DISPLAY", "WAYLAND_DISPLAY", "SWAYSOCK", "HYPRLAND_INSTANCE_SIGNATURE",
                "I3SOCK", "XAUTHORITY")
        env = {k: v for k, v in os.environ.items() if k not in drop}
        env.update(XDG_RUNTIME_DIR=str(self.runtime))
        return env

    def start_wayland(self):
        config = self.root / "sway.conf"
        config.write_text("output HEADLESS-1 resolution 1280x860\n")
        env = dict(self.clean_env(), WLR_BACKENDS="headless", WLR_RENDERER="pixman",
                   WLR_LIBINPUT_NO_DEVICES="1")
        self.spawn([self.args.sway, "-c", str(config)], "sway.log", env)
        socket = self.runtime / "wayland-1"
        self.wait_for(socket, "the compositor")
        return {"WAYLAND_DISPLAY": socket.name}

    def start_x11(self):
        display = ":9" + str(os.getpid() % 90 + 10)
        self.spawn([self.args.xvfb, display, "-screen", "0", "1280x860x24", "-nolisten", "tcp"],
                   "xvfb.log", self.clean_env())
        self.wait_for(pathlib.Path("/tmp/.X11-unix") / f"X{display[1:]}", "the X server")
        return {"DISPLAY": display}

    def app_env(self, display):
        env = self.clean_env()
        env.update(display)
        env.update(HOME=str(self.home), VK_ICD_FILENAMES=self.args.icd,
                   XDG_CONFIG_HOME=str(self.root / "config"),
                   XDG_CACHE_HOME=str(self.root / "cache"),
                   XDG_STATE_HOME=str(self.root / "state"),
                   XDG_DATA_HOME=str(self.root / "data"),
                   # What `just idle-pss` logs at: a list without it leaves
                   # the logging's code scattered in the very measure.
                   DBUS_SESSION_BUS_ADDRESS=self.bus, RUST_LOG="skwir_ui=info")
        return env

    def run(self, command, display, idle, log):
        """`command` for `idle` seconds, then asked to stop."""
        process = self.spawn(command, log, self.app_env(display))
        time.sleep(idle)
        if process.poll() is not None:
            fail(f"it ended by itself; see {self.root / log}")
        process.send_signal(signal.SIGINT if "perf" in command[0] else signal.SIGTERM)
        try:
            process.wait(timeout=30)
        except subprocess.TimeoutExpired:
            os.killpg(process.pid, signal.SIGKILL)
            process.wait()
        self.procs.remove(process)

    def trace(self, binary, start, display, idle, data, log):
        """`start`, the program's arguments, traced."""
        offset, _, size = text_segment(binary)
        self.run([self.args.perf, "record", "-o", str(data), "-e", "intel_pt/cyc=0/u", "-m,256K",
                  "--filter", f"filter {offset:#x}/{size:#x}@{binary}", "--",
                  str(binary), *start], display, idle, log)

    def close(self):
        for process in reversed(self.procs):
            try:
                os.killpg(process.pid, signal.SIGTERM)
            except ProcessLookupError:
                continue
            try:
                process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                os.killpg(process.pid, signal.SIGKILL)
                process.wait()
        if not self.args.keep:
            shutil.rmtree(self.root, ignore_errors=True)


def decode(perf, binary, data):
    """What a trace ran, as (file offset, first seen)."""
    # Where the code was mapped, among the events alone: left to itself,
    # perf script decodes every call of a processor trace with them
    # (`--itrace=ce`), gigabytes of a start held here for one line, which
    # filled the memory of a machine (2026-11-15).
    events = subprocess.run([perf, "script", "-i", str(data), "--show-mmap-events",
                             "--itrace=e"],
                            capture_output=True, text=True, check=False).stdout
    mapped = re.search(r"MMAP2?.*\[(0x[0-9a-f]+)\((0x[0-9a-f]+)\) @ (0x[0-9a-f]+)[^\]]*\]: r-xp "
                       + re.escape(str(binary)), events)
    if not mapped:
        fail(f"{data}: the trace does not say where the code was")
    env = dict(c_locale(), MAPSTART=mapped[1], MAPSIZE=mapped[2], PGOFF=mapped[3])
    script = subprocess.Popen([perf, "script", "-i", str(data), "--itrace=b",
                               "-F", "tid,time,ip,addr"],
                              stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
    cover = subprocess.run(["gawk", COVER], stdin=script.stdout, capture_output=True, text=True,
                           check=True, env=env)
    script.stdout.close()
    script.wait()
    return [(int(offset), float(when)) for offset, when in
            (line.split() for line in cover.stdout.splitlines())]


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n", maxsplit=1)[0])
    parser.add_argument("--binary", default=str(REPO / "target/dist/skwir"))
    parser.add_argument("--sway", required=True)
    parser.add_argument("--xvfb", required=True)
    parser.add_argument("--icd", required=True, help="a software Vulkan driver's manifest")
    parser.add_argument("--perf", required=True)
    parser.add_argument("--dbus", default=shutil.which("dbus-daemon"))
    parser.add_argument("--runs", type=int, default=4, help="traced starts on each display")
    parser.add_argument("--idle", type=float, default=10.0, help="seconds each start is traced")
    parser.add_argument("--output", default=str(REPO / "tools/build/startup-order.txt"))
    parser.add_argument("--keep", action="store_true", help="leave the session's folder")
    args = parser.parse_args()
    if not PT.exists():
        fail("this CPU has no Intel Processor Trace")
    if not args.dbus:
        fail("no dbus-daemon")
    binary = pathlib.Path(os.path.realpath(args.binary))
    offset, address, _ = text_segment(binary)
    syms = symbols(binary, address - offset)

    session = Session(args)
    # What each program is started with, its `n`th start: the file manager
    # on a folder, a viewer's own program on a file in turn (ADR-151).
    programs = {
        "skwir": lambda n: [str(session.folder)],
        "view": lambda n: ["view", str(session.viewed[n % len(session.viewed)])],
    }
    traces = []  # (program, display, its data)
    try:
        session.start_bus()
        for kind, start in (("wayland", session.start_wayland), ("x11", session.start_x11)):
            display = start()
            for program, arguments in programs.items():
                # Once untraced: what follows is a start like any other,
                # with its caches filled, not a first one.
                print(f"{kind}, {program}: warming up", flush=True)
                session.run([str(binary), *arguments(0)], display, args.idle,
                            f"{kind}-{program}-warm.log")
                for n in range(args.runs):
                    print(f"{kind}, {program}: start {n + 1} of {args.runs}", flush=True)
                    data = session.root / f"{kind}-{program}-{n}.perf"
                    session.trace(binary, arguments(n), display, args.idle, data,
                                  f"{kind}-{program}-{n}.log")
                    traces.append((program, kind, data))
        print(f"decoding {len(traces)} traces", flush=True)
        with concurrent.futures.ThreadPoolExecutor(max_workers=6) as pool:
            covers = list(pool.map(lambda trace: decode(args.perf, binary, trace[2]), traces))
    finally:
        session.close()

    by_program = {program: {} for program in programs}
    for (program, kind, _), cover in zip(traces, covers):
        by_display = by_program[program]
        by_display[kind] = merged([by_display.get(kind, {}), first_runs(syms, cover)])
    names = laid_out(list(by_program.values()))
    if len(names) < 1000:
        fail(f"only {len(names)} functions seen: the traces did not cover a start")
    write_list(names, pathlib.Path(args.output))
    sizes = {s[2]: s[1] for s in syms}
    seen = set()
    for program, by_display in by_program.items():
        ran = set().union(*by_display.values())
        added = ran - seen
        seen |= ran
        print(f"{program}: {len(added)} functions more, "
              f"{sum(sizes.get(name, 0) for name in added) / 1048576:.1f} MiB of code")
    size = sum(sizes.get(name, 0) for name in set(names))
    print(f"{args.output}: {len(names)} functions, {size / 1048576:.1f} MiB of code")
    return 0


if __name__ == "__main__":
    sys.exit(main())
