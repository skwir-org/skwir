#!/bin/sh
# A Linux package built and installed in a distribution's own root, from
# any Linux with bubblewrap (ADR-040): no container engine, no root, and
# nothing of this system touched.
#
# The distribution's root filesystem (images.linuxcontainers.org, checked
# against the sums published beside it) is unpacked under
# ~/.cache/bibli-tests/roots and entered as its root user in a user
# namespace. The build's needs are installed there once
# (tools/package/linux/prepare.sh); the tree's files are built into the
# package (deb.sh, rpm.sh, aur/build.sh); then a fresh root installs the
# package with the distribution's package manager, which brings what it
# depends on and nothing it only recommends, and
# tools/package/linux/check.sh checks it there. The package lands in
# target/package/linux.
#
#     tools/package/try.sh debian|fedora|arch
set -eu
distribution="${1:-}"
case "$distribution" in
debian) image=debian/trixie/amd64/default ;;
fedora) image=fedora/44/amd64/default ;;
arch) image=archlinux/current/amd64/default ;;
*)
    echo "usage: $0 debian|fedora|arch" >&2
    exit 2
    ;;
esac
tree="$(cd "$(dirname "$0")/../.." && pwd)"
roots="$HOME/.cache/bibli-tests/roots"
out="$tree/target/package/linux"
mkdir -p "$roots" "$out"

# The image, once: its root filesystem, checked against its sum.
tarball="$roots/$distribution.rootfs.tar.xz"
if [ ! -f "$tarball" ]; then
    index="https://images.linuxcontainers.org/images/$image"
    build="$(curl -fsSL "$index/" | grep -o 'href="[0-9][^"]*/"' | tail -1 | cut -d'"' -f2)"
    curl -fsSL -o "$tarball.part" "$index/${build}rootfs.tar.xz"
    sum="$(curl -fsSL "$index/${build}SHA256SUMS" | awk '$2 == "rootfs.tar.xz" { print $1 }')"
    if [ "$(sha256sum "$tarball.part" | cut -d' ' -f1)" != "$sum" ]; then
        echo "$tarball.part: not the image's published sum" >&2
        exit 1
    fi
    mv "$tarball.part" "$tarball"
fi

# A root of it at $1, as unpacked by someone who is not root: folders
# their owner may write (Fedora's system folders and /root are not),
# this system's DNS, and the package managers' own sandboxes off (they
# would drop to users a namespace of one user does not have).
unpack() {
    if [ -d "$1" ]; then
        chmod -R u+rwX "$1"
        rm -rf "${1:?}"
    fi
    mkdir -p "$1"
    tar -xJf "$tarball" -C "$1" --exclude='./dev/*'
    chmod u+rwx "$1" "$1/root"
    rm -f "$1/etc/resolv.conf"
    cp -L /etc/resolv.conf "$1/etc/resolv.conf"
    case "$distribution" in
    debian)
        echo 'APT::Sandbox::User "root";' > "$1/etc/apt/apt.conf.d/99bibli-sandbox"
        ;;
    arch)
        sed -i 's/^CheckSpace/#CheckSpace/; s/^DownloadUser/#DownloadUser/; s/^#\(DisableSandbox\)/\1/' "$1/etc/pacman.conf"
        ;;
    esac
}

# Run "$@" in the root $1 as its root user (uid 0) or as `builder`
# (1000), with the tree's tools at /tools and this system's crates
# cache shared.
enter() {
    root="$1"
    uid="$2"
    shift 2
    if [ "$uid" = 0 ]; then
        home=/root
        set -- --cap-add ALL "$@"
    else
        home=/home/builder
    fi
    mkdir -p "$root$home/.cargo/registry"
    bwrap --unshare-user --uid "$uid" --gid "$uid" --unshare-pid --share-net --die-with-parent \
        --bind "$root" / --proc /proc --dev /dev --tmpfs /tmp \
        --ro-bind "$tree/tools" /tools \
        --bind "$HOME/.cargo/registry" "$home/.cargo/registry" \
        --clearenv --setenv HOME "$home" --setenv LANG C.UTF-8 \
        --setenv PATH "$home/.cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin" \
        --setenv CARGO_BUILD_JOBS "${CARGO_BUILD_JOBS:-8}" \
        "$@"
}

# The build root, prepared once and kept: what the build needs.
builder="$roots/$distribution-build"
if [ ! -f "$builder/.bibli-prepared" ]; then
    unpack "$builder"
    if [ "$distribution" = arch ]; then
        enter "$builder" 0 sh -c 'pacman-key --init && pacman-key --populate archlinux' >/dev/null
    fi
    enter "$builder" 0 /tools/package/linux/prepare.sh "$distribution"
    touch "$builder/.bibli-prepared"
fi

# The tree's files, fresh; the build's target kept between tries.
if [ -d "$builder/src" ]; then
    chmod -R u+rwX "$builder/src"
    rm -rf "${builder:?}/src"
fi
mkdir -p "$builder/src"
(cd "$tree" && git ls-files -co --exclude-standard -z | tar --null -T - -cf -) | tar -xf - -C "$builder/src"
# The newest package made: names of the tools' own making.
# shellcheck disable=SC2012
case "$distribution" in
debian)
    enter "$builder" 0 env CARGO_TARGET_DIR=/build/target sh -c 'cd /src && tools/package/deb.sh'
    package="$(ls -t "$builder"/build/target/debian/bibli_*.deb | head -1)"
    ;;
fedora)
    enter "$builder" 0 env CARGO_TARGET_DIR=/build/target sh -c 'cd /src && tools/package/rpm.sh'
    package="$(ls -t "$builder"/build/target/generate-rpm/bibli-*.rpm | head -1)"
    ;;
arch)
    enter "$builder" 1000 sh -c 'cd /src && tools/package/aur/build.sh'
    for made in "$builder"/src/target/aur/bibli-*.pkg.tar.zst; do
        case "$made" in
        *-debug-*) ;;
        *) package="$made" ;;
        esac
    done
    ;;
esac
cp "$package" "$out/"
echo "built: $out/$(basename "$package")"

# A fresh root installs it, with what it depends on, and checks it.
fresh="$roots/$distribution-try"
unpack "$fresh"
mkdir -p "$fresh/package"
cp "$package" "$fresh/package/"
name="/package/$(basename "$package")"
case "$distribution" in
debian)
    enter "$fresh" 0 sh -c "apt-get update -q && DEBIAN_FRONTEND=noninteractive apt-get install -y -q --no-install-recommends $name" >/dev/null
    ;;
fedora)
    enter "$fresh" 0 dnf -y -q install --setopt=install_weak_deps=False "$name" >/dev/null
    ;;
arch)
    enter "$fresh" 0 sh -c "pacman-key --init && pacman-key --populate archlinux && pacman -Syu --noconfirm && pacman -U --noconfirm $name" >/dev/null
    ;;
esac
enter "$fresh" 0 /tools/package/linux/check.sh
chmod -R u+rwX "$fresh"
rm -rf "${fresh:?}"
