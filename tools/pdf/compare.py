#!/usr/bin/env python3
"""Compare PDFium's persistent worker with Poppler's per-page CLI renderer.

Renderer timings include pixel output and transfer, but exclude bibli's GPUI
upload and Poppler's PNG disk-cache encoding. PDFium also returns page text and
links. These are different capabilities; this is not an identical-feature test.
Memory samples are child-process PSS, not bibli's total memory or peak RSS.
"""
import argparse
import json
import os
import resource
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


def request(process, value):
    process.stdin.write(json.dumps(value) + "\n")
    process.stdin.flush()
    reply = json.loads(process.stdout.readline())
    assert not ("Error" in reply or "Unavailable" in reply), reply
    return reply


def sample(worker, library, pdf, work, side):
    environment = dict(os.environ, BIBLI_PDFIUM_LIBRARY=str(library))
    start = time.perf_counter()
    process = subprocess.Popen([str(worker), str(pdf), str(work)], env=environment,
                               stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                               stderr=subprocess.DEVNULL, text=True)
    monitor = Peak(process.pid)
    try:
        ready = json.loads(process.stdout.readline())
        assert ready.get("Ready", {}).get("pages") == 3, ready
        opened = (time.perf_counter() - start) * 1000
        timings = []
        for page in [1, 2, 3, 1]:
            start = time.perf_counter()
            reply = request(process, {"Render": {"page": page, "side": side}})["Page"]
            pixels = (work / "page.rgba").read_bytes()
            assert len(pixels) == reply["width"] * reply["height"] * 4
            timings.append((time.perf_counter() - start) * 1000)
        before = time.perf_counter()
        matches = request(process, {"Search": {"query": "needle"}})["Matches"]
        search_ms = (time.perf_counter() - before) * 1000
        used_cpu = cpu(process.pid)
        idle_pss = pss(process.pid)
    finally:
        peak_pss = monitor.close()
        process.stdin.close()
        process.wait(timeout=5)
    assert not Path(f"/proc/{process.pid}").exists()
    poppler, poppler_peak = [], 0
    before_cpu = resource.getrusage(resource.RUSAGE_CHILDREN)
    for page in [1, 2, 3, 1]:
        before = time.perf_counter()
        process = subprocess.Popen(["pdftoppm", "-singlefile", "-f", str(page), "-l", str(page),
                                    "-scale-to", str(side), str(pdf), str(work / "poppler")],
                                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        monitor = Peak(process.pid)
        assert process.wait() == 0
        (work / "poppler.ppm").read_bytes()
        poppler.append((time.perf_counter() - before) * 1000)
        poppler_peak = max(poppler_peak, monitor.close())
    after_cpu = resource.getrusage(resource.RUSAGE_CHILDREN)
    poppler_cpu = (after_cpu.ru_utime + after_cpu.ru_stime - before_cpu.ru_utime - before_cpu.ru_stime) * 1000
    return dict(pdfium_open_ms=opened, pdfium_first_ms=timings[0],
                pdfium_next_ms=statistics.median(timings[1:3]), pdfium_revisit_ms=timings[3],
                pdfium_search_ms=search_ms, pdfium_matches=len(matches), pdfium_cpu_ms=used_cpu,
                pdfium_peak_pss_mib=peak_pss, pdfium_idle_pss_mib=idle_pss,
                poppler_cpu_ms=poppler_cpu, poppler_page_ms=statistics.median(poppler), poppler_peak_pss_mib=poppler_peak,
                closed_worker_exited=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--worker", type=Path, default=Path("target/release/bibli-pdf-worker"))
    parser.add_argument("--library", type=Path, required=True)
    parser.add_argument("--runs", type=int, default=3)
    parser.add_argument("--side", type=int, default=2048)
    args = parser.parse_args()
    result = {"side": args.side, "runs": args.runs, "fixtures": {}}
    with tempfile.TemporaryDirectory(prefix="bibli-pdf-compare-") as directory:
        root = Path(directory)
        for kind in ["text", "vector", "scan"]:
            pdf = root / f"Synthetic {kind}.pdf"
            reader(pdf, kind)
            runs = [sample(args.worker.resolve(), args.library.resolve(), pdf, root, args.side) for _ in range(args.runs)]
            result["fixtures"][kind] = {key: (all(run[key] for run in runs) if isinstance(runs[0][key], bool) else round(statistics.median(run[key] for run in runs), 2)) for key in runs[0]}
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
