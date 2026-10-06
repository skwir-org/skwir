#!/usr/bin/env python3
"""Measure bibli's idle memory as ADR-033 budgets it, and its first frame.

Starts the release binary on the display of the current session (the
reference machine's Wayland session for the gate), on a four-entry folder,
with XDG directories of its own: once to fill the caches (the GPU driver's
shader cache, bibli's), then RUNS times. Each start idles IDLE seconds; its
PSS is read from /proc/<pid>/smaps and split by mapping. The budget is on
bibli's own share: everything but the GPU driver's code (the user-space
driver, the Vulkan ICDs and their LLVM), whose share of PSS depends on the
GPU vendor and on how many other GPU clients map it, not on bibli.

    tools/measure/idle-pss.py [--binary target/release/bibli] [--runs 3]

Exits 1 when the median own share is over the budget. Another bibli running
shares the binary's pages and lowers the reading, so it refuses to start
while one does. Each instance it starts is stopped by its own pid.
"""
import argparse
import os
import re
import shutil
import statistics
import subprocess
import sys
import tempfile
import time
from pathlib import Path

OWN_BUDGET_MIB = 100.0  # ADR-033
DRIVER_CODE = re.compile(
    r"/(libnvidia-[^/]*|lib(GLX|EGL)_nvidia[^/]*|libvulkan_[^/]*"
    r"|libLLVM[^/]*|libgallium[^/]*)$"
)
GPU_BUFFERS = re.compile(r"^(/dev/(nvidia|dri)|/memfd:|/dmabuf|anon_inode:dmabuf)")
FIRST_FRAME = re.compile(r"first frame ([0-9.]+) ms")


def category(path, binary):
    if path == "" or path.startswith("[heap") or path.startswith("[anon") \
            or path.startswith("[stack"):
        return "anonymous"
    if path == binary:
        return "binary"
    if DRIVER_CODE.search(path):
        return "GPU driver code"
    if GPU_BUFFERS.search(path):
        return "GPU buffers"
    return "other files and libraries"


def breakdown(pid, binary):
    sums, current = {}, None
    with open(f"/proc/{pid}/smaps") as smaps:
        for line in smaps:
            head = line.split(maxsplit=5)
            if re.match(r"^[0-9a-f]+-[0-9a-f]+$", head[0]):
                current = category(head[5].strip() if len(head) > 5 else "", binary)
            elif head[0] == "Pss:":
                sums[current] = sums.get(current, 0) + int(head[1])
    mib = {name: kb / 1024 for name, kb in sums.items()}
    mib["total"] = sum(mib.values())
    mib["own share"] = mib["total"] - mib.get("GPU driver code", 0.0)
    return mib


def cpu_ticks(pid):
    fields = Path(f"/proc/{pid}/stat").read_text().rsplit(")", 1)[1].split()
    return int(fields[11]) + int(fields[12])


def others_running():
    found = []
    for entry in Path("/proc").iterdir():
        if entry.name.isdigit():
            try:
                if (entry / "comm").read_text().strip() == "bibli":
                    found.append(int(entry.name))
            except OSError:
                pass
    return found


def start(binary, folder, xdg, idle, log):
    env = dict(os.environ, RUST_LOG="bibli_ui=info", NO_COLOR="1",
               XDG_CONFIG_HOME=str(xdg / "config"),
               XDG_CACHE_HOME=str(xdg / "cache"),
               XDG_DATA_HOME=str(xdg / "data"))
    with open(log, "w") as out:
        app = subprocess.Popen([binary, str(folder)], env=env,
                               stdout=out, stderr=subprocess.STDOUT)
    try:
        time.sleep(idle - 5)
        before = cpu_ticks(app.pid)
        time.sleep(5)
        cpu = (cpu_ticks(app.pid) - before) / os.sysconf("SC_CLK_TCK") / 5 * 100
        if app.poll() is not None:
            sys.exit(f"bibli exited during the run; see {log}")
        result = breakdown(app.pid, binary)
        result["idle CPU %"] = cpu
        frame = FIRST_FRAME.search(Path(log).read_text())
        result["first frame ms"] = float(frame.group(1)) if frame else float("nan")
        return result
    finally:
        app.terminate()
        app.wait()


def main():
    # macOS's phys_footprint and Windows' private working set (ADR-038)
    # are read on those machines, in 5b; nothing is measured here.
    if not sys.platform.startswith("linux"):
        print(f"idle-pss: nothing measured on {sys.platform}: ADR-038 says what "
              "this system's measure is, read on its machine (Phase 5b)")
        return 0
    root = Path(__file__).resolve().parents[2]
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--binary", default=str(root / "target/release/bibli"))
    parser.add_argument("--runs", type=int, default=3)
    parser.add_argument("--idle", type=float, default=15.0)
    args = parser.parse_args()
    binary = os.path.realpath(args.binary)
    if others := others_running():
        sys.exit(f"another bibli is running (pid {', '.join(map(str, others))}): "
                 "it shares the binary's pages; close it first")

    base = Path.home() / ".cache/bibli-tests"
    base.mkdir(parents=True, exist_ok=True)
    scratch = Path(tempfile.mkdtemp(prefix="idle-pss-", dir=base))
    try:
        folder = scratch / "folder"
        (folder / "photos").mkdir(parents=True)
        (folder / "notes.txt").write_text("alpha\n")
        (folder / "readme.md").write_text("beta\n")
        (folder / "data.bin").write_bytes(bytes(4096))
        xdg = scratch / "xdg"
        for name in ("config", "cache", "data"):
            (xdg / name).mkdir(parents=True)

        print("warm-up start (fills the caches)")
        start(binary, folder, xdg, args.idle, scratch / "log-0.txt")
        runs = [start(binary, folder, xdg, args.idle, scratch / f"log-{n}.txt")
                for n in range(1, args.runs + 1)]
    finally:
        shutil.rmtree(scratch, ignore_errors=True)

    rows = ["anonymous", "binary", "GPU buffers", "other files and libraries",
            "own share", "GPU driver code", "total"]
    print(f"{'PSS, MiB':28}" + "".join(f"{f'run {n}':>9}" for n in range(1, len(runs) + 1)))
    for row in rows:
        print(f"{row:28}" + "".join(f"{run.get(row, 0.0):9.1f}" for run in runs))
    for row in ("first frame ms", "idle CPU %"):
        print(f"{row:28}" + "".join(f"{run[row]:9.1f}" for run in runs))

    own = statistics.median(run["own share"] for run in runs)
    verdict = "within" if own <= OWN_BUDGET_MIB else "OVER"
    print(f"\nown share, median: {own:.1f} MiB, {verdict} the {OWN_BUDGET_MIB:.0f} MiB budget")
    return 0 if own <= OWN_BUDGET_MIB else 1


if __name__ == "__main__":
    sys.exit(main())
