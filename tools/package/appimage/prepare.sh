#!/bin/sh
# Run only inside the Ubuntu build root, by appimage.sh or CI.
set -eu
# shellcheck disable=SC1091 # provided by the Ubuntu build root
. /etc/os-release
[ "$ID:$VERSION_ID" = ubuntu:22.04 ] || { echo 'Build the AppImage on Ubuntu 22.04' >&2; exit 1; }
key="$(cat tools/package/linux/build-deps.debian tools/package/appimage/build-deps rust-toolchain.toml | sha256sum | cut -d' ' -f1)"
# Refresh signed distribution packages even when the build root is cached.
# shellcheck disable=SC2046 # one argument per dependency
sh tools/package/appimage/install-deps.sh \
    $(cat tools/package/linux/build-deps.debian tools/package/appimage/build-deps)
if [ "$(cat /root/.skwir-appimage-prepared 2>/dev/null || true)" != "$key" ]; then
    if ! command -v rustup >/dev/null; then
        curl -fsSL https://sh.rustup.rs | sh -s -- -y -q --profile minimal --default-toolchain none
    fi
    channel="$(sed -n 's/^channel = "\(.*\)"$/\1/p' rust-toolchain.toml)"
    rustup toolchain install "$channel" --profile minimal --component clippy,rustfmt
    printf '%s\n' "$key" > /root/.skwir-appimage-prepared
    apt-get clean
fi
