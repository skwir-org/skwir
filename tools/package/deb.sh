#!/bin/sh
# Debian's and Ubuntu's package (ADR-040), made on the distribution it is
# for, whose libraries it depends on: the programs shipped
# (tools/package/programs) built for shipping, then cargo-deb, from
# crates/bibli/Cargo.toml's [package.metadata.deb].
#
#     tools/package/deb.sh        # into target/debian/
#
# Needs rustup's Rust (a distribution's may be older than bibli needs),
# cargo-deb (`cargo install cargo-deb`) and what
# tools/package/linux/build-deps.debian lists.
set -eu
cd "$(dirname "$0")/../.."
if ! command -v cargo-deb >/dev/null; then
    echo "cargo-deb is needed: cargo install cargo-deb" >&2
    exit 1
fi
# shellcheck disable=SC2046 # one `-p name` per program
cargo build --locked --profile dist $(sed 's/^/-p /' tools/package/programs)
cargo deb --locked -p bibli --no-build
