#!/usr/bin/env python3
"""Skwir's apt and dnf repositories (ADR-142, 3): the `.deb` and `.rpm`
packages of a release, as `release-build` made them and `try.sh` tried
them, listed by indexes the repositories' key signs. The indexes go to the
website's static files; the packages stay the release's, which the site
sends each request for to (`skwir-site/static/_redirects`: `/apt/pool/` and
`/rpm/fedora/pool/`), the indexes' SHA-256 checking what arrives.

    tools/package/repository.py target/releases/v0.6.0 --site ../skwir-site/static

needs apt-ftparchive, createrepo_c, gpg and nfpm (the recipe `just
repositories` finds them in the flake's nixpkgs), the repositories' public
key in `tools/package/repository/skwir.asc` (or `--key`) and its secret
part in GnuPG's keyring (`GNUPGHOME`), whose passphrase gpg asks for.

What it writes in the site's static folder, replacing what a release before
wrote there:

    apt/dists/<suite>/     Release, InRelease, Release.gpg, main/binary-amd64/Packages(.gz)
    apt/own/               skwir-repository-debian.deb, skwir-repository-ubuntu.deb
    apt/skwir.asc          the key, as the download page names it
    rpm/fedora/repodata/   createrepo_c's index, repomd.xml signed (repomd.xml.asc)
    rpm/fedora/own/        skwir-repository.rpm
    rpm/skwir.asc

A suite a distribution's release: Debian 13 (`trixie`) takes the `.deb`
built there, Ubuntu 24.04 (`noble`) the one named apart (`-1ubuntu24.04`,
deb.sh), Fedora 44 and later the `.rpm`. Each lists `skwir-repository`, the
package that adds the repository and its key (`sources.list.d`,
`yum.repos.d`), which the site's download page has the visitor install, and
whose update brings a new key: never Skwir's packages, never Skwir.

The packages are not signed one by one: their index is, and it names
their checksums (`repo_gpgcheck`, `Signed-By`). Signing an `.rpm` would
change the file after the release checked and uploaded it.
"""

import argparse
import json
import os
import pathlib
import re
import shutil
import subprocess
import sys
import tempfile

ROOT = pathlib.Path(__file__).resolve().parents[2]
HERE = ROOT / "tools/package/repository"

# Each suite, with what of a release's `.deb` files it takes.
SUITES = {
    "debian": {"suite": "trixie", "label": "Debian 13", "debs": re.compile(r".*_[^_]+-[0-9]+_amd64\.deb")},
    "ubuntu": {"suite": "noble", "label": "Ubuntu 24.04", "debs": re.compile(r".*_[^_]+-[0-9]+ubuntu24\.04_amd64\.deb")},
}
RPMS = re.compile(r".*-[^-]+-[0-9]+\.x86_64\.rpm")


def run(*command, **kwargs):
    return subprocess.run([str(c) for c in command], check=True, **kwargs)


def homepage():
    cargo = (ROOT / "Cargo.toml").read_text()
    return re.search(r'^homepage = "(.*)"$', cargo, re.M).group(1).rstrip("/")


def fingerprint(public):
    """The key's fingerprint, read from its public part."""
    listed = run("gpg", "--batch", "--with-colons", "--show-keys", public, capture_output=True, text=True).stdout
    return next(line.split(":")[9] for line in listed.splitlines() if line.startswith("fpr:"))


def link(source, dest):
    """`dest` the same file as `source` (a hard link, else a copy)."""
    dest.parent.mkdir(parents=True, exist_ok=True)
    try:
        os.link(source, dest)
    except OSError:
        shutil.copy2(source, dest)


def package(work, name, packager, version, contents, description, out):
    """A package of its own made by nfpm: `contents` (source, destination,
    whether it is a configuration file)."""
    config = work / f"{out.stem}.nfpm.json"
    config.write_text(json.dumps({
        "name": name,
        "arch": "all",
        "platform": "linux",
        "version": str(version),
        # As written (`tools/package/repository/version`), not made semver.
        "version_schema": "none",
        "release": "1",
        "maintainer": "The Skwir authors <https://skwir.org>",
        "description": description,
        "vendor": "Skwir",
        "homepage": "https://skwir.org",
        "license": "Apache-2.0",
        "contents": [
            {"src": str(src), "dst": dst, "type": "config|noreplace" if config_file else "file",
             "file_info": {"mode": 0o644}}
            for src, dst, config_file in contents
        ],
    }))
    run("nfpm", "package", "--config", config, "--packager", packager, "--target", out,
        stdout=subprocess.DEVNULL)


def apt_repository(work, bundle, tag, base, public, key, version, site):
    folder = work / "apt"
    keyring = work / "skwir.gpg"
    run("gpg", "--batch", "--yes", "--dearmor", "--output", keyring, public)
    for family, suite in SUITES.items():
        root = work / f"apt-{family}"
        debs = sorted(p for p in bundle.iterdir() if suite["debs"].fullmatch(p.name))
        if not debs:
            sys.exit(f"{bundle}: no {suite['label']} package")
        for deb in debs:
            link(deb, root / "pool" / tag / deb.name)
        sources = work / f"{family}.sources"
        sources.write_text(
            "# Skwir's repository (https://skwir.org/download#repository), added by\n"
            "# the package skwir-repository: removing it removes the repository.\n"
            f"Types: deb\nURIs: {base}/apt\nSuites: {suite['suite']}\nComponents: main\n"
            "Architectures: amd64\nSigned-By: /usr/share/keyrings/skwir.gpg\n")
        own = root / "own" / f"skwir-repository-{family}.deb"
        own.parent.mkdir(parents=True)
        package(work, "skwir-repository", "deb", version,
                [(sources, "/etc/apt/sources.list.d/skwir.sources", True),
                 (keyring, "/usr/share/keyrings/skwir.gpg", False)],
                f"Skwir's repository for {suite['label']}, so the system updates Skwir", own)
        binary = root / "dists" / suite["suite"] / "main/binary-amd64"
        binary.mkdir(parents=True)
        # One folder at a time (what follows a folder is an overrides file),
        # each stanza ending with its blank line.
        packages = b"".join(
            run("apt-ftparchive", "packages", folder, cwd=root, capture_output=True).stdout
            for folder in ("pool", "own"))
        (binary / "Packages").write_bytes(packages)
        run("gzip", "-9nk", binary / "Packages")
        dist = root / "dists" / suite["suite"]
        release = run(
            "apt-ftparchive",
            "-o", "APT::FTPArchive::Release::Origin=Skwir",
            "-o", f"APT::FTPArchive::Release::Label=Skwir for {suite['label']}",
            "-o", f"APT::FTPArchive::Release::Suite={suite['suite']}",
            "-o", f"APT::FTPArchive::Release::Codename={suite['suite']}",
            "-o", "APT::FTPArchive::Release::Architectures=amd64",
            "-o", "APT::FTPArchive::Release::Components=main",
            "release", ".", cwd=dist, capture_output=True).stdout
        (dist / "Release").write_bytes(release)
        run("gpg", "--batch", "--yes", "--local-user", key, "--clearsign", "--output", dist / "InRelease", dist / "Release")
        run("gpg", "--batch", "--yes", "--local-user", key, "--armor", "--detach-sign", "--output", dist / "Release.gpg", dist / "Release")
        link_tree(root / "dists", folder / "dists")
        link_tree(root / "own", folder / "own")
    shutil.copy2(public, folder / "skwir.asc")
    publish(folder, site / "apt", ["dists", "own", "skwir.asc"])


def rpm_repository(work, bundle, tag, base, public, key, version, site):
    folder = work / "rpm"
    root = folder / "fedora"
    rpms = sorted(p for p in bundle.iterdir() if RPMS.fullmatch(p.name))
    if not rpms:
        sys.exit(f"{bundle}: no Fedora package")
    for rpm in rpms:
        link(rpm, root / "pool" / tag / rpm.name)
    repo = work / "skwir.repo"
    repo.write_text(
        "# Skwir's repository (https://skwir.org/download#repository), added by\n"
        "# the package skwir-repository: removing it removes the repository.\n"
        "# Its index is signed and names each package's checksum (repo_gpgcheck).\n"
        f"[skwir]\nname=Skwir\nbaseurl={base}/rpm/fedora\nenabled=1\n"
        "repo_gpgcheck=1\ngpgcheck=0\ngpgkey=file:///etc/pki/rpm-gpg/RPM-GPG-KEY-skwir\n")
    own = root / "own" / "skwir-repository.rpm"
    own.parent.mkdir(parents=True)
    package(work, "skwir-repository", "rpm", version,
            [(repo, "/etc/yum.repos.d/skwir.repo", True),
             (public, "/etc/pki/rpm-gpg/RPM-GPG-KEY-skwir", False)],
            "Skwir's repository for Fedora, so the system updates Skwir", own)
    run("createrepo_c", "--quiet", "--general-compress-type=zstd", root)
    repomd = root / "repodata/repomd.xml"
    run("gpg", "--batch", "--yes", "--local-user", key, "--armor", "--detach-sign", "--output", f"{repomd}.asc", repomd)
    shutil.copy2(public, folder / "skwir.asc")
    publish(folder, site / "rpm", ["fedora/repodata", "fedora/own", "skwir.asc"])


def link_tree(source, dest):
    for path in source.rglob("*"):
        if path.is_file():
            link(path, dest / path.relative_to(source))


def publish(made, into, names):
    """`names` of `made` in `into`, each replacing what was there: an index
    is whole, never mixed with a release's before."""
    for name in names:
        source, dest = made / name, into / name
        if dest.is_dir():
            shutil.rmtree(dest)
        elif dest.exists():
            dest.unlink()
        dest.parent.mkdir(parents=True, exist_ok=True)
        if source.is_dir():
            shutil.copytree(source, dest)
        else:
            shutil.copy2(source, dest)
        print(f"made: {dest}")


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("bundle", type=pathlib.Path)
    parser.add_argument("--site", type=pathlib.Path, required=True,
                        help="the website's static folder (skwir-site/static)")
    parser.add_argument("--key", type=pathlib.Path, default=HERE / "skwir.asc",
                        help="the repositories' public key (its secret part in GnuPG's keyring)")
    parser.add_argument("--base", help="the site's address (by default Cargo.toml's homepage)")
    parser.add_argument("--version", help="skwir-repository's version (by default tools/package/repository/version)")
    args = parser.parse_args()
    bundle = args.bundle.resolve()
    manifest = bundle / "manifest.json"
    if not manifest.is_file():
        sys.exit(f"{bundle}: no release bundle (run `just release-build <tag>` first)")
    if not args.key.is_file():
        sys.exit(f"{args.key}: no key. Make the repositories' key once, then keep its secret part safe:\n"
                 "    gpg --quick-generate-key 'Skwir repositories' ed25519 sign 5y\n"
                 f"    gpg --armor --export <its fingerprint> > {args.key}\n"
                 "and commit the public key (docs/RELEASING.md, Repositories).")
    tag = json.loads(manifest.read_text())["tag"]
    version = args.version or (HERE / "version").read_text().strip()
    base = (args.base or homepage()).rstrip("/")
    key = fingerprint(args.key)
    site = args.site.resolve()
    with tempfile.TemporaryDirectory(prefix="skwir-repositories-") as work:
        work = pathlib.Path(work)
        apt_repository(work, bundle, tag, base, args.key, key, version, site)
        rpm_repository(work, bundle, tag, base, args.key, key, version, site)


if __name__ == "__main__":
    main()
