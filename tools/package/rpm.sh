#!/bin/sh
# The RPM packages (ADR-040, ADR-041), made on the distribution they are
# for (Fedora, openSUSE…), whose libraries they depend on: the programs
# shipped (tools/package/programs) built for shipping, PDFium fetched
# (tools/package/pdfium.sh), then cargo-generate-rpm, from
# crates/bibli/Cargo.toml's [package.metadata.generate-rpm]: bibli, and
# bibli-media, bibli-poppler, bibli-mermaid and bibli-mupdf (its variants).
#
#     tools/package/rpm.sh        # into target/generate-rpm/
#
# Needs rustup's Rust (a distribution's may be older than bibli needs),
# cargo-generate-rpm (`cargo install cargo-generate-rpm`), cargo-about
# (`cargo install --locked cargo-about`, for each package's notices) and what
# tools/package/linux/build-deps.fedora lists (named as Fedora names them).
set -eu
cd "$(dirname "$0")/../.."
if ! command -v cargo-generate-rpm >/dev/null; then
    echo "cargo-generate-rpm is needed: cargo install cargo-generate-rpm" >&2
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
cargo generate-rpm -p crates/bibli --profile dist
for variant in media poppler mermaid mupdf; do
    cargo generate-rpm -p crates/bibli --profile dist --variant "$variant"
done
