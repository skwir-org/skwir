#!/usr/bin/env python3
"""Vendor Zed's gpui into vendor/zed, with Skwir's patches (ADR-064).

    vendor.py series <zed checkout> <dir>
        Make <dir> a git repository holding the crates gpui needs, as the
        checkout has them (the baseline commit), then one commit per patch of
        vendor/patches, applied in order. Edit there: amend a patch's commit
        (`git commit --fixup`, then `git rebase -i --autosquash`), or add one
        named "NNNN name: what it does".
    vendor.py export <dir>
        Write each commit after the baseline to vendor/patches/NNNN-name.patch
        and replace vendor/zed with <dir>'s tree (README.skwir.md stays).

An update is `series` on a newer checkout (a patch that no longer applies
stops it: port it in <dir>, commit, then `series --resume`), the review of
what moved, then `export`. The crates taken are those gpui and gpui_platform
reach through path dependencies, build and target ones included, dev ones
not; a dev dependency on a crate not taken is dropped (gpui's examples'
reqwest_client), so the workspace loads. The workspace manifest keeps Zed's
dependency table (the pins, ADR-064) and drops its other members, its
profiles and its `[patch.crates-io]`, whose entries for the crates gpui
reaches Skwir's root manifest repeats: this prints them, to compare.
"""
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tomllib

ROOT = Path(__file__).resolve().parents[2]
VENDOR = ROOT / "vendor/zed"
PATCHES = ROOT / "vendor/patches"
# Files beside the crates: the licence they link to, the toolchain they build with.
TOP_FILES = ["LICENSE-APACHE", "rust-toolchain.toml"]
BASELINE = "the gpui crates, vendored"
# Skwir's own, beside the tree: kept when the tree is replaced.
README = "README.skwir.md"
# What an editor's build of the vendored workspace leaves there, which git
# ignores: kept too, rather than built again.
KEPT = {README, "target", "Cargo.lock"}


def git(cwd, *args, capture=False):
    result = subprocess.run(
        ["git", *args], cwd=cwd, check=True, text=True,
        stdout=subprocess.PIPE if capture else None,
    )
    # Whole: a diff may end with a context line that is a single space.
    return result.stdout if capture else None


def path_dependencies(checkout, crate, workspace_deps):
    """The workspace paths `crate` (a path below the checkout) depends on."""
    manifest = tomllib.loads((checkout / crate / "Cargo.toml").read_text())
    tables = [manifest.get("dependencies", {}), manifest.get("build-dependencies", {})]
    for target in manifest.get("target", {}).values():
        tables += [target.get("dependencies", {}), target.get("build-dependencies", {})]
    found = set()
    for table in tables:
        for name, spec in table.items():
            if not isinstance(spec, dict):
                continue
            if spec.get("workspace"):
                spec = workspace_deps.get(spec.get("package", name), workspace_deps.get(name, {}))
                if not isinstance(spec, dict):
                    continue
                path = spec.get("path")
            else:
                path = spec.get("path") and str((Path(crate) / spec["path"]))
            if path:
                found.add(str(Path(path)))
    return found


def needed_crates(checkout):
    root = tomllib.loads((checkout / "Cargo.toml").read_text())
    workspace_deps = root["workspace"]["dependencies"]
    todo, seen = ["crates/gpui", "crates/gpui_platform"], set()
    while todo:
        crate = todo.pop()
        if crate in seen:
            continue
        seen.add(crate)
        todo += sorted(path_dependencies(checkout, crate, workspace_deps) - seen)
    return sorted(seen)


def drop_dev_dependencies_outside(dir, crates, workspace_deps):
    """Drop, from the vendored crates' manifests, the dev dependencies on a
    crate not vendored (gpui's examples' `reqwest_client`): the workspace
    would not load without it, and `cargo fmt --all` loads it through
    Skwir's path dependency."""
    for crate in crates:
        path = dir / crate / "Cargo.toml"
        out, table, dropped = [], "", []
        for line in path.read_text().splitlines(keepends=True):
            header = re.match(r"^\[([^\[\]]+)\]\s*$", line)
            if header:
                table = header.group(1)
            name = re.match(r"^([A-Za-z0-9_-]+)\s*(\.workspace\s*)?=", line)
            if table.endswith("dev-dependencies") and name:
                spec = workspace_deps.get(name.group(1))
                if (isinstance(spec, dict) and "path" in spec
                        and str(Path(spec["path"])) not in crates):
                    dropped.append(name.group(1))
                    continue
            out.append(line)
        if dropped:
            path.write_text("".join(out))
            print(f"{crate}: dev dependencies not vendored, dropped: {', '.join(dropped)}")


def trimmed_manifest(text, members):
    """Zed's root manifest with `members`, without its other members,
    `default-members`, profiles and `[patch.crates-io]`."""
    out, table, in_members = [], None, False
    for line in text.splitlines():
        header = re.match(r"^\[([^\[\]]+)\]\s*$", line)
        if header:
            table = header.group(1).strip()
        if table == "patch.crates-io" or (table or "").startswith("profile"):
            continue
        if table == "workspace":
            if line.startswith("default-members"):
                continue
            if line.startswith("members = ["):
                in_members = True
                out.append(line)
                out += [f'    "{member}",' for member in members]
                continue
            if in_members:
                in_members = not line.startswith("]")
                if in_members:
                    continue
        out.append(line)
    text = "\n".join(out).rstrip() + "\n"
    return "# Zed's root manifest, trimmed to the crates gpui needs (tools/gpui/vendor.py).\n" + text


def zed_patches(text):
    """The `[patch.crates-io]` lines of Zed's root manifest."""
    lines, inside = [], False
    for line in text.splitlines():
        if re.match(r"^\[.*\]\s*$", line):
            inside = line.strip() == "[patch.crates-io]"
            continue
        if inside and re.match(r"^[A-Za-z0-9_-]+\s*=", line):
            lines.append(line)
    return lines


def patch_files():
    return sorted(p for p in PATCHES.glob("[0-9][0-9][0-9][0-9]-*.patch"))


def apply_patches(dir, done):
    for patch in patch_files():
        if patch.name.split("-")[0] in done:
            continue
        subject = patch.read_text().splitlines()[0]
        print(f"applying {patch.name}")
        if subprocess.run(["git", "apply", "--index", "--3way", str(patch)], cwd=dir).returncode:
            sys.exit(
                f"{patch.name} does not apply: port it in {dir}, commit it as\n"
                f"  {subject}\nthen run `vendor.py series --resume {dir}`"
            )
        git(dir, "commit", "-q", "-m", subject)


def series(checkout, dir):
    checkout = checkout.resolve()
    if dir.exists():
        sys.exit(f"{dir} exists")
    crates = needed_crates(checkout)
    commit = git(checkout, "log", "-1", "--format=%H %cI", capture=True).strip()
    dir.mkdir(parents=True)
    for crate in crates:
        # Zed's crates link their licence from the root: copy what it points at.
        shutil.copytree(checkout / crate, dir / crate, dirs_exist_ok=True)
    for name in TOP_FILES:
        shutil.copy(checkout / name, dir / name)
    manifest = (checkout / "Cargo.toml").read_text()
    (dir / "Cargo.toml").write_text(trimmed_manifest(manifest, crates))
    workspace_deps = tomllib.loads(manifest)["workspace"]["dependencies"]
    drop_dev_dependencies_outside(dir, crates, workspace_deps)
    (dir / "UPSTREAM-COMMIT").write_text(commit + "\n")
    git(dir, "init", "-q", "-b", "main")
    git(dir, "add", "-A")
    git(dir, "commit", "-q", "-m", f"Zed {commit[:12]}: {BASELINE}")
    print(f"{len(crates)} crates from Zed {commit}")
    print("Zed's [patch.crates-io] (Skwir's root manifest repeats those gpui reaches):")
    for line in zed_patches(manifest):
        print("  " + line)
    apply_patches(dir, set())


def resume(dir):
    log = git(dir, "log", "--format=%s", capture=True).splitlines()
    done = {subject.split(" ")[0] for subject in log if re.match(r"^\d{4} ", subject)}
    apply_patches(dir, done)


def export(dir):
    dir = dir.resolve()
    log = git(dir, "log", "--reverse", "--format=%H %s", capture=True).splitlines()
    commits = [line.split(" ", 1) for line in log]
    if not commits or not commits[0][1].endswith(BASELINE):
        sys.exit(f"{dir} does not start with the baseline commit")
    if not (VENDOR / "UPSTREAM-COMMIT").is_file():
        sys.exit(f"{VENDOR} is not a vendored Zed tree")
    # Every patch made before one is replaced: a commit misnamed stops the
    # export with vendor/patches as it was.
    made = []
    for sha, subject in commits[1:]:
        match = re.match(r"^(\d{4}) ([a-z0-9-]+)", subject)
        if not match:
            sys.exit(f"commit {sha[:12]} is not named \"NNNN name: ...\": {subject}")
        diff = git(dir, "diff", "--no-color", f"{sha}^", sha, capture=True)
        made.append((PATCHES / f"{match.group(1)}-{match.group(2)}.patch", f"{subject}\n\n{diff}"))
    for old in patch_files():
        old.unlink()
    for out, text in made:
        out.write_text(text)
        print(f"wrote {out.relative_to(ROOT)}")
    readme = (VENDOR / README).read_text()
    for entry in VENDOR.iterdir():
        if entry.name in KEPT:
            continue
        if entry.is_dir() and not entry.is_symlink():
            shutil.rmtree(entry)
        else:
            entry.unlink()
    shutil.copytree(dir, VENDOR, ignore=shutil.ignore_patterns(".git"), dirs_exist_ok=True)
    (VENDOR / README).write_text(readme)
    print(f"replaced {VENDOR.relative_to(ROOT)}")


def main():
    args = sys.argv[1:]
    if args[:1] == ["series"] and args[1:2] == ["--resume"] and len(args) == 3:
        resume(Path(args[2]))
    elif args[:1] == ["series"] and len(args) == 3:
        series(Path(args[1]), Path(args[2]))
    elif args[:1] == ["export"] and len(args) == 2:
        export(Path(args[1]))
    else:
        sys.exit(__doc__)


if __name__ == "__main__":
    main()
