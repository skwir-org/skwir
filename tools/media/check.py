#!/usr/bin/env python3
"""Synthetic GStreamer protocol checks and worker-only playback measurements.

Run in the dev shell after building skwir-media-worker. No personal media used.
Audio uses a clocked fakesink by default; --audio exercises the real audio sink.
--codecs chooses the clip's (H.264 and AAC by default); --wine runs a Windows
build of the worker under Wine (Windows' package check, ADR-076), with the
paths it is given in Windows' form.
"""
import argparse
import json
import os
from pathlib import Path
import queue
import subprocess
import tempfile
import threading
import time


CODECS = {
    "h264-aac": (["-c:v", "libx264", "-preset", "ultrafast", "-crf", "25"], ["-c:a", "aac"]),
    "vp9-opus": (["-c:v", "libvpx-vp9", "-deadline", "realtime", "-cpu-used", "8", "-b:v", "2M"],
                 ["-c:a", "libopus"]),
    "av1-vorbis": (["-c:v", "libsvtav1", "-preset", "12"], ["-c:a", "libvorbis"]),
}


def generate(root, size="1920x1080", codecs="h264-aac"):
    root.mkdir(parents=True, exist_ok=True)
    subtitles = root / "synthetic.srt"
    subtitles.write_text("1\n00:00:00,000 --> 00:00:12,000\nSynthetic subtitles\n")
    suffix = "" if codecs == "h264-aac" else f" {codecs}"
    clip = root / ("Synthetic clip's " + size + suffix + ".mkv")
    video, audio = CODECS[codecs]
    if not clip.exists():
        subprocess.run([
            "ffmpeg", "-nostdin", "-v", "error", "-f", "lavfi", "-i",
            f"testsrc2=size={size}:rate=30", "-f", "lavfi", "-i",
            "sine=frequency=440:sample_rate=48000", "-f", "lavfi", "-i",
            "sine=frequency=660:sample_rate=48000", "-i", str(subtitles),
            "-map", "0:v", "-map", "1:a", "-map", "2:a", "-map", "3:s",
            "-t", "12", *video, "-threads", "2", *audio, "-c:s", "srt",
            "-metadata:s:a:0", "language=eng", "-metadata:s:a:1", "language=fra",
            "-metadata:s:s:0", "language=eng", str(clip),
        ], check=True)
    return clip


def windows_path(path):
    return subprocess.run(["winepath", "-w", str(path)], check=True, capture_output=True,
                          text=True).stdout.strip()


class Worker:
    def __init__(self, executable, clip, audio=False, wine=False):
        self.scratch = tempfile.TemporaryDirectory(prefix="skwir-media-check-", dir="/dev/shm")
        self.slot = Path(self.scratch.name) / "video.bgra"
        with self.slot.open("wb") as out:
            out.truncate(1920 * 1920 * 4)
        env = dict(os.environ)
        # Match the session manager's glibc allocation policy.
        env["MALLOC_ARENA_MAX"] = "2"
        if audio:
            env.pop("SKWIR_MEDIA_TEST_AUDIO", None)
        else:
            env["SKWIR_MEDIA_TEST_AUDIO"] = "1"
        command = [executable, str(clip), self.scratch.name]
        if wine:
            command = ["wine", executable, windows_path(clip), windows_path(self.scratch.name)]
        self.process = subprocess.Popen(command,
                                        stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                        stderr=None if os.environ.get("GST_DEBUG") else subprocess.DEVNULL, text=True, env=env)
        self.messages = queue.Queue()
        self.generation = 0
        self.state = {}
        self.frames = 0
        self.frame = None
        self.lateness = []
        self.dropped = None
        self.last_generation = None
        self.started = time.monotonic()
        threading.Thread(target=self.read, daemon=True).start()

    def read(self):
        for line in self.process.stdout:
            # Wine may write lines of its own; the worker's are JSON.
            if line.lstrip().startswith("{"):
                self.messages.put(json.loads(line))
        self.messages.put({"Error": "worker exited"})

    def send(self, command):
        self.process.stdin.write(json.dumps(command) + "\n")
        self.process.stdin.flush()

    def control(self, command):
        self.generation += 1
        self.send({"Control": {"generation": self.generation, "command": command}})

    def receive(self, timeout=20):
        message = self.messages.get(timeout=timeout)
        if "Error" in message or "Unavailable" in message or "Rejected" in message:
            # Every control the check sends is valid for its synthetic clip.
            raise AssertionError(message)
        if "State" in message:
            if message["State"]["generation"] == self.generation:
                self.state = message["State"]["playback"]
        if "Frame" in message:
            frame = message["Frame"]
            self.last_generation = frame["generation"]
            self.dropped = frame.get("dropped")
            if frame.get("seconds") is not None and frame.get("clock") is not None:
                self.lateness.append((frame["clock"] - frame["seconds"]) * 1000)
            assert 0 < frame["width"] <= 1920 and 0 < frame["height"] <= 1920
            with self.slot.open("rb") as src:
                self.frame = src.read(frame["width"] * frame["height"] * 4)
            assert len(self.frame) == frame["width"] * frame["height"] * 4
            self.frames += 1
            self.send("Consumed")
        return message

    def seek(self, position):
        self.control({"Seek": position})
        generation = self.generation
        got_state = False
        got_frame = False
        deadline = time.monotonic() + 20
        while not (got_state and got_frame):
            message = self.receive(max(.01, deadline - time.monotonic()))
            if "State" in message and message["State"]["generation"] == generation:
                actual = message["State"]["playback"]["position"]
                assert abs(actual - position) < .5, ("seek position bounced", position, actual)
                got_state = True
            if "Frame" in message and message["Frame"]["generation"] == generation:
                actual = message["Frame"]["seconds"]
                assert abs(actual - position) < .5, ("stale seek frame", position, actual)
                got_frame = True
            assert time.monotonic() < deadline, "seek timed out"

    def until(self, condition, timeout=20):
        deadline = time.monotonic() + timeout
        while not condition():
            self.receive(max(.01, deadline - time.monotonic()))
            assert time.monotonic() < deadline, "playback condition timed out"

    def close(self):
        self.process.stdin.close()
        try:
            self.process.wait(timeout=3)
        finally:
            if self.process.poll() is None:
                self.process.kill()
                self.process.wait()
            self.scratch.cleanup()


def pss(pid):
    return next(int(line.split()[1]) / 1024 for line in
                Path(f"/proc/{pid}/smaps_rollup").read_text().splitlines() if line.startswith("Pss:"))


def ticks(pid):
    stat = Path(f"/proc/{pid}/stat").read_text().split()
    return int(stat[13]) + int(stat[14])


def check(executable, clip, audio=False, wine=False):
    worker = Worker(executable, clip, audio, wine)
    try:
        worker.until(lambda: worker.frames and worker.state.get("duration"))
        startup = time.monotonic() - worker.started
        assert worker.state["status"] != "Playing"
        assert len(worker.state["audio_tracks"]) == 2
        assert len(worker.state["subtitle_tracks"]) == 1
        first = worker.frame
        before = worker.frames
        worker.seek(4.)
        worker.until(lambda: worker.frames > before and worker.last_generation == worker.generation)
        assert worker.frame != first, "paused seek did not update pixels"
        # Both directions, repeated requests before the prior seek settles.
        for target in (8., 2., 7., 4.):
            worker.control({"Seek": 10.})
            worker.seek(target)
        worker.control({"Audio": 1})
        worker.until(lambda: worker.state.get("audio") == 1 and worker.last_generation == worker.generation)
        without_subtitles = worker.frame
        for _ in range(3):
            worker.control({"Subtitle": 0})
            worker.until(lambda: worker.state.get("subtitle") == 0 and worker.last_generation == worker.generation)
            assert worker.frame != without_subtitles, "subtitle selection did not change paused pixels"
            worker.control({"Subtitle": -1})
            worker.until(lambda: worker.state.get("subtitle") == -1 and worker.last_generation == worker.generation)
            assert worker.frame == without_subtitles, "disabling subtitles did not restore the clean frame"
        worker.control({"Subtitle": 0})
        worker.until(lambda: worker.state.get("subtitle") == 0 and worker.last_generation == worker.generation)
        worker.control({"Volume": .25})
        worker.until(lambda: worker.state.get("volume") == .25)
        worker.control({"Mute": True})
        worker.until(lambda: worker.state.get("muted"))
        worker.control({"Rate": 1.5})
        worker.until(lambda: worker.state.get("rate") == 1.5)
        worker.control({"Play": True})
        # GStreamer state changes and flushing seeks complete asynchronously.
        # Start clock comparisons after preroll, not at requested Play intent.
        worker.until(lambda: worker.state.get("status") == "Playing" and worker.state["position"] > 4.4)
        speed_start = time.monotonic()
        speed_position = worker.state["position"]
        worker.until(lambda: time.monotonic() - speed_start >= 1.)
        speed_elapsed = time.monotonic() - speed_start
        assert abs(worker.state["position"] - speed_position - 1.5 * speed_elapsed) < .5
        worker.control({"Play": False})
        worker.until(lambda: worker.state["status"] != "Playing")
        worker.control({"Rate": 1.})
        worker.until(lambda: worker.state.get("rate") == 1.)
        worker.control({"Seek": 4.})
        before_seek = worker.frames
        worker.until(lambda: worker.frames > before_seek and worker.last_generation == worker.generation)
        worker.control({"Play": True})
        worker.until(lambda: worker.state.get("status") == "Playing" and 4.4 < worker.state["position"] < 6.)
        worker.seek(8.)
        worker.seek(4.)
        before = worker.frames
        start = time.monotonic()
        worker.lateness = []
        tick_start = ticks(worker.process.pid)
        initial_position = worker.state["position"]
        worker.until(lambda: time.monotonic() - start >= 3.)
        elapsed = time.monotonic() - start
        advanced = worker.state["position"] - initial_position
        assert abs(advanced - elapsed) < .5, (advanced, elapsed)
        stats = {"input": clip.stem.split()[-1], "first_frame_ms": round(startup * 1000),
                 "delivered_fps": round((worker.frames - before) / elapsed, 1),
                 "worker_pss_mib": round(pss(worker.process.pid), 1),
                 "worker_dropped": worker.dropped,
                 "frame_lateness_p95_ms": round(sorted(worker.lateness)[int(len(worker.lateness) * .95)], 1),
                 "worker_cpu_percent": round((ticks(worker.process.pid) - tick_start) /
                                             os.sysconf("SC_CLK_TCK") * 100 / elapsed, 1)}
        # Track changes must also resume an already playing pipeline.
        position = worker.state["position"]
        worker.control({"Audio": 0})
        worker.until(lambda: worker.state.get("audio") == 0 and worker.state["position"] > position + .25)
        assert worker.state["status"] == "Playing"
        # Two track changes in a row while playing, the second sent before the
        # first has prerolled: it must not select from the first one's stream
        # collection, or the video never comes back (Player::fence).
        for track in (1, 0):
            worker.control({"Audio": track})
            worker.until(lambda: worker.state.get("audio") == track)
        worker.seek(8.)
        before = worker.frames
        worker.until(lambda: worker.frames > before + 10 and worker.last_generation == worker.generation)
        assert worker.state["status"] == "Playing"
        worker.control({"Play": False})
        worker.until(lambda: worker.state["status"] != "Playing")
        worker.control({"Seek": 11.5})
        worker.control({"Play": True})
        worker.until(lambda: (worker.state.get("status") == "Ended"))
        worker.control({"Play": True})
        worker.until(lambda: (worker.state.get("status") == "Playing") and not (worker.state.get("status") == "Ended") and worker.state["position"] < 2)
        print(json.dumps(stats), flush=True)
    finally:
        worker.close()


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--worker", default="target/debug/skwir-media-worker")
    parser.add_argument("--fixtures", type=Path)
    parser.add_argument("--size", default="1920x1080")
    parser.add_argument("--audio", action="store_true")
    parser.add_argument("--codecs", choices=sorted(CODECS), default="h264-aac")
    parser.add_argument("--wine", action="store_true")
    parser.add_argument("--generate-only", action="store_true")
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="skwir-synthetic-video-") as temporary:
        clip = generate(args.fixtures or Path(temporary), args.size, args.codecs)
        if not args.generate_only:
            check(args.worker, clip, args.audio, args.wine)
