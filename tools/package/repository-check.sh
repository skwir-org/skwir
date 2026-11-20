#!/bin/sh
# Skwir's apt and dnf repositories (ADR-142) tried as a system uses them,
# in fresh Debian 13, Ubuntu 24.04 and Fedora 44 roots entered in a user
# namespace (as try.sh enters its own; nothing of this system touched),
# against a stand-in for skwir.org on this machine (repository/serve.py),
# which sends each package to another host's name as the site sends it to
# the release, and throwaway keys.
#
# The repositories are made as `just repositories` makes them, for
# skwir.org; each root reaches the stand-in in its place (apt's source
# read with its address changed, dnf's `baseurl` set), what it installed
# left as installed. Each root starts where every system that added the
# repositories is: `skwir-repository` 1 as the site published it
# (repository/published: unsigned, its dnf repository `gpgcheck=0`), and
# takes the update to 2 that they all will, 1's key stood in for by the
# throwaway one for that one update. Then each reads the signed index with
# the key 2 brought; Debian and Ubuntu download skwir through the redirect,
# its checksum checked against the index, and Fedora installs it under 2's
# `gpgcheck=1`, importing that key (skwir's dependencies from Fedora's own
# repositories). Each refuses the index a stranger's key signed, and
# Fedora an update a stranger signed; each follows a change of key, as
# docs/RELEASING.md makes it: `skwir-repository` 3 carries the new key
# beside the old, which still signs, and 4 the new alone, which signs, the
# old retired. Then none takes what the old key signs: rpm let go of it
# (repository/keys.sh, after 4's transaction), and so did dnf's keys for
# the index. apt's source is a configuration file: a line its user added
# is kept through the updates. Removing `skwir-repository` takes the
# repository and its key away, the system's own sources read as before;
# added again, apt's source comes back with it. Last, in a Fedora root of
# its own, an extension: rpm holds a key as dnf first imported it, about
# to expire, `skwir-repository` brings it extended, and once the first
# expiry has passed what the key signs is still taken.
#
# repository.py refuses, before it writes the site, a key about to expire,
# one whose secret part is not here, a release whose `.rpm` another key
# signed, and a version published made again with other bytes: each for
# its own reason. A release's bundle is read, never changed: by default the
# newest in target/releases. What is served as the release is its
# packages, each `.rpm` signed again by the key the step names.
#
#     tools/package/repository-check.sh [target/releases/v0.6.0]
#
# Needs apt-ftparchive, createrepo_c, gpg, gpgv, nfpm, rpm, rpmkeys and
# rpmsign (`just repository-check` finds them in the flake's nixpkgs),
# bwrap, curl and python3; Debian's, Ubuntu's and Fedora's mirrors, for
# the system's own sources and skwir's dependencies. It waits two minutes
# for a key to expire.
set -eu
tree="$(cd "$(dirname "$0")/../.." && pwd)"
bundle="${1:-$(find "$tree/target/releases" -mindepth 1 -maxdepth 1 -type d -name 'v*' ! -name '*-*' | sort -V | tail -1)}"
bundle="$(cd "$bundle" && pwd)"
[ -f "$bundle/manifest.json" ] || { echo "$bundle: no release bundle" >&2; exit 1; }
roots="$HOME/.cache/skwir-tests/roots"
mkdir -p "$roots"
work="$(mktemp -d "$HOME/.cache/skwir-tests/repository-check.XXXXXX")"
published="$tree/tools/package/repository/published"
server=""
cleanup() {
    [ -n "$server" ] && kill "$server" 2>/dev/null || :
    gpgconf --kill all 2>/dev/null || :
    chmod -R u+rwX "$work" 2>/dev/null || :
    rm -rf "${work:?}"
}
trap cleanup EXIT INT TERM
fail() {
    echo "$*" >&2
    exit 1
}

# The keys, thrown away with the work: the repositories' (old), the one a
# change of key brings (new), a stranger's, one that expires within the
# 180 days repository.py asks, and one whose secret part is not here.
export GNUPGHOME="$work/gnupg"
mkdir -m 700 "$GNUPGHOME"
for name in old new stranger soon gone; do
    case "$name" in
    soon) expires=30d ;;
    *) expires=never ;;
    esac
    gpg --batch --quiet --passphrase '' --quick-generate-key "Skwir repositories ($name)" ed25519 sign "$expires" 2>/dev/null
done
fingerprint() {
    gpg --batch --with-colons --list-keys "=Skwir repositories ($1)" | awk -F: '$1 == "fpr" { print $10; exit }'
}
for name in old new stranger soon gone; do
    gpg --batch --armor --export "$(fingerprint "$name")" > "$work/$name.asc"
done
# rpm's names: a key's fingerprint, its ID.
lower() {
    fingerprint "$1" | tr 'A-F' 'a-f'
}

port="$(python3 -c 'import socket; s = socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1])')"
# The address the repositories are made for, and the stand-in's.
base="$(sed -n 's/^homepage = "\(.*\)"$/\1/p' "$tree/Cargo.toml" | head -1)"
here="http://127.0.0.1:$port"
[ -n "$base" ] || fail "Cargo.toml: no homepage"
# What is served as the release: the bundle's packages and its manifest,
# each `.rpm` signed again by the key $1, as release-build signs them with
# the repositories' (the manifest saying what they are now), into $2 or
# the release served.
released="$work/release"
signed_by() {
    into="${2:-$released}"
    rm -rf "$into"
    mkdir -p "$into"
    for package in "$bundle"/*.deb "$bundle"/*.rpm; do
        cp "$package" "$into/"
    done
    "$tree/tools/package/rpm-sign.sh" "$work/$1.asc" "$into"/*.rpm >/dev/null 2>&1
    python3 - "$bundle/manifest.json" "$into" <<'PYTHON'
import hashlib, json, pathlib, sys
manifest, released = json.loads(pathlib.Path(sys.argv[1]).read_text()), pathlib.Path(sys.argv[2])
for path in released.iterdir():
    if path.name in manifest["assets"]:
        manifest["assets"][path.name] = {"sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
                                         "bytes": path.stat().st_size}
(released / "manifest.json").write_text(json.dumps(manifest))
PYTHON
}
# The repositories made again from $from (the release served): their
# version $1, signed by the key $2, the key $3 coming beside it, the key $4
# retired.
from="$released"
make() {
    : > "$work/retired"
    [ -z "${4:-}" ] || fingerprint "$4" > "$work/retired"
    python3 "$tree/tools/package/repository.py" "$from" --site "$work/site" --key "$work/$2.asc" \
        --next "${3:+$work/$3.asc}" --retired "$work/retired" --version "$1" >/dev/null 2>"$work/made"
}
# repository.py refusing to make them (make's arguments after $1) for its
# own reason $1 (words of its message), the site left as it was.
site_now() {
    find "$work/site" -type f -exec sha256sum {} + 2>/dev/null | sort | sha256sum
}
refuses() {
    reason="$1"
    shift
    before="$(site_now)"
    if make "$@"; then
        fail "repository.py made the repositories ($*), which it refuses: $reason"
    fi
    said="$(grep -m1 -F -- "$reason" "$work/made")" ||
        fail "repository.py refused ($*), but not for that: $(grep -v GPG_TTY "$work/made" | tail -1)"
    [ "$(site_now)" = "$before" ] || fail "repository.py wrote the site, refusing ($*)"
    echo "ok: refused ($*): $(echo "$said" | cut -c1-150)"
}

# A key about to expire; one whose secret part is not here (what it signed
# made while it was); a release another key signed (named by its ID); and,
# once 2 is made, 2 made again holding another key. Each would sign as it
# should if its own check failed, so each is refused for its reason or the
# run fails.
signed_by gone "$work/release-gone"
gpg --batch --yes --delete-secret-keys "$(fingerprint gone)"
signed_by soon
refuses "$(fingerprint soon) expires on" 2 soon
from="$work/release-gone"
refuses "$(fingerprint gone): its secret part is not in GnuPG's keyring" 2 gone
from="$released"
signed_by stranger
refuses "(signed by key ID $(lower stranger | cut -c33-))" 2 old
signed_by old
make 2 old || fail "repository.py: $(tail -1 "$work/made")"
signed_by new
refuses "published as skwir-repository 2, and what it holds changed since: raise tools/package/repository/version" 2 new
# Made again as it was, it is the same file, and taken.
signed_by old
make 2 old || fail "repository.py made skwir-repository 2 again as it was, and refused it: $(tail -1 "$work/made")"
python3 "$tree/tools/package/repository/serve.py" "$work/site" "$released" "$port" 2>"$work/serve.log" &
server=$!

# A distribution's image, once, checked against its sum (as try.sh).
image() {
    case "$1" in
    debian) path=debian/trixie/amd64/default ;;
    ubuntu) path=ubuntu/noble/amd64/default ;;
    fedora) path=fedora/44/amd64/default ;;
    esac
    tarball="$roots/$1.rootfs.tar.xz"
    if [ ! -f "$tarball" ]; then
        index="https://images.linuxcontainers.org/images/$path"
        build="$(curl -fsSL "$index/" | grep -o 'href="[0-9][^"]*/"' | tail -1 | cut -d'"' -f2)"
        curl -fsSL -o "$tarball.part" "$index/${build}rootfs.tar.xz"
        sum="$(curl -fsSL "$index/${build}SHA256SUMS" | awk '$2 == "rootfs.tar.xz" { print $1 }')"
        [ "$(sha256sum "$tarball.part" | cut -d' ' -f1)" = "$sum" ] || { echo "$tarball.part: not the image's published sum" >&2; exit 1; }
        mv "$tarball.part" "$tarball"
    fi
}
# A fresh root of the distribution $1 at $2, which reaches this system's DNS.
unpack() {
    image "$1"
    mkdir -p "$2"
    tar -xJf "$roots/$1.rootfs.tar.xz" -C "$2" --exclude='./dev/*'
    chmod u+rwx "$2" "$2/root"
    rm -f "$2/etc/resolv.conf"
    cp -L /etc/resolv.conf "$2/etc/resolv.conf"
}

# Run "$@" in the root $1 as its root user, the site's folder at /site,
# skwir-repository 1 as published at /published.
enter() {
    root="$1"
    shift
    bwrap --unshare-user --uid 0 --gid 0 --cap-add ALL --unshare-pid --share-net --die-with-parent \
        --bind "$root" / --proc /proc --dev /dev --tmpfs /tmp --ro-bind "$work/site" /site \
        --ro-bind "$published" /published \
        --clearenv --setenv HOME /root --setenv LANG C.UTF-8 --setenv DEBIAN_FRONTEND noninteractive \
        --setenv PATH /usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin \
        "$@" </dev/null
}

# Only Skwir's repository is read, at the stand-in: the system's own are
# left alone until skwir-repository is removed. Any failure to read it is
# one (apt only warns when an index it read before is refused, and keeps
# that one). apt reads the source skwir-repository installed with its
# address changed ($at_here, then $apt_skwir); dnf the repository it
# installed with its baseurl set ($dnf_skwir).
at_here="sed 's|$base/|$here/|' /etc/apt/sources.list.d/skwir.sources > /tmp/skwir.sources"
apt_skwir='-o APT::Update::Error-Mode=any -o Dir::Etc::SourceList=/tmp/skwir.sources -o Dir::Etc::SourceParts=-'
dnf_at_here="--setopt=skwir.baseurl=$here/rpm/fedora"
dnf_skwir="--disablerepo=* --enablerepo=skwir --setopt=skip_if_unavailable=False $dnf_at_here"
# Until what skwir-repository's %posttrans started has ended (rpm's keys
# brought to what it carries, repository/keys.sh): a minute at most.
# shellcheck disable=SC2016 # expanded in the root
settled='{ i=0; while grep -l -s -a "skwir-repositor[y]/keys" /proc/[0-9]*/cmdline >/dev/null && [ $i -lt 60 ]; do i=$((i + 1)); sleep 1; done; }'

# The version of skwir-repository each system has installed, then offered.
installed() {
    # shellcheck disable=SC2016 # dpkg-query's own format
    case "$1" in
    debian | ubuntu) enter "$work/root-$1" dpkg-query -W -f '${Version}' skwir-repository ;;
    fedora) enter "$work/root-$1" rpm -q --qf '%{VERSION}' skwir-repository ;;
    esac
}
offered() {
    # shellcheck disable=SC2086 # the options are words
    enter "$work/root-$1" sh -c "$at_here && apt-cache policy $apt_skwir skwir-repository | sed -n 's/^ *Candidate: //p'"
}
is_version() {
    case "$(installed "$1")" in
    "$2" | "$2-1") ;;
    *) fail "$1: skwir-repository is $(installed "$1"), not $2" ;;
    esac
}
# The keys rpm holds in the root $1.
rpm_holds() {
    enter "$work/root-$1" rpmkeys --list | tr 'A-F' 'a-f'
}

for distribution in debian ubuntu fedora; do
    echo "== $distribution"
    root="$work/root-$distribution"
    unpack "$distribution" "$root"
    case "$distribution" in
    debian | ubuntu)
        echo 'APT::Sandbox::User "root";' > "$root/etc/apt/apt.conf.d/99skwir-sandbox"
        enter "$root" sh -c "apt-get install -y -q /published/skwir-repository-$distribution.deb" >/dev/null
        is_version "$distribution" 1
        # The update every system that added the repositories takes next,
        # 1's key stood in for by the one the stand-in signs with.
        enter "$root" sh -c "sed -e 's|$base/|$here/|' -e 's|^Signed-By: .*|Signed-By: /site/apt/skwir.asc|' \
            /etc/apt/sources.list.d/skwir.sources > /tmp/skwir.sources &&
            apt-get update -q $apt_skwir && apt-get install -y -q --only-upgrade $apt_skwir skwir-repository" >/dev/null
        is_version "$distribution" 2
        # shellcheck disable=SC2016 # dpkg-query's own format
        enter "$root" dpkg-query -W -f '${Conffiles}' skwir-repository | grep -q ' /etc/apt/sources.list.d/skwir.sources ' ||
            fail "$distribution: 2's source is not a configuration file"
        echo "ok: skwir-repository 1, as published, updated to 2, its source a configuration file"
        # shellcheck disable=SC2086 # the options are words
        enter "$root" sh -c "$at_here && apt-get update -q $apt_skwir" >/dev/null
        candidate="$(enter "$root" sh -c "$at_here && apt-cache policy $apt_skwir skwir | sed -n 's/^ *Candidate: //p'")"
        echo "ok: the signed index read with the key 2 brought, skwir $candidate offered"
        enter "$root" sh -c "$at_here && cd /tmp && apt-get download -q $apt_skwir skwir >/dev/null && ls skwir_*.deb" >/dev/null
        echo "ok: skwir downloaded through the redirect, its checksum the index's"
        ;;
    fedora)
        enter "$root" sh -c "dnf -y -q --disablerepo='*' install /published/skwir-repository.rpm" >/dev/null
        is_version fedora 1
        # The update every system that added the repository takes next,
        # under 1's `gpgcheck=0`, 1's key stood in for by the one the
        # stand-in signs with: 2's repository file in place of 1's, its key
        # beside it.
        enter "$root" sh -c "dnf -y -q $dnf_skwir --setopt=skwir.gpgkey=file:///site/rpm/skwir.asc upgrade skwir-repository && $settled" >/dev/null
        is_version fedora 2
        grep -qx 'gpgcheck=1' "$root/etc/yum.repos.d/skwir.repo" || fail "fedora: 2's repository file not in place of 1's"
        [ ! -e "$root/etc/yum.repos.d/skwir.repo.rpmnew" ] || fail "fedora: skwir.repo.rpmnew left beside 1's"
        cmp -s "$root/etc/pki/rpm-gpg/RPM-GPG-KEY-skwir" "$work/old.asc" || fail "fedora: 2's key not in place"
        echo "ok: skwir-repository 1, as published, updated to 2, its gpgcheck=1 in place"
        # skwir installed under 2's gpgcheck: dnf imports the key 2 brought,
        # and checks skwir with it, downloaded through the redirect, its
        # checksum the index's. Its dependencies are Fedora's.
        enter "$root" sh -c "dnf -y -q $dnf_at_here --setopt=install_weak_deps=False install skwir" >/dev/null
        enter "$root" rpm -q skwir >/dev/null || fail "fedora: skwir not installed"
        rpm_holds fedora | grep -q "^$(lower old) " || fail "fedora: skwir installed, and no key imported for it"
        echo "ok: skwir installed through the redirect under gpgcheck=1, the key 2 brought imported"
        ;;
    esac
done

# Indexes a stranger's key signed, offering skwir-repository 9: no system
# reads them. That the next step reads the same server's indexes shows the
# signature was what each refused.
signed_by stranger
make 9 stranger || fail "repository.py: $(tail -1 "$work/made")"
for distribution in debian ubuntu fedora; do
    root="$work/root-$distribution"
    case "$distribution" in
    debian | ubuntu)
        if enter "$root" sh -c "$at_here && apt-get update -q $apt_skwir" >"$work/said" 2>&1; then
            fail "$distribution read the index a stranger signed"
        fi
        case "$(offered "$distribution")" in
        2 | 2-1) ;;
        *) fail "$distribution offers skwir-repository $(offered "$distribution") from the index a stranger signed" ;;
        esac
        ;;
    fedora)
        if enter "$root" sh -c "dnf -y -q $dnf_skwir --refresh makecache" >"$work/said" 2>&1; then
            fail "$distribution read the index a stranger signed"
        fi
        ;;
    esac
    echo "ok: $distribution refuses the index a stranger signed"
done

# An update the key $2 signed, listed by an index the key $3 signs (its
# checksum the index's), skwir-repository $1: dnf checks the package
# itself (`gpgcheck`) and refuses it, for its signature.
update_signed_by() {
    signed_by "$3"
    make "$1" "$3" || fail "repository.py: $(tail -1 "$work/made")"
    "$tree/tools/package/rpm-sign.sh" "$work/$2.asc" "$work/site/rpm/fedora/own/skwir-repository.rpm" >/dev/null 2>&1
    createrepo_c --quiet --general-compress-type=zstd "$work/site/rpm/fedora"
    gpg --batch --yes --local-user "$(fingerprint "$3")" --armor --detach-sign \
        --output "$work/site/rpm/fedora/repodata/repomd.xml.asc" "$work/site/rpm/fedora/repodata/repomd.xml"
    was="$(installed fedora)"
    if enter "$work/root-fedora" sh -c "dnf -y -q $dnf_skwir --refresh upgrade skwir-repository" >"$work/said" 2>&1; then
        fail "fedora installed skwir-repository $1, which the key '$2' signed"
    fi
    [ "$(installed fedora)" = "$was" ] || fail "fedora: skwir-repository is $(installed fedora), from an update the key '$2' signed"
    refusal="$(grep -i -E 'fail|error|not ' "$work/said" | grep -i -E 'openpgp|signature|signed|key' | head -1)"
    [ -n "$refusal" ] || fail "fedora refused skwir-repository $1, but not for its signature: $(tail -3 "$work/said")"
    echo "ok: fedora refuses an update the key '$2' signed: $(echo "$refusal" | cut -c1-140)"
}
update_signed_by 8 stranger old

# Debian's and Ubuntu's sources edited by their users, a line of their own
# added, which the updates that follow keep (a configuration file).
edited="# A line of this system's own, kept by skwir-repository's updates"
for distribution in debian ubuntu; do
    echo "$edited" >> "$work/root-$distribution/etc/apt/sources.list.d/skwir.sources"
done

# A change of key: skwir-repository 3 carries the new key beside the old,
# which signs its indexes; 4 the new alone, which signs them, the old
# retired. Each system upgrades from the repository to each.
for step in 3:old:new: 4:new::old; do
    version="${step%%:*}"
    keys="${step#*:}"
    signing="${keys%%:*}"
    coming="$(echo "$keys" | cut -d: -f2)"
    retired="$(echo "$keys" | cut -d: -f3)"
    signed_by "$signing"
    make "$version" "$signing" "$coming" "$retired" || fail "repository.py: $(tail -1 "$work/made")"
    for distribution in debian ubuntu fedora; do
        root="$work/root-$distribution"
        case "$distribution" in
        debian | ubuntu)
            enter "$root" sh -c "$at_here && apt-get update -q $apt_skwir && apt-get install -y -q --only-upgrade $apt_skwir skwir-repository" >/dev/null
            ;;
        fedora)
            enter "$root" sh -c "dnf -y -q $dnf_skwir --refresh upgrade skwir-repository && $settled" >/dev/null
            ;;
        esac
        is_version "$distribution" "$version"
        echo "ok: $distribution upgraded skwir-repository to $version ($keys)"
    done
done
for distribution in debian ubuntu; do
    grep -qxF "$edited" "$work/root-$distribution/etc/apt/sources.list.d/skwir.sources" ||
        fail "$distribution: its own line in skwir-repository's source lost to an update"
    echo "ok: $distribution kept its own line in skwir-repository's source through the updates"
done

# The old key retired: rpm let go of it, and dnf of its key for the index
# (it read the repository's keys again, the new alone). No system takes an
# index the old key signs; Fedora no update it signs either, listed by an
# index the new key signs, which rpm would have taken while it held the
# key.
rpm_holds fedora | grep -q "^$(lower old) " && fail "fedora: rpm still holds the old key, retired"
rpm_holds fedora | grep -q "^$(lower new) " || fail "fedora: rpm does not hold the new key"
echo "ok: fedora's rpm let go of the old key, retired, and holds the new"
update_signed_by 5 old new
signed_by old
make 6 old || fail "repository.py: $(tail -1 "$work/made")"
for distribution in debian ubuntu fedora; do
    root="$work/root-$distribution"
    case "$distribution" in
    debian | ubuntu)
        if enter "$root" sh -c "$at_here && apt-get update -q $apt_skwir" >"$work/said" 2>&1; then
            fail "$distribution read an index the old key signed, retired"
        fi
        ;;
    fedora)
        if enter "$root" sh -c "dnf -y -q $dnf_skwir --refresh makecache" >"$work/said" 2>&1; then
            fail "$distribution read an index the old key signed, retired"
        fi
        ;;
    esac
    echo "ok: $distribution refuses an index the old key signed, retired"
done

# What the site offers again: the new key's, the old retired.
signed_by new
make 7 new "" old || fail "repository.py: $(tail -1 "$work/made")"

# Removing skwir-repository removes the repository and its key, the source
# its user edited too: what the system reads after is its own sources
# alone, which still read. Added again from the file the download page
# names, Debian's and Ubuntu's source comes back with it (dpkg never puts
# back a configuration file it takes for deleted by its user), read with
# the key it brings.
for distribution in debian ubuntu fedora; do
    root="$work/root-$distribution"
    case "$distribution" in
    debian | ubuntu)
        enter "$root" apt-get remove -y -q skwir-repository >/dev/null
        left="/etc/apt/sources.list.d/skwir.sources /usr/share/keyrings/skwir.gpg"
        ;;
    fedora)
        enter "$root" sh -c "dnf -y -q --disablerepo='*' remove skwir-repository" >/dev/null
        left="/etc/yum.repos.d/skwir.repo /etc/pki/rpm-gpg/RPM-GPG-KEY-skwir /usr/libexec/skwir-repository
            /usr/share/skwir-repository"
        ;;
    esac
    for file in $left; do
        [ ! -e "$root$file" ] || fail "$distribution: $file left after skwir-repository was removed"
    done
    case "$distribution" in
    debian | ubuntu)
        enter "$root" apt-get update -q -o APT::Update::Error-Mode=any >/dev/null ||
            fail "$distribution: apt-get update fails after skwir-repository was removed"
        echo "ok: $distribution removed the repository and its key, and reads its own sources"
        enter "$root" sh -c "apt-get install -y -q /site/apt/own/skwir-repository-$distribution.deb" >/dev/null
        [ -e "$root/etc/apt/sources.list.d/skwir.sources" ] ||
            fail "$distribution: skwir-repository added again after its removal, without its source"
        enter "$root" sh -c "$at_here && apt-get update -q $apt_skwir" >/dev/null ||
            fail "$distribution: skwir-repository added again, its index not read"
        echo "ok: $distribution added skwir-repository again, its source back and its index read"
        ;;
    fedora)
        echo "ok: $distribution removed the repository and its key"
        ;;
    esac
done

# An extension, in a Fedora root of its own: rpm holds the key 'brief' as
# dnf first imported it, expiring in two minutes, and skwir-repository 10
# brings it extended. Once the first expiry has passed, the release's
# packages are signed again: that first copy refuses skwir, and the
# system's rpm, which dnf's gpgcheck asks, takes it: rpm took the
# extension (repository/keys.sh). skwir-repository's own signature is
# dated by its version (repository.py), so it is not what shows it.
root="$work/root-extension"
unpack fedora "$root"
gpg --batch --quiet --passphrase '' --quick-generate-key "Skwir repositories (brief)" ed25519 sign seconds=120 2>/dev/null
expires="$(gpg --batch --with-colons --list-keys "=Skwir repositories (brief)" | awk -F: '$1 == "pub" { print $7; exit }')"
gpg --batch --armor --export "$(fingerprint brief)" > "$root/root/brief-first.asc"
gpg --batch --quick-set-expire "$(fingerprint brief)" 5y
gpg --batch --armor --export "$(fingerprint brief)" > "$work/brief.asc"
signed_by brief
make 10 brief || fail "repository.py: $(tail -1 "$work/made")"
enter "$root" sh -c "rpmkeys --import /root/brief-first.asc &&
    dnf -y -q --disablerepo='*' install /site/rpm/fedora/own/skwir-repository.rpm && $settled" >/dev/null
[ "$(date +%s)" -lt "$expires" ] || fail "extension: skwir-repository 10 installed after the key's first expiry: give 'brief' longer"
echo "ok: extension: skwir-repository 10, the key extended, installed while rpm held its first copy"
now="$(date +%s)"
[ "$now" -gt "$expires" ] || sleep $((expires - now + 2))
signed_by brief
make 11 brief || fail "repository.py: $(tail -1 "$work/made")"
said="$(enter "$root" sh -c "dnf -y -q $dnf_skwir --refresh download --destdir /tmp skwir >/dev/null &&
    mkdir /tmp/first && rpmkeys --dbpath /tmp/first --import /root/brief-first.asc &&
    echo \"first: \$(LC_ALL=C rpmkeys --dbpath /tmp/first --checksig /tmp/skwir-[0-9]*.rpm)\" &&
    echo \"system: \$(LC_ALL=C rpmkeys --checksig /tmp/skwir-[0-9]*.rpm)\"" 2>&1)" ||
    fail "extension: fedora did not read the index the key signed after its first expiry: $(echo "$said" | tail -3)"
case "$(echo "$said" | sed -n 's/^first: //p')" in
*" signatures OK") fail "extension: the key's first copy still takes what it signs: its expiry did not pass" ;;
esac
echo "ok: extension: the index the key signed after its first expiry read; the key's first copy refuses skwir"
case "$(echo "$said" | sed -n 's/^system: //p')" in
*" signatures OK") echo "ok: extension: fedora's rpm takes skwir, signed after the key's first expiry" ;;
*) fail "extension: fedora's rpm refuses skwir, signed after the key's first expiry: $(echo "$said" | tail -3)" ;;
esac

grep -c ' 302 ' "$work/serve.log" | sed 's/^/ok: requests sent on to the release: /'
echo "repositories: passed"
