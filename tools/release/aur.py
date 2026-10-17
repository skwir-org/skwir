#!/usr/bin/env python3
"""Prepare and publish a verified Linux release's AUR packages: skwir,
built from the release's source; skwir-bin, its portable tarball; and
skwir-git, built from main, published again with each release."""

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
# One AUR package base each, in its own checkout.
TEMPLATES = ("tools/package/aur/PKGBUILD", "tools/package/aur/skwir-bin/PKGBUILD",
             "tools/package/aur/skwir-git/PKGBUILD")


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


def sync(directory, remote):
    """The package base's checkout, clean and at the AUR's master."""
    if not directory.exists():
        run(directory.parent, "git", "-c", "init.defaultBranch=master",
            "clone", "--", remote, str(directory))
    checkout(directory, remote)
    release.require(not git(directory, "status", "--porcelain", "--untracked-files=all"),
                    f"AUR checkout has changes ({directory.name}); publish or resolve them before preparing again")
    git(directory, "fetch", "origin")
    refs = git(directory, "for-each-ref", "--format=%(refname)", "refs/remotes/origin/master")
    if refs:
        git(directory, "merge", "--ff-only", "origin/master")
        release.require(head(directory) == git(directory, "rev-parse", "origin/master"),
                        "AUR checkout has unpublished commits; resolve them first")
    else:
        release.require(head(directory) is None, "Expected an empty new AUR repository")


def download(work, url):
    archive = work / "source"
    run(work, "curl", "--fail", "--location", "--silent", "--show-error",
        "--proto", "=https", "--proto-redir", "=https", "--output", str(archive), url)
    checksum = release.digest(archive)
    archive.unlink()
    return checksum


def fill(template, tag, commit, version, bundle, work):
    """The PKGBUILD and .SRCINFO a template makes for this release: its
    source's checksum, the release's own; for a package built from main,
    the version the release's commit gives it."""
    with tempfile.TemporaryDirectory(prefix=".prepare-", dir=work) as work:
        work = Path(work)
        (work / "PKGBUILD").write_text(template)
        info = run(work, "makepkg", "--printsrcinfo")
        sources = re.findall(r"^\s+source = (.+)$", info, re.M)
        release.require(len(sources) == 1, "Expected one release source")
        url = sources[0].split("::", 1)[-1]
        sums = re.search(r"^sha256sums=\(([^\n]*)\)$", template, re.M)
        release.require(sums, "Expected a one-line sha256sums array")
        archive = re.fullmatch(r"https://github\.com/([\w.-]+/[\w.-]+)/archive/"
                               + re.escape(tag) + r"\.tar\.gz", url)
        asset = re.fullmatch(r"https://github\.com/([\w.-]+/[\w.-]+)/releases/download/"
                             + re.escape(tag) + r"/([\w.+-]+)", url)
        main = re.fullmatch(r"git\+https://github\.com/([\w.-]+/[\w.-]+)\.git", url)
        release.require(archive or asset or main,
                        "PKGBUILD source must name this GitHub release tag, one of its assets or main")
        repo = (archive or asset or main)[1]
        release.require(release.remote_commit(repo, tag) == commit,
                        "Push the exact release tag first; remote commit disagrees")
        if main:
            release.require(shlex.split(sums[1]) == ["SKIP"], "A Git source's checksum is SKIP")
            describe = f"{version}.r0.g{commit[:7]}"
            pkgbuild = re.sub(r"^pkgver=.*$", f"pkgver={describe}", template, count=1, flags=re.M)
            pkgbuild = re.sub(r"^pkgrel=.*$", "pkgrel=1", pkgbuild, count=1, flags=re.M)
        else:
            if archive:
                checksum = download(work, url)
            else:
                # The asset GitHub serves is the one the bundle holds.
                release.require((bundle / asset[2]).is_file(), f"{asset[2]} is not a release asset")
                checksum = release.digest(bundle / asset[2])
                release.require(download(work, url) == checksum,
                                f"GitHub's {asset[2]} is not the release bundle's; publish the release first")
            release.require(shlex.split(sums[1]) in (["SKIP"], [checksum]),
                            "Release source checksum disagrees with PKGBUILD")
            pkgbuild = template[:sums.start()] + f"sha256sums=('{checksum}')" + template[sums.end():]
        (work / "PKGBUILD").write_text(pkgbuild)
        info = run(work, "makepkg", "--printsrcinfo") + "\n"
        sources = re.findall(r"^\s+source(?:_\w+)? = (.+)$", info, re.M)
        checksums = re.findall(r"^\s+sha256sums(?:_\w+)? = (.+)$", info, re.M)
        release.require(len(sources) == len(checksums), "Expected one checksum per source")
        # Only a Git source, whose commit names its contents, goes unsummed.
        release.require(all(s != "SKIP" or "git+" in u for u, s in zip(sources, checksums)),
                        "AUR source checksums must not be SKIP")
        return pkgbuild, info


def bases():
    """Each template and the AUR package base it is published as."""
    for path in TEMPLATES:
        template = (release.ROOT / path).read_text()
        base = re.search(r"^pkg(?:base|name)=([a-z0-9][a-z0-9+_.-]*)$", template, re.M)
        release.require(base, f"Expected a literal pkgbase in {path}")
        yield template, base[1]


def remote(base):
    return f"ssh://aur@aur.archlinux.org/{base}.git"


def prepare(tag, commit, version, bundle, work):
    # Every package is made before any checkout is written: one that fails
    # leaves none half prepared.
    made = []
    for template, base in bases():
        sync(work / base, remote(base))
        made.append((work / base, *fill(template, tag, commit, version, bundle, work)))
    release.require(release.source(tag) == commit, "Release source changed during preparation")
    for directory, pkgbuild, info in made:
        (directory / "PKGBUILD").write_text(pkgbuild)
        (directory / ".SRCINFO").write_text(info)
        git(directory, "add", "--", *FILES)
        state(directory).write_text(json.dumps(
            {"tag": tag, "source": commit, "base": head(directory), "commit": None,
             "files": {n: release.digest(directory / n) for n in FILES}}, indent=2) + "\n")
        print(f"Prepared AUR files: {directory}")
        print(f"Review: git -C {shlex.quote(str(directory))} diff --cached")
    print(f"Then: just aur-publish {tag}")


def state(directory):
    return directory.with_name(f"{directory.name}.json")


def publish(tag, commit, work):
    directories = [work / base for _, base in bases()]
    for directory in directories:
        checkout(directory, remote(directory.name))
        release.require(state(directory).is_file(), f"{directory.name} is not prepared; run aur-prepare")
        prepared = json.loads(state(directory).read_text())
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
    for directory in directories:
        prepared = json.loads(state(directory).read_text())
        git(directory, "add", "--", *FILES)
        if git(directory, "diff", "--cached", "--name-only"):
            git(directory, "commit", "-m", f"Update {directory.name} to {tag}", "--", *FILES)
            prepared["commit"] = head(directory)
            state(directory).write_text(json.dumps(prepared, indent=2) + "\n")
        release.require(head(directory), "No AUR commit to publish")
    for directory in directories:
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
        bundle = release.ROOT / "target/releases" / args.tag
        release.verify(bundle, args.tag, commit, version, notes)
        work = release.ROOT / "target/aur-publish"
        work.mkdir(parents=True, exist_ok=True)
        with (work / ".lock").open("w") as lock:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            if args.command == "prepare":
                prepare(args.tag, commit, version, bundle, work)
            else:
                publish(args.tag, commit, work)
    except (ValueError, KeyError, OSError, release.ET.ParseError, subprocess.CalledProcessError) as error:
        parser.exit(1, f"AUR publication stopped: {error}\n")


if __name__ == "__main__":
    main()
