#!/usr/bin/env python3
"""Free the disk of what no build of this workspace uses any more.

    tools/build/prune.py [--dry-run] [--if-below SIZE] [--quiet]
    tools/build/prune.py record (test|clippy|build)

`target/debug` only grows: a unit built with other dependencies or
features gets a new hash beside the old one, an incremental rebuild writes
its split debug info (`.dwo`, `split-debuginfo = "unpacked"`) under new
names and keeps the old, and an incremental session is kept per unit. What
is in use is asked of Cargo, never guessed from times (the root file system
is mounted `noatime`, so access times say nothing, which is why
`cargo-sweep` does not fit):

- `record KIND` notes the units a build uses, as Cargo lists them
  (`--message-format=json`, a build that is up to date: `just test`,
  `just lint` and `just build` record after they ran, nothing compiled);
- pruning keeps every unit a record names, and every unit of a crate only
  the workspace's other members use (the native workers, built apart by
  `just workers`); it removes the others from `deps`, `build` and
  `.fingerprint`, then the `.dwo` no kept artifact names, then the older
  sessions of each incremental unit and the units not rebuilt for three
  days (one recompile of that crate, should it be built again).

With no record of a kind, its units are asked for first (`record`, which
may compile). Nothing is removed while a build runs. `--if-below 40G`
does nothing while the disk has that much free. A build that needs a
removed unit builds it again: nothing is lost but time.
"""
import argparse
import collections
import json
import os
import re
import shutil
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
TARGET = Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "target"))
PROFILE = TARGET / "debug"
RECORDS = TARGET / "prune"

# Each recorded build, as the recipes run it (the same flags, so that it is
# up to date after them and nothing is compiled).
BUILDS = {
    "test": ["cargo", "test", "--no-run"],
    "clippy": ["cargo", "clippy", "--all-targets", "--", "-D", "warnings"],
    "build": ["cargo", "build"],
}

# A unit's artifact: `libname-0123456789abcdef.rlib`, `name-0123….d`, a
# folder `name-0123…` of `build` or `.fingerprint`.
UNIT = re.compile(r"^(?:lib)?([A-Za-z0-9_-]+?)-([0-9a-f]{16})(?:[.].*)?$")
DWO = re.compile(rb"[A-Za-z0-9_]+-[0-9a-f]{16}\.[A-Za-z0-9_.-]*?\.dwo")
# Incremental sessions: `s-<time in base 36>-<random>-<hash>`.
SESSION = re.compile(r"^s-([0-9a-z]+)-([0-9a-z]+)-[0-9a-z]+$")
STALE_SESSION_DAYS = 3


def size_of(text):
    units = {"K": 2**10, "M": 2**20, "G": 2**30, "T": 2**40}
    match = re.fullmatch(r"([0-9.]+)\s*([KMGT])i?B?", text.strip(), re.I)
    if not match:
        raise argparse.ArgumentTypeError(f"{text!r} is no size (40G)")
    return int(float(match.group(1)) * units[match.group(2).upper()])


def free():
    return shutil.disk_usage(TARGET).free


def gib(n):
    return f"{n / 2**30:.1f} GiB"


def building():
    """The builds of this workspace into this target running now, by pid:
    Cargo, nextest, cargo-zigbuild started here, not one writing elsewhere
    (rust-analyzer's `target/flycheck0`, another project)."""
    names = {"cargo", "cargo-nextest", "cargo-zigbuild"}
    found = []
    for entry in Path("/proc").iterdir():
        if not entry.name.isdigit() or int(entry.name) == os.getpid():
            continue
        try:
            comm = (entry / "comm").read_text().strip()
            if comm.lstrip(".").removesuffix("-wrapped") not in names:
                continue
            cwd = Path(os.readlink(entry / "cwd"))
            line = (entry / "cmdline").read_bytes().split(b"\0")
            environ = (entry / "environ").read_bytes().split(b"\0")
        except OSError:
            continue
        if cwd != ROOT and ROOT not in cwd.parents:
            continue
        elsewhere = [e.split(b"=", 1)[1] for e in environ if e.startswith(b"CARGO_TARGET_DIR=")]
        if b"--target-dir" in line or any(Path(e.decode()).resolve() != TARGET.resolve() for e in elsewhere):
            continue
        found.append(int(entry.name))
    return found


def record(kind):
    """Note the units of build `kind`, as Cargo lists them."""
    build = BUILDS[kind]
    at = build.index("--") if "--" in build else len(build)
    command = build[:at] + ["--message-format=json"] + build[at:]
    ran = subprocess.run(
        command, cwd=ROOT, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True
    )
    hashes, compiled = set(), []
    for line in ran.stdout.splitlines():
        try:
            message = json.loads(line)
        except ValueError:
            continue
        reason = message.get("reason")
        paths = []
        if reason == "compiler-artifact":
            if not message.get("fresh", True):
                target = message.get("target", {})
                compiled.append(f"{target.get('name')} ({'/'.join(target.get('kind', []))})")
            paths = message.get("filenames", []) + [message.get("executable") or ""]
        elif reason == "build-script-executed":
            paths = [message.get("out_dir", "")]
        for path in paths:
            found = [m.group(2) for part in Path(path).parts if (m := UNIT.match(part))]
            # A program or an example is reported where Cargo placed it,
            # under its own name: its unit is the file of `deps` (or
            # `examples`) it is a hard link of.
            if not found and path:
                found = linked_unit(Path(path))
            hashes.update(found)
    if ran.returncode != 0 or not hashes:
        sys.exit(f"`{' '.join(BUILDS[kind])}` did not build: nothing recorded")
    RECORDS.mkdir(parents=True, exist_ok=True)
    (RECORDS / f"{kind}.units").write_text("\n".join(sorted(hashes)) + "\n")
    return len(hashes), compiled


def linked_unit(placed):
    """The hash of the unit whose file `placed` (a program Cargo placed in
    the profile's folder) is a hard link of; where none is (Cargo copied
    it), every unit of that name, kept rather than one taken in use: a
    unit's `.dwo` taken from under it never come back on an incremental
    rebuild, which reuses its compiled parts without them."""
    try:
        inode = placed.stat().st_ino
    except OSError:
        return []
    stem = placed.name.replace("-", "_")
    named = []
    for folder in (PROFILE / "deps", PROFILE / "examples"):
        if not folder.is_dir():
            continue
        for candidate in folder.glob(f"{stem}-*"):
            match = UNIT.match(candidate.name)
            if not match:
                continue
            named.append(match.group(2))
            try:
                if candidate.stat().st_ino == inode:
                    return [match.group(2)]
            except OSError:
                pass
    return named


def apart():
    """The crates only the workspace's other members use (the native
    workers and what they alone link), by name: built apart, never judged
    by the default builds."""
    meta = json.loads(
        subprocess.run(
            ["cargo", "metadata", "--format-version", "1", "--offline"],
            cwd=ROOT,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            text=True,
            check=True,
        ).stdout
    )
    graph = {node["id"]: [d["pkg"] for d in node["deps"]] for node in meta["resolve"]["nodes"]}
    names = {p["id"]: p for p in meta["packages"]}

    def reach(roots):
        seen, stack = set(), list(roots)
        while stack:
            node = stack.pop()
            if node in seen:
                continue
            seen.add(node)
            stack.extend(graph.get(node, []))
        return seen

    default = set(meta.get("workspace_default_members") or meta["workspace_members"])
    others = set(meta["workspace_members"]) - default
    only = reach(others) - reach(default)
    crates = set()
    for node in only:
        package = names[node]
        crates.add(package["name"].replace("-", "_"))
        for target in package["targets"]:
            crates.add(target["name"].replace("-", "_"))
    return crates


def stale_units(keep, protected):
    """What of `deps`, `build` and `.fingerprint` no kept unit is."""
    gone = []
    for folder in ("deps", "build", ".fingerprint"):
        place = PROFILE / folder
        if not place.is_dir():
            continue
        for entry in place.iterdir():
            match = UNIT.match(entry.name)
            if not match or match.group(2) in keep:
                continue
            if match.group(1).replace("-", "_") in protected:
                continue
            gone.append(entry)
    return gone


def dead_dwo(going):
    """The `.dwo` of `deps` no artifact left names (those `going` left out):
    an incremental rebuild writes its split debug info under new names and
    keeps the old."""
    deps = PROFILE / "deps"
    going = set(going)
    artifacts = [
        p
        for p in deps.iterdir()
        if p not in going
        and (p.suffix in (".rlib", ".so") or (p.is_file() and "." not in p.name and os.access(p, os.X_OK)))
    ]
    for folder in (PROFILE / "build", PROFILE / "examples"):
        if folder.is_dir():
            artifacts += [p for p in folder.rglob("*") if p.is_file() and "." not in p.name and os.access(p, os.X_OK)]
    named = set()
    if shutil.which("rg"):
        listed = subprocess.run(
            ["rg", "-a", "-o", "-N", "--no-filename", DWO.pattern.decode(), "--", *map(str, artifacts)]
            if artifacts
            else ["true"],
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
        )
        named = set(listed.stdout.decode(errors="replace").split())
    else:
        for artifact in artifacts:
            try:
                named.update(m.decode() for m in DWO.findall(artifact.read_bytes()))
            except OSError:
                pass
    return [p for p in deps.iterdir() if p.suffix == ".dwo" and p.name not in named and p not in going]


def stale_incremental(now):
    """Older sessions of each incremental unit, and units not rebuilt for
    `STALE_SESSION_DAYS` days; never one being written (`-working`)."""
    gone = []
    place = PROFILE / "incremental"
    if not place.is_dir():
        return gone
    for unit in place.iterdir():
        if not unit.is_dir():
            continue
        names = [p.name for p in unit.iterdir()]
        if any(n.endswith("-working") for n in names):
            continue
        sessions = sorted(
            (int(m.group(1), 36), m.group(1), m.group(2), n)
            for n in names
            if (m := SESSION.match(n))
        )
        if not sessions:
            continue
        newest = max((unit / n).stat().st_mtime for *_, n in sessions)
        if now - newest > STALE_SESSION_DAYS * 86400:
            gone.append(unit)
            continue
        for _, stamp, rand, name in sessions[:-1]:
            gone.append(unit / name)
            lock = unit / f"s-{stamp}-{rand}.lock"
            if lock.exists():
                gone.append(lock)
    return gone


def weight(paths):
    total = 0
    for path in paths:
        try:
            if path.is_dir() and not path.is_symlink():
                total += sum(f.lstat().st_size for f in path.rglob("*") if f.is_file())
            else:
                total += path.lstat().st_size
        except OSError:
            pass
    return total


def remove(paths):
    for path in paths:
        try:
            if path.is_dir() and not path.is_symlink():
                shutil.rmtree(path)
            else:
                path.unlink()
        except FileNotFoundError:
            pass


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("action", nargs="?", choices=["record", "prune"], default="prune")
    parser.add_argument("kind", nargs="?", choices=sorted(BUILDS))
    parser.add_argument("--dry-run", action="store_true", help="say what would go, remove nothing")
    parser.add_argument("--if-below", type=size_of, help="only while the disk has less free (40G)")
    parser.add_argument("--quiet", action="store_true", help="say nothing when nothing is done")
    args = parser.parse_args()

    if args.action == "record":
        if not args.kind:
            sys.exit("record what: test, clippy or build")
        units, compiled = record(args.kind)
        if not args.quiet:
            print(f"recorded {units} units of `{args.kind}`" + (f" ({len(compiled)} compiled)" if compiled else ""))
        if compiled and not args.quiet:
            print("  compiled: " + ", ".join(compiled[:12]) + (" …" if len(compiled) > 12 else ""))
        return

    before = free()
    # A runner's target is fresh, holding nothing stale, and its disk is
    # always short: pruned by hand alone there.
    if args.if_below is not None and os.environ.get("CI"):
        return
    if args.if_below is not None and before >= args.if_below:
        if not args.quiet:
            print(f"{gib(before)} free: nothing to prune")
        return
    if not PROFILE.is_dir():
        return
    running = building()
    if running:
        sys.exit(f"a build is running (pids {', '.join(map(str, running))}): nothing pruned")
    missing = [kind for kind in BUILDS if not (RECORDS / f"{kind}.units").exists()]
    automatic = args.if_below is not None
    if missing and not automatic:
        for kind in missing:
            print(f"no record of `{kind}`: asking Cargo (it may compile)")
            record(kind)
        missing = []

    said = []
    units = []
    if missing:
        # Run by a recipe: nothing compiled to find what is used.
        print(f"no record of {', '.join(missing)} yet: units kept (`just prune` records them)")
    else:
        keep = set()
        for kind in BUILDS:
            keep |= set((RECORDS / f"{kind}.units").read_text().split())
        units = stale_units(keep, apart())
        said.append(("units no build uses", units, weight(units)))
    dwo = dead_dwo(units)
    said.append(("debug info no artifact names", dwo, weight(dwo)))
    incremental = stale_incremental(time.time())
    said.append(("incremental sessions and stale units", incremental, weight(incremental)))
    if not args.dry_run:
        for _, paths, _ in said:
            remove(paths)

    for what, paths, size in said:
        if paths:
            kinds = collections.Counter(p.suffix or ("folder" if p.is_dir() else "file") for p in paths)
            print(
                f"{'would remove' if args.dry_run else 'removed'} {len(paths)} {what}"
                f" ({gib(size)} uncompressed): "
                + ", ".join(f"{n} {k}" for k, n in kinds.most_common(4))
            )
    after = free()
    print(f"free: {gib(before)} before, {gib(after)} after" + (" (dry run)" if args.dry_run else ""))


if __name__ == "__main__":
    main()
