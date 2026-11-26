#!/bin/sh
# In the Ubuntu root (build.sh): skwird, skwir's index of names (ADR-050),
# for the tarball alone (ADR-052, amended 2026-11-26). The AppImage
# carries none: its mount ends with skwir, and skwird outlives its last
# client. Built from its release's source, which tools/package/skwird.lock
# pins and appimage.sh fetched into /inputs, its sum checked again here
# (tools/package/skwird.py), against the root's glibc, as skwir is:
#
# - the crates its Cargo.lock names (26 from crates.io, beside its own
#   7) fetched (`cargo fetch --locked`), the network's one use here, then
#   built offline (`--frozen`), as skwird's release builds it (`--profile
#   dist`: no debug information, stripped), its target kept for the next
#   build (the source unpacked anew with the archive's dates, its crates
#   are found unchanged);
# - with the root's Rust, skwir's channel (RUSTUP_TOOLCHAIN), not the
#   one skwird's rust-toolchain.toml would have rustup download with
#   every target its checks build for;
# - with the paths of the build rewritten as skwird's release rewrites
#   them (the crates' sources under /cargo, Rust's own under /rust, the
#   tree under /skwird, and what build scripts write, kept apart for the
#   next build, under /skwird/target): none of the root's folders is left
#   in it.
#
# Into /build/skwird/carried: usr/bin/skwird, beside skwir, where skwir
# looks for it after PATH, and usr/share/licenses/skwird, its licences and
# its crates' notices (tools/package/notices/skwird).
#
#     tools/package/appimage/skwird.sh
set -eu
work=/build/skwird
tree="$(python3 tools/package/skwird.py source "$work" --downloads /inputs)"
RUSTUP_TOOLCHAIN="$(sed -n 's/^channel = "\(.*\)"$/\1/p' rust-toolchain.toml)"
export RUSTUP_TOOLCHAIN
export CARGO_TARGET_DIR="$work/target" CARGO_INCREMENTAL=0
cargo_home="${CARGO_HOME:-$HOME/.cargo}"
sysroot="$(rustc --print sysroot)"
CARGO_ENCODED_RUSTFLAGS="$(printf -- '--remap-path-prefix=%s=/cargo\037--remap-path-prefix=%s=/rust\037--remap-path-prefix=%s=/skwird\037--remap-path-prefix=%s=/skwird/target' \
    "$cargo_home" "$sysroot" "$tree" "$CARGO_TARGET_DIR")"
export CARGO_ENCODED_RUSTFLAGS
(cd "$tree" && cargo fetch --locked && cargo build --frozen --profile dist -p skwird --bin skwird)
program="$CARGO_TARGET_DIR/dist/skwird"
for folder in "$cargo_home" "$sysroot" "$tree" "$work" "$HOME/"; do
    if grep -q -a -F "$folder" "$program"; then
        echo "skwird names $folder, a folder of the root that built it" >&2
        exit 1
    fi
done
carried="$work/carried"
rm -rf "$carried"
install -Dm755 "$program" "$carried/usr/bin/skwird"
mkdir -p "$carried/usr/share/licenses"
cp -R "$work/licenses/skwird" "$carried/usr/share/licenses/"
