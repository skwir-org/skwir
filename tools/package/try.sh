#!/bin/sh
# A Linux package built and installed in a distribution's own root, from
# any Linux with bubblewrap (ADR-040): no container engine, no root, and
# nothing of this system touched.
#
# The distribution's root filesystem (images.linuxcontainers.org, checked
# against the sums published beside it) is unpacked under
# ~/.cache/skwir-tests/roots and entered as its root user in a user
# namespace. The build's needs are installed there once
# (tools/package/linux/prepare.sh); the tree's files are built into the
# package (deb.sh, rpm.sh, aur/build.sh); then a fresh root installs the
# package with the distribution's package manager, which brings what it
# depends on and nothing it only recommends, and
# tools/package/linux/check.sh checks it there. The package lands in
# target/package/linux.
# Set SKWIR_PACKAGE_OUT to collect checked packages in a different directory.
#
#     tools/package/try.sh debian|ubuntu|fedora|arch
set -eu
distribution="${1:-}"
case "$distribution" in
debian) image=debian/trixie/amd64/default ;;
# The oldest Ubuntu LTS with the libraries' 64-bit time names (t64).
ubuntu) image=ubuntu/noble/amd64/default ;;
fedora) image=fedora/44/amd64/default ;;
arch) image=archlinux/current/amd64/default ;;
*)
    echo "usage: $0 debian|ubuntu|fedora|arch" >&2
    exit 2
    ;;
esac
tree="$(cd "$(dirname "$0")/../.." && pwd)"
roots="$HOME/.cache/skwir-tests/roots"
out="${SKWIR_PACKAGE_OUT:-$tree/target/package/linux}"
mkdir -p "$roots" "$out"
exec 9>"$roots/$distribution.lock"
flock -n 9 || { echo "A $distribution package build is already running" >&2; exit 1; }

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
    debian | ubuntu)
        echo 'APT::Sandbox::User "root";' > "$1/etc/apt/apt.conf.d/99skwir-sandbox"
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
        package_home=/root
        set -- --cap-add ALL "$@"
    else
        package_home=/home/builder
    fi
    mkdir -p "$root$package_home/.cargo/registry"
    bwrap --unshare-user --uid "$uid" --gid "$uid" --unshare-pid --share-net --die-with-parent \
        --bind "$root" / --proc /proc --dev /dev --tmpfs /tmp \
        --ro-bind "$tree/tools" /tools \
        --bind "$HOME/.cargo/registry" "$package_home/.cargo/registry" \
        --clearenv --setenv HOME "$package_home" --setenv LANG C.UTF-8 \
        --setenv PATH "$package_home/.cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin" \
        --setenv CARGO_BUILD_JOBS "${CARGO_BUILD_JOBS:-8}" \
        "$@"
}

# dpkg's package scripts give files to users and groups this namespace
# does not have: on Debian and Ubuntu, dpkg runs under fakeroot.
rootless() {
    case "$distribution" in
    debian | ubuntu) enter "$1" 0 /tools/package/linux/rootless-apt.sh >/dev/null ;;
    esac
}

# The build root, prepared once and kept: what the build needs.
builder="$roots/$distribution-build"
if [ ! -f "$builder/.skwir-prepared" ]; then
    unpack "$builder"
    if [ "$distribution" = arch ]; then
        enter "$builder" 0 sh -c 'pacman-key --init && pacman-key --populate archlinux' >/dev/null
    fi
    rootless "$builder"
    enter "$builder" 0 /tools/package/linux/prepare.sh "$distribution"
    touch "$builder/.skwir-prepared"
fi

# The tree's files, fresh; the build's target kept between tries.
if [ -d "$builder/src" ]; then
    chmod -R u+rwX "$builder/src"
    rm -rf "${builder:?}/src"
fi
mkdir -p "$builder/src"
(cd "$tree" && git ls-files -co --exclude-standard -z | tar --null -T - -cf -) | tar -xf - -C "$builder/src"
# The packages made: skwir, and those of its own a package of its own
# holds (skwir-media, skwir-poppler… on Debian and Fedora; skwir-mermaid
# and skwir-mupdf on Arch), by names of the tools' own making.
case "$distribution" in
debian | ubuntu)
    # Keep Cargo's compiled dependencies, but never reuse old packages.
    rm -rf "$builder/build/target/debian"
    enter "$builder" 0 env CARGO_TARGET_DIR=/build/target sh -c 'cd /src && tools/package/deb.sh'
    made="$builder/build/target/debian"
    main='skwir_*.deb'
    extra='skwir-*_*.deb'
    ;;
fedora)
    rm -rf "$builder/build/target/generate-rpm"
    enter "$builder" 0 env CARGO_TARGET_DIR=/build/target sh -c 'cd /src && tools/package/rpm.sh'
    made="$builder/build/target/generate-rpm"
    main='skwir-[0-9]*.rpm'
    extra='skwir-[a-z]*.rpm'
    ;;
arch)
    enter "$builder" 1000 sh -c 'cd /src && tools/package/aur/build.sh'
    made="$builder/src/target/aur"
    main='skwir-[0-9]*.pkg.tar.zst'
    extra='skwir-[a-z]*.pkg.tar.zst'
    ;;
esac

# A fresh root installs skwir alone, without what it only recommends,
# and checks it; then the others, with what they recommend (and on Arch
# the libraries skwir's helpers may go without), and checks them all.
fresh="$roots/$distribution-try"
unpack "$fresh"
rootless "$fresh"
mkdir -p "$fresh/package/main" "$fresh/package/extra"
# shellcheck disable=SC2086 # the names are patterns
for package in "$made"/$main; do
    cp "$package" "$fresh/package/main/"
done
# shellcheck disable=SC2086 # the names are patterns
for package in "$made"/$extra; do
    case "$package" in
    *-debug-*) continue ;;
    esac
    cp "$package" "$fresh/package/extra/"
done
case "$distribution" in
debian | ubuntu)
    enter "$fresh" 0 sh -c 'apt-get update -q && DEBIAN_FRONTEND=noninteractive apt-get install -y -q --no-install-recommends /package/main/*.deb' >/dev/null
    ;;
fedora)
    enter "$fresh" 0 sh -c 'dnf -y -q install --setopt=install_weak_deps=False /package/main/*.rpm' >/dev/null
    ;;
arch)
    enter "$fresh" 0 sh -c 'pacman-key --init && pacman-key --populate archlinux && pacman -Syu --noconfirm && pacman -U --noconfirm /package/main/*.pkg.tar.zst' >/dev/null
    ;;
esac
echo "== $distribution: skwir alone"
enter "$fresh" 0 /tools/package/linux/check.sh minimal
case "$distribution" in
debian | ubuntu)
    enter "$fresh" 0 sh -c 'DEBIAN_FRONTEND=noninteractive apt-get install -y -q /package/extra/*.deb' >/dev/null
    ;;
fedora)
    enter "$fresh" 0 sh -c 'dnf -y -q install /package/extra/*.rpm' >/dev/null
    ;;
arch)
    enter "$fresh" 0 sh -c 'pacman -S --noconfirm --needed gst-plugins-base poppler-glib djvulibre && pacman -U --noconfirm /package/extra/*.pkg.tar.zst' >/dev/null
    ;;
esac
echo "== $distribution: with the others and what they recommend"
enter "$fresh" 0 /tools/package/linux/check.sh
# Only packages that passed both installations leave the test root.
for package in "$fresh"/package/main/* "$fresh"/package/extra/*; do
    cp "$package" "$out/"
    echo "built and checked: $out/$(basename "$package")"
done
chmod -R u+rwX "$fresh"
rm -rf "${fresh:?}"
