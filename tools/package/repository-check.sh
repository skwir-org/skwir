#!/bin/sh
# Skwir's apt and dnf repositories (ADR-142) tried as a system uses them,
# in fresh Debian 13, Ubuntu 24.04 and Fedora 44 roots entered in a user
# namespace (as try.sh enters its own; nothing of this system touched),
# against a stand-in for skwir.org on this machine (repository/serve.py),
# which sends each package to another host's name as the site sends it to
# the release, and throwaway keys.
#
# Each root installs `skwir-repository` from the file the download page
# names, reads the signed index with the key that package brought, and
# downloads skwir through the redirect, its checksum checked against the
# index. Then each refuses the index a stranger's key signed; follows a
# change of key, as docs/RELEASING.md makes it: `skwir-repository` 2
# carries the new key beside the old, which still signs, and 3 the new
# alone, which signs; and removing `skwir-repository` takes the repository
# and its key away, the system's own sources read as before. A release's bundle is
# read, never changed: by default the newest in target/releases.
#
#     tools/package/repository-check.sh [target/releases/v0.6.0]
#
# Needs apt-ftparchive, createrepo_c, gpg and nfpm (`just repository-check`
# finds them in the flake's nixpkgs), bwrap, curl and python3; Debian's and
# Ubuntu's mirrors, for the system's own sources.
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
gpg --batch --yes --delete-secret-keys "$(fingerprint gone)"

port="$(python3 -c 'import socket; s = socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1])')"
# The repositories made again: skwir-repository's version $1, signed by
# the key $2, the key $3 coming beside it.
make() {
    python3 "$tree/tools/package/repository.py" "$bundle" --site "$work/site" --key "$work/$2.asc" \
        --next "${3:+$work/$3.asc}" --base "http://127.0.0.1:$port" --version "$1" >/dev/null
}
for key in soon gone; do
    if make 1 "$key" 2>"$work/refused"; then
        fail "repository.py signed with the key '$key'"
    fi
    [ ! -e "$work/site" ] || fail "repository.py wrote the site, refusing the key '$key'"
    echo "ok: the key '$key' refused: $(head -1 "$work/refused")"
done
make 1 old
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

# Only Skwir's repository is read: the system's own are left alone until
# skwir-repository is removed. Any failure to read it is one (apt only
# warns when an index it read before is refused, and keeps that one).
apt_skwir='-o APT::Update::Error-Mode=any -o Dir::Etc::SourceList=/etc/apt/sources.list.d/skwir.sources -o Dir::Etc::SourceParts=-'
dnf_skwir="--disablerepo=* --enablerepo=skwir --setopt=skip_if_unavailable=False"

# The version of skwir-repository each system has installed, then offered.
installed() {
    # shellcheck disable=SC2016 # dpkg-query's own format
    case "$1" in
    debian | ubuntu) enter "$work/root-$1" dpkg-query -W -f '${Version}' skwir-repository ;;
    fedora) enter "$work/root-$1" rpm -q --qf '%{VERSION}' skwir-repository ;;
    esac
}
offered() {
    enter "$work/root-$1" sh -c "apt-cache policy skwir-repository | sed -n 's/^ *Candidate: //p'"
}

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
        # This system's DNS, for the distribution's own sources at the end.
        rm -f "$root/etc/resolv.conf"
        cp -L /etc/resolv.conf "$root/etc/resolv.conf"
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

# Indexes a stranger's key signed, offering skwir-repository 9: no system
# reads them. That the next step reads the same server's indexes shows the
# signature was what each refused.
make 9 stranger
for distribution in debian ubuntu fedora; do
    root="$work/root-$distribution"
    case "$distribution" in
    debian | ubuntu)
        if enter "$root" sh -c "apt-get update -q $apt_skwir" >"$work/said" 2>&1; then
            fail "$distribution read the index a stranger signed"
        fi
        case "$(offered "$distribution")" in
        1 | 1-1) ;;
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

# A change of key: skwir-repository 2 carries the new key beside the old,
# which signs its indexes; 3 the new alone, which signs them. Each system
# upgrades from the repository to each.
for step in 2:old:new 3:new; do
    version="${step%%:*}"
    keys="${step#*:}"
    # shellcheck disable=SC2046 # the keys are words
    make "$version" $(echo "$keys" | tr : ' ')
    for distribution in debian ubuntu fedora; do
        root="$work/root-$distribution"
        case "$distribution" in
        debian | ubuntu)
            enter "$root" sh -c "apt-get update -q $apt_skwir && apt-get install -y -q --only-upgrade $apt_skwir skwir-repository" >/dev/null
            ;;
        fedora)
            enter "$root" sh -c "dnf -y -q $dnf_skwir --refresh upgrade skwir-repository" >/dev/null
            ;;
        esac
        case "$(installed "$distribution")" in
        "$version" | "$version-1") echo "ok: $distribution upgraded skwir-repository to $version ($keys)" ;;
        *) fail "$distribution: skwir-repository is $(installed "$distribution"), not $version" ;;
        esac
    done
done

# Removing skwir-repository removes the repository and its key: what the
# system reads after is its own sources alone, which still read.
for distribution in debian ubuntu fedora; do
    root="$work/root-$distribution"
    case "$distribution" in
    debian | ubuntu)
        enter "$root" apt-get remove -y -q skwir-repository >/dev/null
        left="/etc/apt/sources.list.d/skwir.sources /usr/share/keyrings/skwir.gpg"
        ;;
    fedora)
        enter "$root" sh -c "dnf -y -q --disablerepo='*' remove skwir-repository" >/dev/null
        left="/etc/yum.repos.d/skwir.repo /etc/pki/rpm-gpg/RPM-GPG-KEY-skwir"
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
        ;;
    fedora)
        echo "ok: $distribution removed the repository and its key"
        ;;
    esac
done
grep -c ' 302 ' "$work/serve.log" | sed 's/^/ok: requests sent on to the release: /'
echo "repositories: passed"
