#!/bin/sh
# The RPM package (ADR-040), made on the distribution it is for (Fedora,
# openSUSE…), whose libraries it depends on: the programs shipped
# (tools/package/programs) built for shipping, then cargo-generate-rpm,
# from crates/bibli/Cargo.toml's [package.metadata.generate-rpm].
#
#     tools/package/rpm.sh        # into target/generate-rpm/
#
# Needs rustup's Rust (a distribution's may be older than bibli needs),
# cargo-generate-rpm (`cargo install cargo-generate-rpm`) and what
# tools/package/linux/build-deps.fedora lists (named as Fedora names them).
set -eu
cd "$(dirname "$0")/../.."
if ! command -v cargo-generate-rpm >/dev/null; then
    echo "cargo-generate-rpm is needed: cargo install cargo-generate-rpm" >&2
    exit 1
fi
# shellcheck disable=SC2046 # one `-p name` per program
cargo build --locked --profile dist $(sed 's/^/-p /' tools/package/programs)
cargo generate-rpm -p crates/bibli --profile dist
