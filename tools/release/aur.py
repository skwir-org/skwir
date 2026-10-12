#!/usr/bin/env python3
"""Prepare and publish a verified Linux release's AUR package."""

import argparse
import fcntl
import json
from pathlib import Path
import re
import shlex
import subprocess
import sys
import tempfile

# Importing the shared release checks must not dirty the tagged source tree.
sys.dont_write_bytecode = True
import release

FILES = ("PKGBUILD", ".SRCINFO")


def run(directory, *args):
    return subprocess.run(args, cwd=directory, check=True, text=True,
                          stdout=subprocess.PIPE).stdout.strip()


def git(directory, *args):
    return run(directory, "git", *args)


def head(directory):
    result = subprocess.run(["git", "rev-parse", "--verify", "HEAD"],
                            cwd=directory, capture_output=True, text=True)
    return result.stdout.strip() if result.returncode == 0 else None


def checkout(directory, remote):
    release.require(directory.is_dir() and not directory.is_symlink(),
                    "No AUR checkout; run aur-prepare first")
    release.require(git(directory, "rev-parse", "--show-toplevel") == str(directory),
                    "Expected a separate AUR checkout")
    release.require(git(directory, "remote", "get-url", "origin") == remote
                    and git(directory, "remote", "get-url", "--push", "origin") == remote,
                    "AUR checkout origin disagrees with the package")
    release.require(git(directory, "symbolic-ref", "--short", "HEAD") == "master",
                    "AUR checkout must be on master")


def prepare(tag, commit, template, directory, remote, state):
    if not directory.exists():
        run(directory.parent, "git", "-c", "init.defaultBranch=master",
            "clone", "--", remote, str(directory))
    checkout(directory, remote)
    release.require(not git(directory, "status", "--porcelain", "--untracked-files=all"),
                    "AUR checkout has changes; publish or resolve them before preparing again")
    git(directory, "fetch", "origin")
    refs = git(directory, "for-each-ref", "--format=%(refname)", "refs/remotes/origin/master")
    if refs:
        git(directory, "merge", "--ff-only", "origin/master")
        release.require(head(directory) == git(directory, "rev-parse", "origin/master"),
                        "AUR checkout has unpublished commits; resolve them first")
    else:
        release.require(head(directory) is None, "Expected an empty new AUR repository")

    with tempfile.TemporaryDirectory(prefix=".prepare-", dir=directory.parent) as work:
        work = Path(work)
        (work / "PKGBUILD").write_text(template)
        info = run(work, "makepkg", "--printsrcinfo")
        sources = re.findall(r"^\s+source = (.+)$", info, re.M)
        release.require(len(sources) == 1, "Expected one release source archive")
        url = sources[0].split("::", 1)[-1]
        match = re.fullmatch(r"https://github\.com/([\w.-]+/[\w.-]+)/archive/"
                             + re.escape(tag) + r"\.tar\.gz", url)
        release.require(match, "PKGBUILD source must name this GitHub release tag")
        release.require(release.remote_commit(match[1], tag) == commit,
                        "Push the exact release tag first; remote commit disagrees")
        archive = work / "source.tar.gz"
        run(work, "curl", "--fail", "--location", "--silent", "--show-error",
            "--proto", "=https", "--proto-redir", "=https", "--output", str(archive), url)
        checksum = release.digest(archive)
        sums = re.search(r"^sha256sums=\(([^\n]*)\)$", template, re.M)
        release.require(sums and shlex.split(sums[1]) in (["SKIP"], [checksum]),
                        "Release source checksum disagrees with PKGBUILD")
        pkgbuild = template[:sums.start()] + f"sha256sums=('{checksum}')" + template[sums.end():]
        (work / "PKGBUILD").write_text(pkgbuild)
        info = run(work, "makepkg", "--printsrcinfo") + "\n"
        release.require(not re.search(r"^\s+\w*sums(?:_\w+)? = SKIP$", info, re.M),
                        "AUR source checksums must not be SKIP")
        release.require(release.source(tag) == commit, "Release source changed during preparation")
        (directory / "PKGBUILD").write_text(pkgbuild)
        (directory / ".SRCINFO").write_text(info)
    git(directory, "add", "--", *FILES)
    state.write_text(json.dumps({"tag": tag, "source": commit, "base": head(directory),
                                 "commit": None,
                                 "files": {n: release.digest(directory / n) for n in FILES}},
                                indent=2) + "\n")
    print(f"Prepared AUR files: {directory}")
    print(f"Review: git -C {shlex.quote(str(directory))} diff --cached")
    print(f"Then: just aur-publish {tag}")


def publish(tag, commit, directory, remote, state):
    checkout(directory, remote)
    prepared = json.loads(state.read_text())
    release.require(prepared["tag"] == tag and prepared["source"] == commit,
                    "Preparation belongs to another release; run aur-prepare")
    release.require(all((directory / n).is_file() and not (directory / n).is_symlink()
                        and release.digest(directory / n) == prepared["files"][n] for n in FILES),
                    "Prepared AUR files changed; review and prepare again")
    changes = set()
    for args in (("diff", "--name-only", "-z"), ("diff", "--cached", "--name-only", "-z"),
                 ("ls-files", "--others", "--exclude-standard", "-z")):
        changes.update(filter(None, git(directory, *args).split("\0")))
    release.require(changes <= set(FILES), "AUR checkout contains unrelated changes")
    release.require(head(directory) == (prepared["commit"] or prepared["base"]),
                    "AUR checkout history changed since preparation")
    git(directory, "add", "--", *FILES)
    if git(directory, "diff", "--cached", "--name-only"):
        git(directory, "commit", "-m", f"Update {directory.name} to {tag}", "--", *FILES)
        prepared["commit"] = head(directory)
        state.write_text(json.dumps(prepared, indent=2) + "\n")
    release.require(head(directory), "No AUR commit to publish")
    # A normal push rejects concurrent remote changes. Keep the local commit
    # and preparation record so a failed network/authentication attempt retries.
    git(directory, "push", "origin", "HEAD:refs/heads/master")
    print(f"Published AUR package: https://aur.archlinux.org/pkgbase/{directory.name}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=("prepare", "publish"))
    parser.add_argument("tag")
    args = parser.parse_args()
    try:
        version, _, notes = release.metadata(args.tag)
        commit = release.source(args.tag)
        release.verify(release.ROOT / "target/releases" / args.tag, args.tag, commit, version, notes)
        template = (release.ROOT / "tools/package/aur/PKGBUILD").read_text()
        base = re.search(r"^pkgbase=([a-z0-9][a-z0-9+_.-]*)$", template, re.M)
        release.require(base, "Expected a literal pkgbase in PKGBUILD")
        remote = f"ssh://aur@aur.archlinux.org/{base[1]}.git"
        work = release.ROOT / "target/aur-publish"
        work.mkdir(parents=True, exist_ok=True)
        with (work / ".lock").open("w") as lock:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            directory = work / base[1]
            state = work / "prepared.json"
            if args.command == "prepare":
                prepare(args.tag, commit, template, directory, remote, state)
            else:
                publish(args.tag, commit, directory, remote, state)
    except (ValueError, KeyError, OSError, release.ET.ParseError, subprocess.CalledProcessError) as error:
        parser.exit(1, f"AUR publication stopped: {error}\n")


if __name__ == "__main__":
    main()
