#!/usr/bin/env python3
"""Compare persistent PDFium, Poppler and MuPDF workers with identical reader requests.

Renderer timings include metadata extraction and pixel output/transfer, but
exclude skwir's platform manager, GPUI upload and painting. Memory samples are
child-process PSS, not skwir's total memory or peak RSS.
"""
import argparse
import json
import os
from pathlib import Path
import statistics
import subprocess
import tempfile
import threading
import time
from fixtures import reader


def pss(pid):
    try:
        lines = Path(f"/proc/{pid}/smaps_rollup").read_text().splitlines()
        return next(int(line.split()[1]) for line in lines if line.startswith("Pss:")) / 1024
    except (OSError, StopIteration):
        return 0


def cpu(pid):
    parts = Path(f"/proc/{pid}/stat").read_text().split()
    return (int(parts[13]) + int(parts[14])) / os.sysconf("SC_CLK_TCK") * 1000


class Peak:
    def __init__(self, pid):
        self.pid, self.value = pid, 0
        self.stop = threading.Event()
        self.thread = threading.Thread(target=self.sample)
        self.thread.start()

    def sample(self):
        while not self.stop.is_set():
            self.value = max(self.value, pss(self.pid))
            self.stop.wait(.002)

    def close(self):
        self.stop.set()
        self.thread.join()
        return self.value


def reply(process, expected):
    """The next reply, which must answer request `expected` (0 is the greeting)."""
    line = json.loads(process.stdout.readline())
    assert line["id"] == expected, line
    body = line["body"]
    assert not ("Error" in body or "Unavailable" in body), body
    return body


def request(process, value):
    """Requests are numbered from 1 and answered once each, in order."""
    process.sent = getattr(process, "sent", 0) + 1
    process.stdin.write(json.dumps({"id": process.sent, "body": value}) + "\n")
    process.stdin.flush()
    return reply(process, process.sent)


def sample(worker, library, pdf, work, side):
    environment = dict(os.environ, SKWIR_PDFIUM_LIBRARY=str(library))
    start = time.perf_counter()
    process = subprocess.Popen([str(worker), str(pdf), str(work)], env=environment,
                               stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                               stderr=subprocess.DEVNULL, text=True)
    monitor = Peak(process.pid)
    idle_pss = 0
    try:
        ready = reply(process, 0)
        assert ready.get("Ready", {}).get("pages") == 3, ready
        opened = (time.perf_counter() - start) * 1000
        timings = []
        for page in [1, 2, 3, 1]:
            start = time.perf_counter()
            rendered = request(process, {"Render": {"page": page, "side": side}})["Page"]
            pixels = (work / "page.rgba").read_bytes()
            assert len(pixels) == rendered["width"] * rendered["height"] * 4
            timings.append((time.perf_counter() - start) * 1000)
        before = time.perf_counter()
        matches = request(process, {"Search": {"query": "needle"}})["Matches"]
        search_ms = (time.perf_counter() - before) * 1000
        used_cpu = cpu(process.pid)
        idle_pss = pss(process.pid)
    finally:
        peak_pss = max(monitor.close(), idle_pss)
        process.stdin.close()
        process.wait(timeout=5)
    assert not Path(f"/proc/{process.pid}").exists()
    return dict(open_ms=opened, first_ms=timings[0],
                next_ms=statistics.median(timings[1:3]), revisit_ms=timings[3],
                search_ms=search_ms, matches=len(matches), cpu_ms=used_cpu,
                peak_pss_mib=peak_pss, idle_pss_mib=idle_pss,
                closed_worker_exited=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--worker", type=Path, default=Path("target/release/skwir-pdf-worker"))
    parser.add_argument("--poppler-worker", type=Path, default=Path("target/release/skwir-poppler-worker"))
    parser.add_argument("--mupdf-worker", type=Path, default=Path("target/release/skwir-mupdf-worker"))
    parser.add_argument("--engines", default="pdfium,poppler,mupdf",
                        help="comma-separated engines to compare")
    parser.add_argument("--library", type=Path, required=True)
    parser.add_argument("--runs", type=int, default=3)
    parser.add_argument("--side", type=int, default=2048)
    args = parser.parse_args()
    if args.runs < 1 or not 64 <= args.side <= 2048:
        parser.error("runs must be positive and side must be between 64 and 2048")
    result = {"side": args.side, "runs": args.runs, "fixtures": {}}
    with tempfile.TemporaryDirectory(prefix="skwir-pdf-compare-") as directory:
        root = Path(directory)
        for kind in ["text", "vector", "scan"]:
            pdf = root / f"Synthetic {kind}.pdf"
            reader(pdf, kind)
            result["fixtures"][kind] = {}
            workers = {"pdfium": args.worker, "poppler": args.poppler_worker,
                       "mupdf": args.mupdf_worker}
            engines = [(name, workers[name]) for name in args.engines.split(",")]
            samples = {name: [] for name, _ in engines}
            # Rotate the execution order to reduce bias from changing host load
            # and CPU frequency; expose the observed spread, not just a median.
            for index in range(args.runs):
                shift = index % len(engines)
                for engine, worker in engines[shift:] + engines[:shift]:
                    samples[engine].append(sample(worker.resolve(), args.library.resolve(), pdf, root, args.side))
            for engine, runs in samples.items():
                result["fixtures"][kind][engine] = {key: (all(run[key] for run in runs) if isinstance(runs[0][key], bool) else round(statistics.median(run[key] for run in runs), 2)) for key in runs[0]}
                result["fixtures"][kind][engine]["next_ms_range"] = [round(f(run["next_ms"] for run in runs), 2) for f in (min, max)]

    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
