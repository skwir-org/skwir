#!/bin/sh
# Debian's and Ubuntu's packages (ADR-040, ADR-041), made on the
# distribution they are for, whose libraries they depend on: the programs
# shipped (tools/package/programs) built for shipping, PDFium fetched
# (tools/package/pdfium.sh), then cargo-deb, from crates/skwir/Cargo.toml's
# [package.metadata.deb]: skwir, and skwir-media, skwir-poppler,
# skwir-djvulibre, skwir-mermaid and skwir-mupdf (its variants).
#
#     tools/package/deb.sh        # into target/debian/
#
# Made on Ubuntu, a package's revision names the release
# (skwir_0.1.0-1ubuntu24.04_amd64.deb), so Ubuntu's are named apart from
# Debian's (skwir_0.1.0-1_amd64.deb) in a release.
#
# Needs rustup's Rust (a distribution's may be older than skwir needs),
# cargo-deb (`cargo install cargo-deb`), cargo-about (`cargo install
# --locked --features cli cargo-about`, for each package's notices) and what
# tools/package/linux/build-deps.debian lists.
set -eu
cd "$(dirname "$0")/../.."
if ! command -v cargo-deb >/dev/null; then
    echo "cargo-deb is needed: cargo install cargo-deb" >&2
    exit 1
fi
if ! command -v cargo-about >/dev/null; then
    echo "cargo-about is needed: cargo install --locked --features cli cargo-about" >&2
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
revision=1
if [ -r /etc/os-release ]; then
    # shellcheck disable=SC1091 # the system's, read where it runs
    os="$(. /etc/os-release && echo "${ID:-}:${VERSION_ID:-}")"
    case "$os" in
    ubuntu:?*) revision="1ubuntu${os#ubuntu:}" ;;
    esac
fi
cargo deb --locked -p skwir --no-build --deb-revision "$revision"
for variant in media poppler djvulibre mermaid mupdf; do
    cargo deb --locked -p skwir --no-build --deb-revision "$revision" --variant "$variant"
done
