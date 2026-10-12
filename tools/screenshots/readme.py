#!/usr/bin/env python3
"""The README's screenshots, made again: skwir on an X display of its own,
in a home of synthetic files, captured light and dark.

    just screenshots                    # builds skwir, then runs this
    tools/screenshots/readme.py [--theme light|dark|both] [--skwir B] [--keep]

The window is drawn at twice its size (1280 x 800 points, 2560 x 1600
pixels) with Mesa's software Vulkan, in fonts (Inter, JetBrains Mono,
DejaVu) and icons (Papirus) the flake's nixpkgs provides, so every run
looks the same whatever is installed. The folder shown is generated:
landscape pictures drawn here, a README, a track and a table, some of them
tagged. Each capture is framed as a window (rounded corners, a hairline, a
shadow, on transparency) and written to docs/images/screenshot-<theme>.png.

Nothing personal can reach the pictures: HOME, the XDG folders and the
runtime folder are under ~/.cache/skwir-tests/screenshots-*, and the
sidebar's drives are hidden, since mountinfo lists the machine's real ones
whatever HOME says. A file is selected before the capture, so the preview
panel shows it rather than Details (which says where the folder is and
who owns it). Look at the result before committing it all the same.
"""

import argparse
import itertools
import math
import os
import pathlib
import random
import shutil
import signal
import subprocess
import tempfile
import time

REPO = pathlib.Path(__file__).resolve().parents[2]
# Logical size of the window, and the scale it is drawn at.
WIDTH, HEIGHT, SCALE = 1280, 800, 2
CORNER = 28


def fail(why):
    raise SystemExit(f"screenshots: {why}")


def nix(package):
    """A package of the flake's nixpkgs, built or fetched; its store path."""
    run = subprocess.run(
        ["nix", "build", "--no-link", "--print-out-paths",
         "--inputs-from", f"git+file://{REPO}", f"nixpkgs#{package}"],
        capture_output=True, text=True, check=False,
    )
    if run.returncode != 0:
        fail(f"nix could not build {package}: {run.stderr.strip()}")
    return pathlib.Path(run.stdout.split()[0])


class Tools:
    def __init__(self):
        self.xvfb = nix("xorg.xorgserver") / "bin/Xvfb"
        self.icd = nix("mesa") / "share/vulkan/icd.d/lvp_icd.x86_64.json"
        self.xdotool = nix("xdotool") / "bin/xdotool"
        self.magick = nix("imagemagick") / "bin/magick"
        self.rsvg = nix("librsvg") / "bin/rsvg-convert"
        self.oxipng = nix("oxipng") / "bin/oxipng"
        self.icons = nix("papirus-icon-theme") / "share"
        self.fonts = [nix(p) / "share/fonts" for p in ("inter", "jetbrains-mono", "dejavu_fonts")]


# Landscapes: layered ridges under a gradient sky, drawn as SVG.

PICTURE_W, PICTURE_H = 1600, 1067


def ridge(rng, base, amp, rough, color):
    n = 48
    phase = rng.random() * 10
    points = []
    for i in range(n + 1):
        t = i / n
        y = base - amp * (
            0.55 * math.sin(t * math.pi * 2 * rough + phase)
            + 0.35 * math.sin(t * math.pi * 5.3 * rough + phase * 1.7)
            + 0.25 * (rng.random() - 0.5)
        )
        points.append((PICTURE_W * t, y))
    d = f"M0,{PICTURE_H} L0,{points[0][1]:.1f} "
    for (x0, y0), (x1, y1) in itertools.pairwise(points):
        d += f"Q{x0:.1f},{y0:.1f} {(x0 + x1) / 2:.1f},{(y0 + y1) / 2:.1f} "
    d += f"L{PICTURE_W},{points[-1][1]:.1f} L{PICTURE_W},{PICTURE_H} Z"
    return f'<path d="{d}" fill="{color}"/>'


def stars(seed, n):
    rng = random.Random(seed)
    return "".join(
        f'<circle cx="{rng.random() * PICTURE_W:.0f}" cy="{rng.random() * PICTURE_H * 0.55:.0f}" '
        f'r="{rng.random() * 1.8 + 0.4:.1f}" fill="#fff" fill-opacity="{rng.random() * 0.6 + 0.3:.2f}"/>'
        for _ in range(n)
    )


def scene(seed, sky, sun, layers, extra=""):
    rng = random.Random(seed)
    top, bottom = sky
    parts = [
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{PICTURE_W}" height="{PICTURE_H}">',
        (
            '<defs><linearGradient id="sky" x1="0" y1="0" x2="0" y2="1">'
            f'<stop offset="0" stop-color="{top}"/><stop offset="1" stop-color="{bottom}"/>'
            '</linearGradient><radialGradient id="glow">'
            '<stop offset="0" stop-color="#fff" stop-opacity="0.55"/>'
            '<stop offset="1" stop-color="#fff" stop-opacity="0"/></radialGradient></defs>'
        ),
        f'<rect width="{PICTURE_W}" height="{PICTURE_H}" fill="url(#sky)"/>',
    ]
    if sun:
        cx, cy, r, color = sun
        parts.append(f'<circle cx="{cx}" cy="{cy}" r="{r * 3}" fill="url(#glow)"/>')
        parts.append(f'<circle cx="{cx}" cy="{cy}" r="{r}" fill="{color}"/>')
    parts.append(extra)
    parts += [ridge(rng, *layer) for layer in layers]
    parts.append("</svg>")
    return "\n".join(parts)


AURORA = (
    '<path d="M-50,420 C300,180 700,520 1000,260 S1500,200 1700,320" stroke="#5fffc0" '
    'stroke-width="70" stroke-opacity="0.35" fill="none" stroke-linecap="round"/>'
    '<path d="M-50,360 C350,160 650,460 1050,230 S1500,170 1700,260" stroke="#7affd8" '
    'stroke-width="24" stroke-opacity="0.45" fill="none" stroke-linecap="round"/>'
)

PICTURES = {
    "aurora": scene(7, ("#06141f", "#123c46"), None,
                    [(640, 150, 1.1, "#163b46"), (780, 110, 1.8, "#0f2c36"),
                     (930, 60, 2.5, "#081d25")], stars(7, 180) + AURORA),
    "blue-hour": scene(2, ("#1d2b53", "#5b6fa8"), (420, 330, 46, "#f3f0e0"),
                       [(600, 150, 1.0, "#3c4a7d"), (720, 120, 1.7, "#2c3766"),
                        (860, 80, 2.3, "#1e2650"), (980, 50, 3.0, "#121836")], stars(2, 140)),
    "dawn": scene(1, ("#fbd3a5", "#f39c7a"), (1080, 420, 70, "#fff4dc"),
                  [(560, 140, 1.2, "#c97a7e"), (680, 120, 1.6, "#9a5b74"),
                   (800, 90, 2.1, "#6b3f63"), (930, 60, 2.6, "#3f2a4d")]),
    "dunes": scene(4, ("#ffe7b8", "#f7c27c"), (300, 300, 90, "#fff6e0"),
                   [(700, 90, 0.8, "#eaa45f"), (800, 80, 1.1, "#d9894a"),
                    (900, 70, 1.4, "#c06f3a"), (1000, 40, 1.8, "#9d552c")]),
    "fjord": scene(3, ("#bfe3e8", "#7fb8c9"), None,
                   [(520, 220, 0.9, "#7aa5b3"), (640, 180, 1.4, "#5c8a9b"),
                    (780, 110, 1.9, "#3f6e80"), (900, 40, 0.6, "#2a5466")]),
    "harvest": scene(8, ("#fde2c4", "#f5b38a"), (500, 380, 60, "#fff1dc"),
                     [(620, 100, 0.9, "#d99168"), (740, 80, 1.3, "#c1764f"),
                      (860, 60, 1.7, "#9e5c3f"), (970, 40, 2.2, "#6f3f2d")]),
    "lavender": scene(6, ("#e6dcf5", "#b9a8e0"), (800, 480, 80, "#fff1f6"),
                      [(640, 120, 1.0, "#9f8fcf"), (760, 100, 1.5, "#7d6db8"),
                       (880, 30, 0.4, "#5a4f99"), (980, 20, 0.3, "#3d3675")]),
    "pines": scene(5, ("#d6efd8", "#9fd0b0"), (1200, 260, 55, "#fbfbe9"),
                   [(560, 160, 1.3, "#7fb495"), (680, 130, 1.9, "#5a9a7a"),
                    (810, 100, 2.6, "#3c7d60"), (940, 60, 3.4, "#245c46")]),
}

GUIDE_README = """# Field Guide

Eight days on foot across the high plateau, from the fjord to the
dunes. These notes travel with the photos: **tags stay on the files**,
so every copy keeps them.

## Stages

| Day | To | Walk | Climb |
| --- | --- | ---: | ---: |
| 1 | Pines | 14 km | 620 m |
| 2 | The ridge | 18 km | 1,140 m |
| 3 | The coast | 11 km | 280 m |
| 4 | The dunes | 21 km | 90 m |

## To pack

- [x] Maps, printed and offline
- [x] Water filter
- [ ] Spare batteries for the night shots
"""

ROUTE = """<?xml version="1.0" encoding="UTF-8"?>
<gpx version="1.1" creator="synthetic" xmlns="http://www.topografix.com/GPX/1/1">
  <trk><name>Plateau</name><trkseg>
    <trkpt lat="64.10" lon="-21.90"><ele>12</ele></trkpt>
    <trkpt lat="64.15" lon="-21.70"><ele>340</ele></trkpt>
    <trkpt lat="64.21" lon="-21.52"><ele>610</ele></trkpt>
    <trkpt lat="64.30" lon="-21.31"><ele>820</ele></trkpt>
  </trkseg></trk>
</gpx>
"""

SPECIES = "name,habitat,seen\nArctic tern,coast,3\nRock ptarmigan,plateau,5\nWhooper swan,lakes,2\n"

# Tags in the desktop's attributes, comma separated as Dolphin writes them.
TAGS = {
    "aurora.jpg": "favorite,print",
    "blue-hour.jpg": "print",
    "dawn.jpg": "favorite",
    "pines.jpg": "favorite,print",
    "lavender.jpg": "print",
    "README.md": "draft",
}

# The desktop's folders, by their key in user-dirs.dirs.
USER_DIRS = {
    "DESKTOP": "Desktop",
    "DOCUMENTS": "Documents",
    "DOWNLOAD": "Downloads",
    "MUSIC": "Music",
    "PICTURES": "Pictures",
    "VIDEOS": "Videos",
}


def stamp(path, when):
    t = time.mktime(time.strptime(when, "%Y-%m-%d %H:%M"))
    os.utime(path, (t, t))


class Window:
    def __init__(self, tools, args, theme):
        base = pathlib.Path.home() / ".cache" / "skwir-tests"
        base.mkdir(parents=True, exist_ok=True)
        self.root = pathlib.Path(tempfile.mkdtemp(prefix=f"screenshots-{theme}-", dir=base))
        self.home = self.root / "home"
        self.guide = self.home / "Projects" / "Field Guide"
        self.tools = tools
        self.args = args
        self.theme = theme
        self.procs = []
        self.display = None

    def env(self):
        env = {k: v for k, v in os.environ.items() if k != "WAYLAND_DISPLAY"}
        # Papirus first; the rest as the session has them (the type database).
        system = env.get("XDG_DATA_DIRS") or "/usr/local/share:/usr/share"
        data_dirs = [str(self.tools.icons)] + [d for d in system.split(":") if d]
        env.update(
            DISPLAY=self.display,
            HOME=str(self.home),
            GPUI_X11_SCALE_FACTOR=str(SCALE),
            VK_ICD_FILENAMES=str(self.tools.icd),
            FONTCONFIG_FILE=str(self.root / "fonts.conf"),
            XDG_CONFIG_HOME=str(self.root / "config"),
            XDG_CACHE_HOME=str(self.root / "cache"),
            XDG_STATE_HOME=str(self.root / "state"),
            XDG_DATA_HOME=str(self.root / "data"),
            XDG_DATA_DIRS=":".join(data_dirs),
            XDG_RUNTIME_DIR=str(self.root / "rt"),
        )
        return env

    def make_home(self, pictures):
        for name in [*USER_DIRS.values(), "Projects"]:
            (self.home / name).mkdir(parents=True)
        config = self.root / "config"
        (config / "skwir").mkdir(parents=True)
        (config / "gtk-3.0").mkdir()
        (self.root / "rt").mkdir(mode=0o700)
        (config / "user-dirs.dirs").write_text(
            "".join(f'XDG_{key}_DIR="$HOME/{name}"\n' for key, name in USER_DIRS.items())
        )
        (config / "gtk-3.0" / "bookmarks").write_text(
            (self.home / "Projects").as_uri() + " Projects\n"
        )
        (config / "skwir" / "settings.toml").write_text(
            f'[ui]\ntheme = "{self.theme}"\nicon_theme = "Papirus"\n'
        )
        # The window as it was left is state, not a setting (ADR-056).
        (self.root / "state" / "skwir").mkdir(parents=True, exist_ok=True)
        (self.root / "state" / "skwir" / "state.toml").write_text(
            '[ui.session]\nview = "grid"\nzoom = 96\npreview = true\n'
            "preview_width = 480.0\nsidebar_width = 230.0\n\n"
            '[ui.session.sidebar_sections]\nhidden = ["devices"]\n'
        )
        dirs = "".join(f"  <dir>{d}</dir>\n" for d in self.tools.fonts)
        (self.root / "fonts.conf").write_text(
            '<?xml version="1.0"?>\n<!DOCTYPE fontconfig SYSTEM "fonts.dtd">\n'
            f"<fontconfig>\n{dirs}  <cachedir>{self.root / 'cache' / 'fontconfig'}</cachedir>\n"
            "</fontconfig>\n"
        )

        for sub in ("Drafts", "Maps", "Sketches"):
            (self.guide / sub).mkdir(parents=True)
        for name, jpeg in pictures.items():
            shutil.copy(jpeg, self.guide / f"{name}.jpg")
            stamp(self.guide / f"{name}.jpg", "2026-09-28 09:12")
        shutil.copy(pictures["dunes"], self.guide / "Sketches" / "dunes.jpg")
        shutil.copy(pictures["fjord"], self.guide / "Maps" / "fjord.jpg")
        (self.guide / "README.md").write_text(GUIDE_README)
        stamp(self.guide / "README.md", "2026-10-09 18:40")
        (self.guide / "route.gpx").write_text(ROUTE)
        (self.guide / "species.csv").write_text(SPECIES)
        for name, tags in TAGS.items():
            os.setxattr(self.guide / name, "user.xdg.tags", tags.encode())

    def start_display(self):
        """A display of its own, from :90 up, as tools/test/smoke.py finds one."""
        for number in range(90, 200):
            if pathlib.Path(f"/tmp/.X{number}-lock").exists() or pathlib.Path(
                f"/tmp/.X11-unix/X{number}"
            ).exists():
                continue
            size = f"{WIDTH * SCALE}x{HEIGHT * SCALE}x24"
            xvfb = subprocess.Popen(
                [str(self.tools.xvfb), f":{number}", "-screen", "0", size, "-nolisten", "tcp"],
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

    def xdotool(self, *args):
        run = subprocess.run([str(self.tools.xdotool), *args], env=self.env(),
                             capture_output=True, text=True, check=False)
        return run.stdout.strip() if run.returncode == 0 else None

    def start(self, pictures):
        self.make_home(pictures)
        self.display = self.start_display()
        with open(self.root / "skwir.log", "w") as log:
            window = subprocess.Popen(
                [self.args.skwir, str(self.guide)], env=self.env(),
                stdin=subprocess.DEVNULL, stdout=log, stderr=log, start_new_session=True,
            )
        self.procs.append(window)
        deadline = time.time() + 60
        while True:
            found = self.xdotool("search", "--onlyvisible", "--name", "^skwir$")
            if found:
                self.window = found.split()[0]
                break
            if window.poll() is not None or time.time() > deadline:
                fail(f"the window did not start; see {self.root / 'skwir.log'}")
            time.sleep(0.3)
        self.xdotool("windowmove", self.window, "0", "0")
        self.xdotool("windowsize", self.window, str(WIDTH * SCALE), str(HEIGHT * SCALE))
        # Without a window manager the window gets the keyboard only when asked.
        self.xdotool("windowfocus", self.window)
        self.xdotool("mousemove", str(WIDTH * SCALE - 20), str(HEIGHT * SCALE - 10))

    def capture(self, out):
        """Select README.md by typing its name, then capture once three
        frames a second apart are the same: thumbnails and the preview
        arrive from workers."""
        time.sleep(2)
        self.xdotool("type", "--delay", "120", "README")
        shots = [self.root / "a.png", self.root / "b.png"]
        magick = str(self.tools.magick)
        previous, unchanged = None, 0
        deadline = time.time() + 60
        for turn in range(1000):
            time.sleep(1)
            shot = shots[turn % 2]
            subprocess.run([magick, "import", "-window", "root", str(shot)],
                           env=self.env(), check=True)
            if previous is not None:
                same = subprocess.run([magick, "compare", "-metric", "AE", str(previous),
                                       str(shot), "null:"], capture_output=True, check=False)
                unchanged = unchanged + 1 if same.returncode == 0 else 0
                if unchanged == 2:
                    shutil.copy(shot, out)
                    return
            previous = shot
            if time.time() > deadline:
                fail("the window kept changing for a minute")

    def stop(self):
        for proc in reversed(self.procs):
            if proc.poll() is None:
                os.killpg(proc.pid, signal.SIGTERM)
        for proc in self.procs:
            try:
                proc.wait(timeout=10)
            except subprocess.TimeoutExpired:
                os.killpg(proc.pid, signal.SIGKILL)


def draw_pictures(tools, folder):
    folder.mkdir(parents=True, exist_ok=True)
    out = {}
    for name, svg in PICTURES.items():
        source = folder / f"{name}.svg"
        source.write_text(svg)
        png = subprocess.run([str(tools.rsvg), str(source)], capture_output=True, check=True).stdout
        jpeg = folder / f"{name}.jpg"
        subprocess.run([str(tools.magick), "-", "-quality", "88", str(jpeg)], input=png, check=True)
        out[name] = jpeg
    return out


def frame(tools, raw, out):
    """Rounded corners, a hairline border and a shadow, on transparency."""
    magick = str(tools.magick)
    w, h, r = WIDTH * SCALE, HEIGHT * SCALE, CORNER
    rounded = out.with_suffix(".rounded.png")
    subprocess.run([
        magick, str(raw), "-alpha", "set",
        "(", "+clone", "-alpha", "transparent", "-fill", "white",
        "-draw", f"roundrectangle 0,0 {w - 1},{h - 1} {r},{r}", ")",
        "-compose", "DstIn", "-composite",
        "-fill", "none", "-stroke", "rgba(0,0,0,0.18)", "-strokewidth", "2",
        "-draw", f"roundrectangle 1,1 {w - 2},{h - 2} {r},{r}",
        str(rounded),
    ], check=True)
    subprocess.run([
        magick, str(rounded), "(", "+clone", "-background", "black", "-shadow", "35x30+0+16", ")",
        "+swap", "-background", "none", "-layers", "merge", "+repage", str(out),
    ], check=True)
    rounded.unlink()
    subprocess.run([str(tools.oxipng), "-q", "-o", "4", "--strip", "safe", str(out)], check=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--theme", choices=("light", "dark", "both"), default="both")
    parser.add_argument("--skwir", default=str(REPO / "target/release/skwir"))
    parser.add_argument("--out", default=str(REPO / "docs/images"),
                        help="where screenshot-<theme>.png are written")
    parser.add_argument("--keep", action="store_true", help="leave the last window running")
    args = parser.parse_args()
    if not pathlib.Path(args.skwir).exists():
        fail(f"no skwir at {args.skwir}: `cargo build --release -p skwir`")
    tools = Tools()
    out = pathlib.Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    themes = ("light", "dark") if args.theme == "both" else (args.theme,)
    with tempfile.TemporaryDirectory(prefix="screenshots-pictures-") as scratch:
        pictures = draw_pictures(tools, pathlib.Path(scratch))
        for theme in themes:
            w = Window(tools, args, theme)
            keep = args.keep and theme == themes[-1]
            try:
                w.start(pictures)
                raw = w.root / "raw.png"
                w.capture(raw)
                target = out / f"screenshot-{theme}.png"
                frame(tools, raw, target)
                print(f"screenshots: {target.relative_to(REPO) if target.is_relative_to(REPO) else target}")
            finally:
                if keep:
                    groups = " ".join(f"-{p.pid}" for p in w.procs)
                    print(f"screenshots: kept in {w.root} on DISPLAY={w.display}; "
                          f"stop with: kill -- {groups}")
                else:
                    w.stop()
                    shutil.rmtree(w.root, ignore_errors=True)


if __name__ == "__main__":
    main()
