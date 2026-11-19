#!/bin/sh
# Skwir's apt and dnf repositories (ADR-142) tried as a system uses them,
# in fresh Debian 13, Ubuntu 24.04 and Fedora 44 roots entered in a user
# namespace (as try.sh enters its own; nothing of this system touched),
# against a stand-in for skwir.org on this machine (repository/serve.py),
# which sends each package to another host's name as the site sends it to
# the release, and a throwaway key.
#
# Each root installs `skwir-repository` from the file the download page
# names, reads the signed index with the key that package brought, and
# downloads skwir through the redirect, its checksum checked against the
# index; then the repositories are made again with `skwir-repository` 2,
# and the system upgrades to it. A release's bundle is read, never
# changed: by default the newest in target/releases.
#
#     tools/package/repository-check.sh [target/releases/v0.6.0]
#
# Needs apt-ftparchive, createrepo_c, gpg and nfpm (`just repository-check`
# finds them in the flake's nixpkgs), bwrap, curl and python3.
set -eu
tree="$(cd "$(dirname "$0")/../.." && pwd)"
bundle="${1:-$(find "$tree/target/releases" -mindepth 1 -maxdepth 1 -type d -name 'v*' ! -name '*-*' | sort -V | tail -1)}"
bundle="$(cd "$bundle" && pwd)"
[ -f "$bundle/manifest.json" ] || { echo "$bundle: no release bundle" >&2; exit 1; }
roots="$HOME/.cache/skwir-tests/roots"
mkdir -p "$roots"
work="$(mktemp -d "$HOME/.cache/skwir-tests/repository-check.XXXXXX")"
server=""
cleanup() {
    [ -n "$server" ] && kill "$server" 2>/dev/null || :
    chmod -R u+rwX "$work" 2>/dev/null || :
    rm -rf "${work:?}"
}
trap cleanup EXIT INT TERM

# The key: thrown away with the work.
export GNUPGHOME="$work/gnupg"
mkdir -m 700 "$GNUPGHOME"
gpg --batch --quiet --passphrase '' --quick-generate-key 'Skwir repositories (test)' ed25519 sign never
gpg --batch --armor --export 'Skwir repositories (test)' > "$work/key.asc"

port="$(python3 -c 'import socket; s = socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1])')"
make() {
    python3 "$tree/tools/package/repository.py" "$bundle" --site "$work/site" --key "$work/key.asc" \
        --base "http://127.0.0.1:$port" --version "$1" >/dev/null
}
make 1
python3 "$tree/tools/package/repository/serve.py" "$work/site" "$bundle" "$port" 2>"$work/serve.log" &
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

# Run "$@" in the root $1 as its root user, the site's folder at /site.
enter() {
    root="$1"
    shift
    bwrap --unshare-user --uid 0 --gid 0 --cap-add ALL --unshare-pid --share-net --die-with-parent \
        --bind "$root" / --proc /proc --dev /dev --tmpfs /tmp --ro-bind "$work/site" /site \
        --clearenv --setenv HOME /root --setenv LANG C.UTF-8 --setenv DEBIAN_FRONTEND noninteractive \
        --setenv PATH /usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin \
        "$@"
}

# Only Skwir's repository is read: the system's own are left alone.
apt_skwir='-o Dir::Etc::SourceList=/etc/apt/sources.list.d/skwir.sources -o Dir::Etc::SourceParts=-'
dnf_skwir="--disablerepo=* --enablerepo=skwir"

for distribution in debian ubuntu fedora; do
    echo "== $distribution"
    image "$distribution"
    root="$work/root-$distribution"
    mkdir -p "$root"
    tar -xJf "$roots/$distribution.rootfs.tar.xz" -C "$root" --exclude='./dev/*'
    chmod u+rwx "$root" "$root/root"
    case "$distribution" in
    debian | ubuntu)
        echo 'APT::Sandbox::User "root";' > "$root/etc/apt/apt.conf.d/99skwir-sandbox"
        enter "$root" sh -c "apt-get install -y -q /site/apt/own/skwir-repository-$distribution.deb" >/dev/null
        # shellcheck disable=SC2086 # the options are words
        enter "$root" sh -c "apt-get update -q $apt_skwir" >/dev/null
        candidate="$(enter "$root" sh -c "apt-cache policy skwir | sed -n 's/^ *Candidate: //p'")"
        echo "ok: the signed index read, skwir $candidate offered"
        enter "$root" sh -c "cd /tmp && apt-get download -q $apt_skwir skwir >/dev/null && ls skwir_*.deb"
        echo "ok: skwir downloaded through the redirect, its checksum the index's"
        ;;
    fedora)
        # Nothing outside this machine: the file the page names needs no
        # repository, and only Skwir's is read after it.
        rm -f "$root/etc/resolv.conf"
        enter "$root" sh -c "dnf -y -q --disablerepo='*' install /site/rpm/fedora/own/skwir-repository.rpm" >/dev/null
        # shellcheck disable=SC2086 # the options are words
        enter "$root" sh -c "dnf -y -q $dnf_skwir makecache" >/dev/null
        enter "$root" sh -c "dnf -y -q $dnf_skwir download --destdir /tmp skwir >/dev/null && ls /tmp/skwir-[0-9]*.rpm"
        echo "ok: the signed index read, skwir downloaded through the redirect, its checksum the index's"
        ;;
    esac
done

# skwir-repository 2: each system upgrades to it from the repository.
make 2
for distribution in debian ubuntu fedora; do
    root="$work/root-$distribution"
    case "$distribution" in
    debian | ubuntu)
        enter "$root" sh -c "apt-get update -q $apt_skwir && apt-get install -y -q --only-upgrade $apt_skwir skwir-repository" >/dev/null
        version="$(enter "$root" dpkg-query -W -f '${Version}' skwir-repository)"
        ;;
    fedora)
        enter "$root" sh -c "dnf -y -q $dnf_skwir --refresh upgrade skwir-repository" >/dev/null
        version="$(enter "$root" rpm -q --qf '%{VERSION}' skwir-repository)"
        ;;
    esac
    case "$version" in
    2 | 2-1) echo "ok: $distribution upgraded skwir-repository to $version" ;;
    *) echo "$distribution: skwir-repository is $version, not 2" >&2; exit 1 ;;
    esac
done
grep -c ' 302 ' "$work/serve.log" | sed 's/^/ok: requests sent on to the release: /'
echo "repositories: passed"
