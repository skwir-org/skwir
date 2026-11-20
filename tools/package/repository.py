#!/usr/bin/env python3
"""Skwir's apt and dnf repositories (ADR-142, 3): the `.deb` and `.rpm`
packages of a release, as `release-build` made them and `try.sh` tried
them, listed by indexes the repositories' key signs. The indexes go to the
website's static files; the packages stay the release's, which the site
sends each request for to (`skwir-site/static/_redirects`: `/apt/pool/` and
`/rpm/fedora/pool/`), the indexes' SHA-256 checking what arrives.

    tools/package/repository.py target/releases/v0.6.0 --site ../skwir-site/static

needs apt-ftparchive, createrepo_c, gpg, gpgv, nfpm, rpmkeys and rpmsign
(the recipe `just repositories` finds them in the flake's nixpkgs), the
repositories' public
key in `tools/package/repository/skwir.asc` (or `--key`) and its secret
part in GnuPG's keyring (`GNUPGHOME`), whose passphrase gpg asks for; while
the key changes, the key coming in `tools/package/repository/next.asc` (or
`--next`).

    tools/package/repository.py --check-key

checks the keys alone, as a publication starts by checking them, and prints
the fingerprint that signs: `release-build` runs it before its hour of
checks, as it signs Fedora's packages with that key.

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

Each index is signed and names its packages' checksums (`Signed-By`,
`repo_gpgcheck`). Each `.rpm` is signed too, by the key that signs the
indexes, and dnf checks it (`gpgcheck=1`): PackageKit, under GNOME Software
and Discover, installs only what a key it trusts signed. `release-build`
signs the release's (`SKWIR_RPM_KEY`, tools/package/rpm-sign.sh) before it
tries and uploads them, and a release whose `.rpm` that key did not sign is
refused here. apt trusts a package by its signed index, and a `.deb` stays
as built. A bundle whose packages are not those its manifest records is
refused, as `release-verify` refuses it: an index would name another file
than the one the release serves.

The indexes are signed by the key of `skwir.asc` alone (dnf refuses a
signature beside another), whose secret part must be in the keyring.
`skwir-repository` carries it, and the key of `next.asc` while the key
changes: a change is two versions of it, the key coming carried beside the
one that signs, then, a grace period later, the new key signing alone
(`docs/RELEASING.md`). Each key must be valid for 180 days yet: one expired
on a system refuses every index, the update that would extend it included.
The key that signs must sign here, and what it signs be read back with its
file alone (`proven`), before anything is signed.

rpm keeps a key as dnf first imported it, so `skwir-repository`'s `.rpm`
carries tools/package/repository/keys.sh, which its `%posttrans` starts
once the transaction is done: each key it carries that rpm holds imported
again (an extension), each key `tools/package/repository/retired` names
(`--retired`) let go of, by rpm and dnf's keys for the index.

`skwir-repository` is dated by the last commit of its version file
(`tools/package/repository/version`), its `.rpm`'s signature too, whatever
`SOURCE_DATE_EPOCH` the shell sets, so it is made again identical until its
version is raised. What it holds changing raises the version: a version the
site already holds, made again with other bytes, is refused, and so are its
version file and keys uncommitted. Both repositories are made before either
is written into the site.
"""

import argparse
import datetime
import hashlib
import io
import json
import os
import pathlib
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile

ROOT = pathlib.Path(__file__).resolve().parents[2]
HERE = ROOT / "tools/package/repository"

# Each suite, with what of a release's `.deb` files it takes.
SUITES = {
    "debian": {"suite": "trixie", "label": "Debian 13", "debs": re.compile(r".*_[^_]+-[0-9]+_amd64\.deb")},
    "ubuntu": {"suite": "noble", "label": "Ubuntu 24.04", "debs": re.compile(r".*_[^_]+-[0-9]+ubuntu24\.04_amd64\.deb")},
}
RPMS = re.compile(r".*-[^-]+-[0-9]+\.x86_64\.rpm")
# How long a key must be valid yet to sign: its extension has to reach every
# system, as an update of skwir-repository, before it expires there.
LEAD = datetime.timedelta(days=180)


def run(*command, check=True, **kwargs):
    return subprocess.run([str(c) for c in command], check=check, **kwargs)


def homepage():
    cargo = (ROOT / "Cargo.toml").read_text()
    return re.search(r'^homepage = "(.*)"$', cargo, re.M).group(1).rstrip("/")


def digest(path):
    sha256 = hashlib.sha256()
    with path.open("rb") as file:
        for block in iter(lambda: file.read(1 << 20), b""):
            sha256.update(block)
    return sha256.hexdigest()


def released(bundle):
    """The bundle's manifest, once its packages are the release's: each `.deb`
    and `.rpm` it records is there with its size and SHA-256, and there is
    none it does not record (what `release-verify` checks of them)."""
    manifest = json.loads((bundle / "manifest.json").read_text())
    assets = manifest["assets"]
    packages = (".deb", ".rpm")
    recorded = {name for name in assets if name.endswith(packages)}
    present = {path.name for path in bundle.iterdir() if path.name.endswith(packages)}
    if missing := sorted(recorded - present):
        sys.exit(f"{bundle}: {', '.join(missing)}: recorded by its manifest.json, and missing")
    if stray := sorted(present - recorded):
        sys.exit(f"{bundle}: {', '.join(stray)}: not a file the release built (its manifest.json)")
    for name in sorted(recorded):
        path = bundle / name
        if path.stat().st_size != assets[name]["bytes"] or digest(path) != assets[name]["sha256"]:
            sys.exit(f"{path}: changed since the release built it (its manifest.json): the indexes would name"
                     " another file than the one the release serves")
    return manifest


def keys(listing):
    """Each key of a `gpg --with-colons --show-keys` listing: its primary
    fingerprint and until when it signs (seconds since the epoch, None for
    never): the primary's expiry, or sooner the last expiry of its parts
    that sign (the primary where it signs, each subkey that does, gpg
    signing with one while it is valid)."""
    found = []
    for line in listing.splitlines():
        fields = line.split(":")
        expires = int(fields[6]) if len(fields) > 6 and fields[6] else None
        if fields[0] == "pub":
            found.append([None, expires, [expires] if "s" in fields[11] else []])
        elif fields[0] == "fpr" and found and found[-1][0] is None:
            found[-1][0] = fields[9]
        elif fields[0] == "sub" and found and fields[1] not in ("e", "r") and "s" in fields[11]:
            found[-1][2].append(expires)
    said = []
    for fingerprint, primary, parts in found:
        last = None if not parts or None in parts else max(parts)
        said.append((fingerprint, min((e for e in (primary, last) if e is not None), default=None)))
    return said


def unfit(signing, coming, secret, now):
    """Why the key that signs and the key coming (each a fingerprint and an
    expiry, as `keys` reads them; no key coming but while the key changes)
    cannot be used at `now`, `secret` whether the keyring has the secret
    part of the key that signs: it must, the key coming must be another,
    and each must be valid for `LEAD` yet."""
    said = []
    if not secret:
        said.append(f"{signing[0]}: its secret part is not in GnuPG's keyring, and it signs the indexes")
    if coming and coming[0] == signing[0]:
        said.append(f"{coming[0]}: the key coming is the key that signs")
    for fingerprint, expires in [signing, coming] if coming else [signing]:
        if expires is not None:
            when = datetime.datetime.fromtimestamp(expires, datetime.UTC)
            if when - now < LEAD:
                said.append(f"{fingerprint} expires on {when:%Y-%m-%d}: extend it"
                            f" (gpg --quick-set-expire {fingerprint} 5y, and its subkeys that sign with '*'),"
                            " export it again, raise tools/package/repository/version and commit both, so"
                            " that every system has it before it expires (docs/RELEASING.md)")
    return said


def unsigned(rpms, public):
    """The `.rpm` files of `rpms` that the key of `public` did not sign, as
    rpm reads them with that key alone, each with why: the key ID that
    signed it, none, or a digest that fails."""
    refused = {}
    with tempfile.TemporaryDirectory(prefix="skwir-rpmkeys-") as keyring:
        run("rpmkeys", "--dbpath", keyring, "--import", public)
        for rpm in rpms:
            said = run("rpmkeys", "--dbpath", keyring, "--checksig", "--verbose", rpm, capture_output=True,
                       text=True, check=False, env={**os.environ, "LC_ALL": "C"}).stdout
            # Each signature and digest, then its state: all OK, one a signature.
            states = re.findall(r"^\s+(.*): (\w+)$", said, re.M)
            signed = [re.search(r"Signature, key ID ([0-9a-f]+)$", what) for what, _ in states]
            if other := next((m[1] for m, (_, state) in zip(signed, states) if m and state != "OK"), None):
                refused[rpm] = f"signed by key ID {other}"
            elif not any(signed):
                refused[rpm] = "unsigned"
            elif any(state != "OK" for _, state in states):
                refused[rpm] = "a digest fails"
    return refused


def key_of(public):
    """The key `public` holds, one and no more: its fingerprint and expiry."""
    listing = run("gpg", "--batch", "--with-colons", "--show-keys", public, capture_output=True, text=True).stdout
    found = keys(listing)
    if len(found) != 1:
        sys.exit(f"{public}: {len(found)} keys, where each file keeps one (the key coming in its own,"
                 " docs/RELEASING.md)")
    return found[0]


def proven(fingerprint, public):
    """Refuse, before anything is signed, a key gpg cannot sign with here, as
    the indexes and the `.rpm` files are signed (`--local-user`), or whose
    signature `public` alone does not read, as systems hold it: a secret part
    kept elsewhere (a stub), a passphrase refused or not given, a part that
    signs and that `public` lacks. gpg asks for the passphrase now, while
    whoever publishes is there; gpg-agent keeps it a while."""
    with tempfile.TemporaryDirectory(prefix="skwir-key-") as folder:
        said, signature, keyring = (pathlib.Path(folder) / name for name in ("said", "said.sig", "key.gpg"))
        said.write_text("Skwir's repositories: what their key signs, read back with its public key alone\n")
        signed = run("gpg", "--batch", "--yes", "--local-user", fingerprint, "--detach-sign", "--output", signature,
                     said, check=False, capture_output=True, text=True)
        if signed.returncode:
            reason = (signed.stderr.strip().splitlines() or ["no reason given"])[-1]
            sys.exit(f"{fingerprint}: gpg cannot sign with it here ({reason}): its secret part must be in GnuPG's"
                     " keyring, not kept elsewhere, and its passphrase given when gpg asks (docs/RELEASING.md)")
        keyring.write_bytes(run("gpg", "--batch", "--dearmor", input=public.read_bytes(), capture_output=True).stdout)
        read = run("gpgv", "--keyring", keyring, signature, said, check=False, capture_output=True, text=True)
        if read.returncode:
            reason = (read.stderr.strip().splitlines() or ["no reason given"])[-1]
            sys.exit(f"{public}: does not read what {fingerprint} signs ({reason}), and systems hold that file:"
                     f" export the key again (gpg --armor --export {fingerprint}), docs/RELEASING.md")


def retired_keys(listed, carried):
    """The fingerprints `listed` names (a file, one a line, `#` beginning a
    note; none where there is no file): keys skwir-repository carried
    before, which rpm and dnf let go of. Never one it carries."""
    if not listed.is_file():
        return []
    found = [line.split("#")[0].strip().upper() for line in listed.read_text().splitlines()]
    found = [fingerprint for fingerprint in found if fingerprint]
    if wrong := [f for f in found if not re.fullmatch(r"[0-9A-F]{40}", f)]:
        sys.exit(f"{listed}: {', '.join(wrong)}: not a key's fingerprint (40 hexadecimal digits)")
    if kept := sorted(set(found) & {f.upper() for f in carried}):
        sys.exit(f"{listed}: {', '.join(kept)}: retired, and skwir-repository carries it")
    return found


def signer(public, coming):
    """The fingerprint that signs the indexes: the key of `public`, the key
    of `coming` (a file, or None) carried beside it. Each is fit to sign
    (`unfit`), and the key of `public` signs here (`proven`)."""
    signing = key_of(public)
    next_key = key_of(coming) if coming else None
    secret = subprocess.run(["gpg", "--batch", "--list-secret-keys", signing[0]],
                            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL).returncode == 0
    if said := unfit(signing, next_key, secret, datetime.datetime.now(datetime.UTC)):
        sys.exit("\n".join(said))
    proven(signing[0], public)
    for fingerprint, expires in [signing, next_key] if next_key else [signing]:
        if expires is not None:
            when = datetime.datetime.fromtimestamp(expires, datetime.UTC)
            print(f"key: {fingerprint} expires on {when:%Y-%m-%d}: extend it before {when - LEAD:%Y-%m-%d}",
                  file=sys.stderr)
    return signing[0]


def epoch():
    """When skwir-repository's files are dated: the last commit of its
    version file, which changes only when the version is raised, so nfpm
    makes the same package byte for byte until then, in any shell. Never
    `SOURCE_DATE_EPOCH` as the environment holds it: nixpkgs' shell sets
    it to 1980, and outside the shell it would be another date."""
    said = run("git", "-C", ROOT, "log", "-1", "--format=%ct", "--", "tools/package/repository/version",
               capture_output=True, text=True).stdout.strip()
    if not said:
        sys.exit("tools/package/repository/version: never committed, so nothing dates skwir-repository:"
                 " commit it first")
    return said


def committed(paths):
    """Refuse to make skwir-repository from these files while one differs
    from the last commit: it is dated by the version file's commit, and a
    version is one file (`changed_since`)."""
    if not paths:
        return
    changed = run("git", "-C", ROOT, "status", "--porcelain", "--", *paths, capture_output=True, text=True).stdout
    if changed.strip():
        sys.exit(f"{', '.join(str(path) for path in paths)}: changed since the last commit, and skwir-repository"
                 " holds them, dated by the version file's commit: commit them first")


def version_of(package):
    """A package's version as its system reads it (`<version>-<release>`):
    a `.rpm`'s header, a `.deb`'s control."""
    if package.suffix == ".rpm":
        with tempfile.TemporaryDirectory(prefix="skwir-rpmdb-") as db:
            return run("rpm", "--dbpath", db, "-qp", "--nosignature", "--nodigest", "--qf", "%{VERSION}-%{RELEASE}",
                       package, capture_output=True, text=True).stdout.strip()
    data = package.read_bytes()
    if not data.startswith(b"!<arch>\n"):
        sys.exit(f"{package}: not a .deb")
    at = 8
    while at + 60 <= len(data):
        name, size = data[at:at + 16].decode().strip(), int(data[at + 48:at + 58])
        if name.startswith("control.tar"):
            with tarfile.open(fileobj=io.BytesIO(data[at + 60:at + 60 + size]), mode="r:*") as control:
                text = control.extractfile("./control").read().decode()
            return re.search(r"^Version: (.*)$", text, re.M)[1].strip()
        at += 60 + size + size % 2
    sys.exit(f"{package}: a .deb without its control")


def changed_since(made, site):
    """skwir-repository's files in the folder `made` that the folder `site`
    already holds at the same version, and with other bytes: a version
    published holds what it held, or a system holding the index from before
    finds another checksum, and a change under the old version never reaches
    the systems that have it."""
    for package in sorted(made.iterdir()):
        published = site / package.name
        if (published.is_file() and published.read_bytes() != package.read_bytes()
                and version_of(published) == version_of(package)):
            yield published


def link(source, dest):
    """`dest` the same file as `source` (a hard link, else a copy)."""
    dest.parent.mkdir(parents=True, exist_ok=True)
    try:
        os.link(source, dest)
    except OSError:
        shutil.copy2(source, dest)


def package(work, name, packager, version, contents, description, out, scripts=None, rpm_scripts=None,
            folders=()):
    """A package of its own made by nfpm, dated by `epoch`: `contents`
    (source, destination, whether it is a configuration file, and the
    mode, 0o644 unless said), `scripts` nfpm's maintainer scripts and
    `rpm_scripts` an `.rpm`'s own (each a file), `folders` of its own,
    which go with it."""
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
        # nfpm names the machine that made an `.rpm` (`os.Hostname()`) unless
        # told otherwise, and the published index repeats it: a name here
        # that is no one's.
        "rpm": {"buildhost": "localhost", **({"scripts": rpm_scripts} if rpm_scripts else {})},
        **({"scripts": scripts} if scripts else {}),
        "contents": [
            {"src": str(src), "dst": dst, "type": "config|noreplace" if config_file else "file",
             "file_info": {"mode": mode[0] if mode else 0o644}}
            for src, dst, config_file, *mode in contents
        ] + [
            {"dst": folder, "type": "dir", "file_info": {"mode": 0o755}} for folder in folders
        ],
    }))
    run("nfpm", "package", "--config", config, "--packager", packager, "--target", out,
        stdout=subprocess.DEVNULL, env={**os.environ, "SOURCE_DATE_EPOCH": epoch()})


def apt_repository(work, bundle, tag, base, carried, key, version):
    """apt's repository, made in `work`, its source trusting the keys
    `carried` (the key that signs, then the key coming): the folder to
    publish."""
    folder = work / "apt"
    keyring = work / "skwir.gpg"
    keyring.write_bytes(b"".join(
        run("gpg", "--batch", "--dearmor", input=public.read_bytes(), capture_output=True).stdout
        for public in carried))
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
        # A configuration file, so that a local edit is kept across updates
        # (Debian's rule): its postrm takes it away on removal too, as its
        # key goes then, and its preinst puts it back when the package is
        # added again after (tools/package/repository/preinst.sh).
        preinst = work / f"{family}.preinst"
        preinst.write_text((HERE / "preinst.sh").read_text().replace("@source@", sources.read_text()))
        package(work, "skwir-repository", "deb", version,
                [(sources, "/etc/apt/sources.list.d/skwir.sources", True),
                 (keyring, "/usr/share/keyrings/skwir.gpg", False)],
                f"Skwir's repository for {suite['label']}, so the system updates Skwir", own,
                scripts={"preinstall": str(preinst), "postremove": str(HERE / "postrm.sh")})
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
    shutil.copy2(carried[0], folder / "skwir.asc")
    return folder


def rpm_repository(work, bundle, tag, base, carried, key, version, retired):
    """dnf's repository, made in `work`, its repo file trusting the keys
    `carried` (the key that signs, then the key coming), the fingerprints
    `retired` those of keys it carried before: the folder to publish."""
    folder = work / "rpm"
    root = folder / "fedora"
    rpms = sorted(p for p in bundle.iterdir() if RPMS.fullmatch(p.name))
    if not rpms:
        sys.exit(f"{bundle}: no Fedora package")
    for rpm in rpms:
        link(rpm, root / "pool" / tag / rpm.name)
    # Each key a file of its own, as dnf's gpgkey lists them.
    keys = [(public, "/etc/pki/rpm-gpg/RPM-GPG-KEY-skwir" + ("-next" if i else ""), False)
            for i, public in enumerate(carried)]
    repo = work / "skwir.repo"
    repo.write_text(
        "# Skwir's repository (https://skwir.org/download#repository), added by\n"
        "# the package skwir-repository: removing it removes the repository.\n"
        "# Its index is signed (repo_gpgcheck), and so is each package (gpgcheck).\n"
        f"[skwir]\nname=Skwir\nbaseurl={base}/rpm/fedora\nenabled=1\n"
        "repo_gpgcheck=1\ngpgcheck=1\ngpgkey=" + " ".join(f"file://{dest}" for _, dest, _ in keys) + "\n")
    own = root / "own" / "skwir-repository.rpm"
    own.parent.mkdir(parents=True)
    # What rpm and dnf keep of the keys, brought to what the package carries
    # once its transaction is done (tools/package/repository/keys.sh): rpm
    # keeps a key as first imported, so an extension and a key retired reach
    # it only so.
    listed = work / "keys"
    listed.write_text(
        "# What skwir-repository carries of the repositories' keys, for\n"
        "# /usr/libexec/skwir-repository/keys: each key it carries, by its\n"
        "# fingerprint and file, and each it carried before (retired).\n"
        + "".join(f"carried {key_of(public)[0]} {dest}\n" for public, dest, _ in keys)
        + "".join(f"retired {fingerprint}\n" for fingerprint in retired))
    # A configuration file, which rpm erases with the package all the same
    # (or keeps as `.rpmsave`, which dnf never reads).
    package(work, "skwir-repository", "rpm", version,
            [(repo, "/etc/yum.repos.d/skwir.repo", True), *keys,
             (HERE / "keys.sh", "/usr/libexec/skwir-repository/keys", False, 0o755),
             (listed, "/usr/share/skwir-repository/keys", False)],
            "Skwir's repository for Fedora, so the system updates Skwir", own,
            rpm_scripts={"posttrans": str(HERE / "posttrans.sh")},
            folders=["/usr/libexec/skwir-repository", "/usr/share/skwir-repository"])
    # Signed as the release's are, dated as its files are: made again the same.
    run(ROOT / "tools/package/rpm-sign.sh", carried[0], "--at", epoch(), own, stdout=subprocess.DEVNULL)
    run("createrepo_c", "--quiet", "--general-compress-type=zstd", root)
    repomd = root / "repodata/repomd.xml"
    run("gpg", "--batch", "--yes", "--local-user", key, "--armor", "--detach-sign", "--output", f"{repomd}.asc", repomd)
    shutil.copy2(carried[0], folder / "skwir.asc")
    return folder


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
    parser.add_argument("bundle", type=pathlib.Path, nargs="?")
    parser.add_argument("--site", type=pathlib.Path, help="the website's static folder (skwir-site/static)")
    parser.add_argument("--check-key", action="store_true",
                        help="only check the keys as a publication would, and print the fingerprint that signs"
                        " (release-build, before its checks)")
    parser.add_argument("--key", type=pathlib.Path, default=HERE / "skwir.asc",
                        help="the repositories' public key, which signs (its secret part in GnuPG's keyring)")
    parser.add_argument("--next", help="the key coming while the key changes, carried beside it (by default"
                        " tools/package/repository/next.asc, where there is one; '' for none)")
    parser.add_argument("--retired", type=pathlib.Path, help="the fingerprints of the keys skwir-repository"
                        " carried before and no longer does, one a line (by default"
                        " tools/package/repository/retired, where there is one)")
    parser.add_argument("--base", help="the site's address (by default Cargo.toml's homepage)")
    parser.add_argument("--version", help="skwir-repository's version (by default tools/package/repository/version)")
    args = parser.parse_args()
    if not args.check_key and not (args.bundle and args.site):
        parser.error("a bundle and --site are needed, unless --check-key")
    if not args.key.is_file():
        sys.exit(f"{args.key}: no key. Make the repositories' key once, then keep its secret part safe:\n"
                 "    gpg --quick-generate-key 'Skwir repositories' ed25519 sign 5y\n"
                 f"    gpg --armor --export <its fingerprint> > {args.key}\n"
                 "and commit the public key (docs/RELEASING.md, Repositories).")
    if args.next is None:
        coming = HERE / "next.asc" if (HERE / "next.asc").is_file() else None
    else:
        coming = pathlib.Path(args.next) if args.next else None
    if args.check_key:
        print(signer(args.key, coming))
        return
    bundle = args.bundle.resolve()
    manifest = bundle / "manifest.json"
    if not manifest.is_file():
        sys.exit(f"{bundle}: no release bundle (run `just release-build <tag>` first)")
    tag = released(bundle)["tag"]
    version = args.version or (HERE / "version").read_text().strip()
    base = (args.base or homepage()).rstrip("/")
    # What skwir-repository holds of this tree, as committed (all but when a
    # version is named: repository-check tries the tree as it is).
    held = [path.resolve() for path in ([] if args.version else [HERE / "version", HERE / "keys.sh",
                                                                  HERE / "posttrans.sh", HERE / "preinst.sh",
                                                                  HERE / "postrm.sh"])
            + [args.key, coming, args.retired or HERE / "retired"] if path and path.exists()]
    committed([path for path in held if path.is_relative_to(ROOT)])
    key = signer(args.key, coming)
    retired = retired_keys(args.retired or HERE / "retired",
                           [key_of(args.key)[0], *([key_of(coming)[0]] if coming else [])])
    # Before anything is signed: dnf checks each package (`gpgcheck`).
    rpms = sorted(p for p in bundle.iterdir() if RPMS.fullmatch(p.name))
    if refused := unsigned(rpms, args.key):
        named = ", ".join(f"{rpm.name} ({why})" for rpm, why in refused.items())
        sys.exit(f"{named}: not signed by the key that signs the indexes ({key}, {args.key.name}), and dnf"
                 " checks each package. `release-build` signs a release's with the key of skwir.asc at its tag:"
                 " a release made before skwir.asc changed (a new key's second step) is never published by it,"
                 " but by the release after (docs/RELEASING.md, A new key)")
    carried = [args.key, coming] if coming else [args.key]
    site = args.site.resolve()
    # Beside the bundle, on its filesystem, so its packages are hard links
    # into the pools, never copies in $TMPDIR (a tmpfs, in memory).
    with tempfile.TemporaryDirectory(prefix=".skwir-repositories-", dir=bundle.parent) as work:
        work = pathlib.Path(work)
        apt = apt_repository(work, bundle, tag, base, carried, key, version)
        rpm = rpm_repository(work, bundle, tag, base, carried, key, version, retired)
        if changed := [*changed_since(apt / "own", site / "apt/own"),
                       *changed_since(rpm / "fedora/own", site / "rpm/fedora/own")]:
            sys.exit(f"{', '.join(map(str, changed))}: published as skwir-repository {version}, and what it holds"
                     " changed since: raise tools/package/repository/version and commit it (docs/RELEASING.md)")
        # Both made, both written: never the indexes of one release beside
        # those of another.
        publish(apt, site / "apt", ["dists", "own", "skwir.asc"])
        publish(rpm, site / "rpm", ["fedora/repodata", "fedora/own", "skwir.asc"])


if __name__ == "__main__":
    main()
