#!/bin/sh
# Debian's and Ubuntu's packages (ADR-040, ADR-041), made on the
# distribution they are for, whose libraries they depend on: the programs
# shipped (tools/package/programs) built for shipping, PDFium fetched
# (tools/package/pdfium.sh), then cargo-deb, from crates/skwir/Cargo.toml's
# [package.metadata.deb]: skwir, and skwir-media, skwir-poppler,
# skwir-mermaid and skwir-mupdf (its variants).
#
#     tools/package/deb.sh        # into target/debian/
#
# Needs rustup's Rust (a distribution's may be older than skwir needs),
# cargo-deb (`cargo install cargo-deb`), cargo-about (`cargo install
# --locked cargo-about`, for each package's notices) and what
# tools/package/linux/build-deps.debian lists.
set -eu
cd "$(dirname "$0")/../.."
if ! command -v cargo-deb >/dev/null; then
    echo "cargo-deb is needed: cargo install cargo-deb" >&2
    exit 1
fi
if ! command -v cargo-about >/dev/null; then
    echo "cargo-about is needed: cargo install --locked cargo-about" >&2
    exit 1
fi
case "$(uname -m)" in
x86_64) platform=linux-x64 ;;
aarch64) platform=linux-arm64 ;;
*) echo "no PDFium is pinned for $(uname -m)" >&2; exit 1 ;;
esac
# shellcheck disable=SC2046 # one `-p name` per program
cargo build --locked --profile dist $(sed 's/^/-p /' tools/package/programs)
tools/package/notices.py
tools/package/pdfium.sh "$platform" target/pdfium
cargo deb --locked -p skwir --no-build
for variant in media poppler mermaid mupdf; do
    cargo deb --locked -p skwir --no-build --variant "$variant"
done
