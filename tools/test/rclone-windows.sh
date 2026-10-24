#!/bin/sh
# rclone's own Windows release, for the tests `just test-windows` runs under
# Wine (ADR-086): its remote control and configuration tried as Windows has
# them. Downloaded once into the tests' cache, its sum checked against the
# one pinned here (rclone's SHA256SUMS for the release), and its path
# printed. Nothing of it is shipped.
set -eu
version=1.75.2
sum=8a9db2bad1dc79a65b3348fd857069598e2aab7cf4d118b77b1df23055aff818
asset="rclone-v$version-windows-amd64.zip"
dir="$HOME/.cache/skwir-tests/rclone-windows/v$version"
exe="$dir/rclone-v$version-windows-amd64/rclone.exe"
if [ ! -x "$exe" ]; then
    mkdir -p "$dir"
    curl -fsSL -o "$dir/$asset" "https://downloads.rclone.org/v$version/$asset"
    got="$(sha256sum "$dir/$asset" | cut -d' ' -f1)"
    if [ "$got" != "$sum" ]; then
        rm -f "$dir/$asset"
        echo "$asset: its sum is not the one pinned" >&2
        exit 1
    fi
    (cd "$dir" && bsdtar -xf "$asset" "rclone-v$version-windows-amd64/rclone.exe")
    chmod +x "$exe"
fi
printf '%s\n' "$exe"
