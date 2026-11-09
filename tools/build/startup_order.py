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
import subprocess
import sys
import tempfile
import time

REPO = pathlib.Path(__file__).resolve().parents[2]
PT = pathlib.Path("/sys/bus/event_source/devices/intel_pt")
HEADER = """\
# The functions a start of skwir runs, by first execution (ADR-128).
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


def laid_out(by_display):
    """The functions of every display's starts, in the order for the list:
    what all of them run, by first execution, then what each runs alone.
    Mixed, a start on one display would keep windows of the other's code."""
    displays = list(by_display)
    everywhere = set.intersection(*(set(by_display[d]) for d in displays))
    placed = []

    def add(names, first):
        placed.extend(sorted(names, key=lambda name: (first[name], name)))

    add(everywhere, merged(by_display.values()))
    taken = set(everywhere)
    for display in displays:
        alone = set(by_display[display]) - taken
        add(alone, by_display[display])
        taken |= alone
    return placed


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

    def trace(self, binary, display, idle, data, log):
        offset, _, size = text_segment(binary)
        self.run([self.args.perf, "record", "-o", str(data), "-e", "intel_pt/cyc=0/u", "-m,256K",
                  "--filter", f"filter {offset:#x}/{size:#x}@{binary}", "--",
                  str(binary), str(self.folder)], display, idle, log)

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
    events = subprocess.run([perf, "script", "-i", str(data), "--show-mmap-events"],
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
    traces = []  # (display, its data)
    try:
        session.start_bus()
        for kind, start in (("wayland", session.start_wayland), ("x11", session.start_x11)):
            display = start()
            # Once untraced: what follows is a start like any other, with
            # its caches filled, not a first one.
            print(f"{kind}: warming up", flush=True)
            session.run([str(binary), str(session.folder)], display, args.idle, f"{kind}-warm.log")
            for n in range(args.runs):
                print(f"{kind}: start {n + 1} of {args.runs}", flush=True)
                data = session.root / f"{kind}-{n}.perf"
                session.trace(binary, display, args.idle, data, f"{kind}-{n}.log")
                traces.append((kind, data))
        print(f"decoding {len(traces)} traces", flush=True)
        with concurrent.futures.ThreadPoolExecutor(max_workers=6) as pool:
            covers = list(pool.map(lambda trace: decode(args.perf, binary, trace[1]), traces))
    finally:
        session.close()

    by_display = {}
    for (kind, _), cover in zip(traces, covers):
        by_display[kind] = merged([by_display.get(kind, {}), first_runs(syms, cover)])
    names = laid_out(by_display)
    if len(names) < 1000:
        fail(f"only {len(names)} functions seen: the traces did not cover a start")
    write_list(names, pathlib.Path(args.output))
    listed = set(names)
    size = sum(s[1] for s in syms if s[2] in listed)
    print(f"{args.output}: {len(names)} functions, {size / 1048576:.1f} MiB of code")
    return 0


if __name__ == "__main__":
    sys.exit(main())
