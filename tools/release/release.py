#!/usr/bin/env python3
"""Local Linux release gate, bundle and GitHub draft upload (ADR-053)."""

import argparse
import datetime
import fcntl
import hashlib
import json
import os
from pathlib import Path
import platform
import posixpath
import re
import shlex
import shutil
import subprocess
import tempfile
import tomllib
from urllib.parse import quote, unquote
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parents[2]
CHECKS = ["lint-all", "deny", "notices-check", "test-all", "smoke", "poppler-check",
          "mupdf-check", "djvu-check", "bench-check", "release-tools-check"]
PACKAGES = [("appimage", ["package-appimage"]),
            *[(d, ["package-try", d]) for d in ("debian", "ubuntu", "fedora", "arch")]]
# Markdown link targets, inline (after its text's `]`) or defined by reference (not
# a footnote), and code, where neither is one.
LINK = re.compile(r"\]\((?P<inline><[^>\n]*>|[^\s()]+)"
                  r"|^ {0,3}\[(?!\^)[^\]\n]+\]:[ \t]*(?P<defined><[^>\n]*>|\S+)", re.M)
CODE = re.compile(r"^ {0,3}(`{3,}|~{3,})[^\n]*\n.*?(?:^ {0,3}\1[`~]*[ \t]*$|\Z)"
                  r"|(`+).*?(?<!`)\2(?!`)", re.M | re.S)


def require(condition, message):
    if not condition:
        raise ValueError(message)


def run(*args, capture=False, env=None):
    result = subprocess.run(args, cwd=ROOT, check=True, text=True,
                            stdout=subprocess.PIPE if capture else None, env=env)
    return result.stdout.strip() if capture else None


def git(*args):
    return run("git", *args, capture=True)


def metadata(tag):
    require(re.fullmatch(r"v[0-9]+\.[0-9]+\.[0-9]+", tag),
            "Expected a stable release tag: v<major>.<minor>.<patch>")
    version = tag[1:]
    cargo = tomllib.loads((ROOT / "Cargo.toml").read_text())
    require(cargo["workspace"]["package"]["version"] == version,
            "The tag and workspace version disagree")
    for manifest in (ROOT / "crates").rglob("Cargo.toml"):
        package = tomllib.loads(manifest.read_text()).get("package", {})
        require(package.get("version") in (version, {"workspace": True}),
                f"{manifest.relative_to(ROOT)} does not share the release version")
    pkgbuild = (ROOT / "tools/package/aur/PKGBUILD").read_text()
    require(re.search(rf"^pkgver={re.escape(version)}$", pkgbuild, re.M),
            "The PKGBUILD version disagrees")
    changelog = (ROOT / "CHANGELOG.md").read_text()
    section = re.search(rf"^## {re.escape(version)} \((\d{{4}}-\d{{2}}-\d{{2}})\)\n"
                        r"(.*?)(?=^## |\Z)", changelog, re.M | re.S)
    require(section, f"CHANGELOG.md needs a dated section for {version}")
    # The tag holds everything above too: its notes would leave it out.
    first = re.search(r"^## .*$", changelog, re.M)
    require(first.start() == section.start(),
            f"CHANGELOG.md has “{first[0]}” above {version}: fold it into the release's section")
    date = section[1]
    datetime.date.fromisoformat(date)
    notes = section[2].strip() + "\n"
    require(notes.strip(), "Release notes are empty")
    for _, path, _, target in relative_links(notes):
        require(path is not None and (ROOT / path).exists(),
                f"CHANGELOG.md's {version} notes link to a file not in the source: {target}")
    metainfo = ET.parse(ROOT / "tools/package/linux/org.skwir.skwir.metainfo.xml")
    require(any(r.get("version") == version and r.get("date") == date
                for r in metainfo.findall("./releases/release")),
            "AppStream needs the same release version and date as the changelog")
    return version, date, notes


def relative_links(notes):
    """Each link and image in the notes that names a file of the source, outside code.

    Yields its target's span, its path from the root (None when it leaves the
    source), whether it is an image, and the target as written. The changelog is
    at the root, so paths start there.
    """
    code = [m.span() for m in CODE.finditer(notes)]
    for link in LINK.finditer(notes):
        group = "inline" if link["inline"] is not None else "defined"
        start, end = link.span(group)
        target = link[group].removeprefix("<").removesuffix(">")
        if any(a <= start < b for a, b in code) or re.match(r"[A-Za-z][\w+.-]*:|#|//", target):
            continue
        image = False
        if group == "inline":
            # The text's `[`, past any brackets nested in it (an image in a link).
            depth, opening = 0, link.start()
            while opening >= 0:
                depth += {"]": 1, "[": -1}.get(notes[opening], 0)
                if depth == 0:
                    break
                opening -= 1
            if opening < 0:
                continue
            image = notes[opening - 1:opening] == "!"
        path = posixpath.normpath(unquote(target.partition("#")[0]).lstrip("/"))
        outside = path in (".", "..") or path.startswith("../")
        yield (start, end), None if outside else path, image, target


def published(notes, repo, tag):
    """The notes for GitHub's release page, away from the files they link to.

    The changelog keeps its links relative, read in the repository; on the page
    each leads to its file at the tag (an image to the file itself). Paths that
    leave the source never get here: metadata refuses them.
    """
    parts, last = [], 0
    for (start, end), path, image, target in relative_links(notes):
        kind = "raw" if image else "tree" if (ROOT / path).is_dir() else "blob"
        fragment = target.partition("#")[2]
        url = f"https://github.com/{repo}/{kind}/{tag}/{quote(path)}" + (f"#{fragment}" if fragment else "")
        parts += [notes[last:start], url]
        last = end
    return "".join(parts) + notes[last:]


def source(tag):
    commit = git("rev-parse", "HEAD")
    require(git("rev-parse", f"refs/tags/{tag}^{{commit}}") == commit,
            "Check out the release tag before building or uploading")
    require(not git("status", "--porcelain", "--untracked-files=all"),
            "Release source must be clean, including untracked files")
    return commit


def portable(version):
    """The AppImage and the tarball of the same tree (ADR-052)."""
    return [f"skwir-{version}-x86_64.AppImage", f"skwir-{version}-linux-x86_64.tar.gz"]


def patterns(version):
    """Derive split package names from their owning manifests, not a second list."""
    package = tomllib.loads((ROOT / "crates/skwir/Cargo.toml").read_text())["package"]
    result = {"appimage": [rf"{re.escape(name)}{suffix}" for name in portable(version)
                           for suffix in ("", r"\.sha256")]}
    for distro, key, template in (
        ("debian", "deb", r"{}_{version}-[0-9]+_amd64\.deb"),
        # Ubuntu's revision names the release it was made on (deb.sh).
        ("ubuntu", "deb", r"{}_{version}-[0-9]+ubuntu[0-9]+\.[0-9]+_amd64\.deb"),
        ("fedora", "generate-rpm", r"{}-{version}-[0-9]+\.x86_64\.rpm"),
    ):
        config = package["metadata"][key]
        names = [config.get("name", package["name"])]
        names += [v["name"] for v in config.get("variants", {}).values()]
        result[distro] = [template.format(re.escape(n), version=re.escape(version))
                          for n in names]
    pkgbuild = (ROOT / "tools/package/aur/PKGBUILD").read_text()
    name_match = re.search(r"^pkgname=\(([^\n]*)\)$", pkgbuild, re.M)
    revision_match = re.search(r"^pkgrel=([0-9]+)$", pkgbuild, re.M)
    require(name_match and revision_match, "Expected literal pkgname array and numeric pkgrel in PKGBUILD")
    names = shlex.split(name_match[1])
    revision = revision_match[1]
    result["arch"] = [rf"{re.escape(n)}-{re.escape(version)}-{revision}-x86_64\.pkg\.tar\.zst"
                      for n in names]
    return result


def select(directory, expressions):
    files = list(directory.iterdir())
    require(all(p.is_file() and not p.is_symlink() for p in files),
            f"Unexpected directory or symlink in {directory.name}")
    selected = []
    for expression in expressions:
        matches = [p for p in files if re.fullmatch(expression, p.name)]
        require(len(matches) == 1, f"Expected exactly one asset matching {expression}")
        selected.extend(matches)
    require(set(selected) == set(files), f"Unexpected assets in {directory.name}")
    require(len(selected) == len(set(selected)), "Package metadata declares duplicate asset names")
    return selected


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def checksums(directory):
    return "".join(f"{digest(p)}  {p.name}\n" for p in sorted(directory.iterdir())
                   if p.name != "SHA256SUMS")


def verify(directory, tag, commit, version, notes):
    require(directory.is_dir() and not directory.is_symlink(), "No release bundle; run release-build first")
    manifest = json.loads((directory / "manifest.json").read_text())
    require(manifest["schema"] == 1 and manifest["tag"] == tag
            and manifest["version"] == version and manifest["commit"] == commit
            and manifest["date"] == metadata(tag)[1]
            and manifest["checks"] == CHECKS and manifest["platform"] == "linux-x86_64",
            "Bundle identity or required checks disagree with the release")
    expressions = [e for group in patterns(version).values() for e in group]
    expressions += [re.escape(n) for n in ("notes.md", "manifest.json", "SHA256SUMS")]
    files = select(directory, expressions)
    assets = {p.name: {"sha256": digest(p), "bytes": p.stat().st_size} for p in files
              if p.name not in ("manifest.json", "SHA256SUMS")}
    require(assets == manifest["assets"], "An asset changed since the release build")
    require((directory / "notes.md").read_text() == notes, "Bundle notes disagree with the changelog")
    require((directory / "SHA256SUMS").read_text() == checksums(directory), "SHA256SUMS does not match")
    for name in portable(version):
        require((directory / (name + ".sha256")).read_text().strip() == f"{digest(directory / name)}  {name}",
                f"{name}'s companion checksum does not match")
    return files


def build(tag, destination, commit, version, date, notes):
    require(platform.system() == "Linux" and platform.machine() == "x86_64",
            "Local release packaging requires x86-64 Linux")
    require(not destination.exists() and not destination.is_symlink(),
            "A bundle already exists; move it aside before rebuilding")
    expected = patterns(version)
    run("just", "release-check")
    require(source(tag) == commit, "Source changed during release checks")
    # A failed build never replaces a previous bundle or leaves an uploadable one.
    with tempfile.TemporaryDirectory(prefix=f".{tag}-", dir=destination.parent) as work:
        work = Path(work)
        bundle = work / "bundle"
        bundle.mkdir()
        for distro, command in PACKAGES:
            output = work / distro
            output.mkdir()
            run("just", *command, env={**os.environ, "SKWIR_PACKAGE_OUT": str(output)})
            for asset in select(output, expected[distro]):
                shutil.copy2(asset, bundle / asset.name)
        require(source(tag) == commit, "Source changed during packaging")
        (bundle / "notes.md").write_text(notes)
        manifest = {"schema": 1, "tag": tag, "version": version, "date": date,
                    "commit": commit, "platform": "linux-x86_64", "checks": CHECKS,
                    "assets": {p.name: {"sha256": digest(p), "bytes": p.stat().st_size}
                               for p in sorted(bundle.iterdir())}}
        (bundle / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
        (bundle / "SHA256SUMS").write_text(checksums(bundle))
        verify(bundle, tag, commit, version, notes)
        bundle.rename(destination)
    print(f"Verified release bundle: {destination}")


def gh(*args, capture=False):
    # origin chooses github.com; an unrelated GH_HOST must not redirect it.
    return run("gh", *args, capture=capture, env={**os.environ, "GH_HOST": "github.com"})


def gh_json(*args):
    return json.loads(gh(*args, capture=True))


def remote_commit(repo, tag):
    obj = gh_json("api", f"repos/{repo}/git/ref/tags/{tag}")["object"]
    for _ in range(8):
        if obj["type"] == "commit":
            return obj["sha"]
        require(obj["type"] == "tag", "Remote tag does not reference a commit")
        obj = gh_json("api", f"repos/{repo}/git/tags/{obj['sha']}")["object"]
    raise ValueError("Too many nested annotated tags")


def remote_release(repo, tag):
    pages = gh_json("api", "--paginate", "--slurp", f"repos/{repo}/releases?per_page=100")
    releases = [r for page in pages for r in page if r["tag_name"] == tag]
    require(len(releases) <= 1, "More than one release uses this tag")
    if not releases:
        return None
    release = releases[0]
    require(release["draft"] and not release.get("immutable", False),
            "Refusing to change a published or immutable release")
    return release


def remote_assets(repo, release):
    pages = gh_json("api", "--paginate", "--slurp",
                    f"repos/{repo}/releases/{release['id']}/assets?per_page=100")
    return [asset for page in pages for asset in page]


def upload(tag, directory, commit, version, notes):
    files = verify(directory, tag, commit, version, notes)
    origin = git("remote", "get-url", "origin")
    match = re.fullmatch(r"(?:git@github\.com:|ssh://git@github\.com/|https://github\.com/)"
                         r"([\w.-]+/[\w.-]+?)(?:\.git)?", origin)
    require(match, "origin must name the intended GitHub repository (SSH or HTTPS)")
    repo = match[1]
    gh("auth", "status", "--hostname", "github.com", capture=True)
    require(remote_commit(repo, tag) == commit, "Push the exact release tag first; remote commit disagrees")
    # The bundle's notes.md stays the changelog's; only the page gets absolute links.
    post = "--notes=" + published(notes, repo, tag)
    release = remote_release(repo, tag)
    if release:
        require({a["name"] for a in remote_assets(repo, release)} <= {p.name for p in files},
                "Draft contains unexpected assets; review those on GitHub first")
    else:
        gh("release", "create", tag, "--repo", repo, "--draft", "--verify-tag",
            "--title", f"skwir {version}", post)
    gh("release", "upload", tag, "--repo", repo, "--clobber", *map(str, sorted(files)))
    release = remote_release(repo, tag)
    require(release, "The draft disappeared during upload")
    assets = remote_assets(repo, release)
    expected = {p.name: (p.stat().st_size, "sha256:" + digest(p), "uploaded") for p in files}
    actual = {a["name"]: (a["size"], a.get("digest"), a["state"]) for a in assets}
    require(actual == expected and len(assets) == len(expected),
            "GitHub asset names, sizes or SHA256 digests disagree; leave the draft unpublished and retry")
    require(remote_commit(repo, tag) == commit, "Remote tag changed during upload")
    gh("release", "edit", tag, "--repo", repo, "--draft=true",
        "--title", f"skwir {version}", post)
    print(f"Draft updated: https://github.com/{repo}/releases (review and publish manually)")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=("check", "metadata", "build", "verify", "upload"))
    parser.add_argument("tag", nargs="?")
    args = parser.parse_args()
    try:
        if args.command == "check":
            require(args.tag is None, "release-check takes no tag")
            for recipe in CHECKS:
                run("just", recipe)
            return
        require(args.tag, "This command needs a release tag")
        version, date, notes = metadata(args.tag)
        if args.command == "metadata":
            print(f"Release metadata agrees: {args.tag} ({date})")
            return
        commit = source(args.tag)
        releases = ROOT / "target/releases"
        releases.mkdir(parents=True, exist_ok=True)
        with (releases / ".lock").open("w") as lock:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            destination = releases / args.tag
            if args.command == "build":
                build(args.tag, destination, commit, version, date, notes)
            elif args.command == "verify":
                verify(destination, args.tag, commit, version, notes)
                print(f"Release bundle verified: {args.tag}")
            else:
                upload(args.tag, destination, commit, version, notes)
    except (ValueError, KeyError, OSError, ET.ParseError, subprocess.CalledProcessError) as error:
        parser.exit(1, f"Release stopped: {error}\n")


if __name__ == "__main__":
    main()
