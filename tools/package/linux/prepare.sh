#!/bin/sh
# What building a Linux package needs (ADR-040), installed as root on the
# distribution it is for (a CI container, or tools/package/try.sh's
# root): the libraries' development packages (build-deps.<distribution>),
# and for Debian and Fedora rustup's Rust (a distribution's may be older
# than skwir needs) and the packaging tool; Arch builds with its own Rust
# (the PKGBUILD's makedepends) as a user of its own, `builder`.
#
#     tools/package/linux/prepare.sh debian|fedora|arch
set -eu
here="$(cd "$(dirname "$0")" && pwd)"
PATH="$HOME/.cargo/bin:$PATH"
rust() {
    if ! command -v cargo >/dev/null; then
        curl -sSf https://sh.rustup.rs | sh -s -- -y -q --profile minimal --default-toolchain stable
    fi
}
# shellcheck disable=SC2046 # one argument per package named
case "${1:-}" in
debian)
    apt-get update -q
    DEBIAN_FRONTEND=noninteractive apt-get install -y -q --no-install-recommends $(cat "$here/build-deps.debian")
    rust
    command -v cargo-deb >/dev/null || cargo install --locked cargo-deb
    command -v cargo-about >/dev/null || cargo install --locked cargo-about
    ;;
fedora)
    dnf -y -q install --setopt=install_weak_deps=False $(cat "$here/build-deps.fedora")
    rust
    command -v cargo-generate-rpm >/dev/null || cargo install --locked cargo-generate-rpm
    command -v cargo-about >/dev/null || cargo install --locked cargo-about
    ;;
arch)
    pacman -Syu --noconfirm --needed base-devel $(bash -c '. "$1"; printf "%s\n" "${makedepends[@]}" "${depends[@]}"' _ "$here/../aur/PKGBUILD")
    id builder >/dev/null 2>&1 || useradd -m builder
    ;;
*)
    echo "usage: $0 debian|fedora|arch" >&2
    exit 2
    ;;
esac
