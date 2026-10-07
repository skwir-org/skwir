# bibli task runner. Run `just` to list recipes.

default:
    @just --list

# Build the default members (no native workers; see the root Cargo.toml).
build:
    cargo build

# Build the optional native workers (Poppler, MuPDF, GStreamer, libarchive).
workers:
    cargo build -p bibli-poppler-worker -p bibli-mupdf-worker -p bibli-media-worker -p bibli-archive-worker

# Build everything, native workers included.
build-all:
    cargo build --workspace

# Build what is shipped: bibli and every helper, without debug info, into
# target/dist (the `dist` profile). Helpers sit beside bibli, where it finds them.
dist:
    cargo build --profile dist $(sed 's/^/-p /' tools/package/programs)

# Build the plugins from outside bibli this workspace holds as WebAssembly
# modules, into target/plugins (ADR-046): BIBLI_PLUGIN_PATH=$PWD/target/plugins.
plugins:
    tools/plugins/build.sh

# Run the tests of the default members.
test:
    cargo nextest run --no-tests=pass

# Run every test, native workers included (what CI runs).
test-all:
    cargo nextest run --workspace --no-tests=pass

# Exercise real Poppler with synthetic text, geometry and navigation fixtures.
poppler-check:
    cargo build -p bibli-poppler-worker
    python3 tools/pdf/check.py --engine poppler --worker target/debug/bibli-poppler-worker

# Exercise MuPDF with the same synthetic fixtures (its first build compiles MuPDF).
mupdf-check:
    cargo build -p bibli-mupdf-worker
    python3 tools/pdf/check.py --engine mupdf --worker target/debug/bibli-mupdf-worker

# Exercise GStreamer playback using a synthetic two-track video and subtitles.
media-check:
    cargo build -p bibli-media-worker
    python3 tools/media/check.py

# Exercise audio and archive helpers against generated inputs outside the repo.
previews-check:
    cargo build -p bibli-media-worker -p bibli-archive-worker
    python3 tools/previews/check.py --temporary

# Run the vendored Blade atlas regression tests with a real GPU context.
atlas-check:
    python3 tools/gpui/check-atlas.py

# Run the vendored GPUI tests that cover our list and text patches.
list-check:
    python3 tools/gpui/check-list.py

# Lint: rustfmt check + clippy with warnings as errors (default members).
lint:
    cargo fmt --all -- --check
    cargo clippy --all-targets -- -D warnings

# Lint everything, native workers and the Windows and macOS builds included (CI).
lint-all: && check-cross
    cargo fmt --all -- --check
    cargo clippy --workspace --all-targets -- -D warnings

# Phase 5 (ADR-035): Zig is the C compiler of these builds, `target/cross`
# their build directory (not incremental: checks, rarely rebuilt in a loop,
# whose caches only fill the disk), and nothing is run. On macOS bibli-ui
# and bibli are left out: gpui's Metal backend generates its bindings from
# Apple's SDK.

# Compile and lint the Windows and macOS builds from Linux.
check-cross: check-windows check-macos

# Compile and lint the Windows build (default members, tests included).
check-windows:
    CARGO_TARGET_DIR=target/cross CARGO_INCREMENTAL=0 cargo-zigbuild clippy --target x86_64-pc-windows-gnu --all-targets -- -D warnings

# Compile and lint the macOS build, but for the crates built on gpui.
check-macos:
    CARGO_TARGET_DIR=target/cross CARGO_INCREMENTAL=0 cargo-zigbuild clippy --target aarch64-apple-darwin --all-targets --workspace --exclude bibli-ui --exclude bibli --exclude bibli-poppler-worker --exclude bibli-mupdf-worker --exclude bibli-media-worker --exclude bibli-archive-worker -- -D warnings

# Run the Windows build's tests under Wine (args go to nextest: `-p bibli-core`).
test-windows *args:
    tools/cross/test-windows.sh {{args}}

# Linux's package (Phase 5, step K): bibli and its helpers, by Nix, from the git tree.
package-nix:
    nix build "git+file://{{justfile_directory()}}#default"

# Debian's and Ubuntu's package, made on the distribution it is for
# (ADR-040), into target/debian.
package-deb:
    tools/package/deb.sh

# The RPM package, made on Fedora or the distribution it is for (ADR-040),
# into target/generate-rpm.
package-rpm:
    tools/package/rpm.sh

# The AUR package (ADR-040): its .SRCINFO beside the PKGBUILD, which the
# AUR wants with it (makepkg, from pacman: on Arch).
package-aur:
    cd tools/package/aur && makepkg --printsrcinfo > .SRCINFO

# A Linux package built in a distribution's own root and installed in a
# fresh one, from any Linux with bubblewrap (ADR-040): debian, fedora, arch.
package-try distribution:
    tools/package/try.sh {{distribution}}

# An AppImage of that package (nix-appimage), into result-appimage.
package-appimage:
    nix bundle --bundler github:ralismark/nix-appimage -o result-appimage "git+file://{{justfile_directory()}}#default"

# Format the workspace.
fmt:
    cargo fmt --all

# Generate the synthetic benchmark trees under bench-data/ (idempotent).
bench-gen *profiles:
    cargo run --release -p bibli-bench -- gen {{profiles}}

# Print raw filesystem measurements as JSON.
bench-list *profiles:
    cargo run --release -p bibli-bench -- list {{profiles}}

# Fail on any performance budget regression (what CI runs).
bench-check:
    cargo run --release -p bibli-bench -- check

# Idle memory and first frame (ADR-033) on this display; close other bibli first.
idle-pss:
    cargo build --release -p bibli
    tools/measure/idle-pss.py

# Build both toolkit spikes.
spike-build:
    cd spikes && cargo build --release

# Run a spike: `just spike gpui nvidia` or `just spike floem intel`.
spike toolkit gpu="intel" rows="100000":
    ./spikes/run.sh {{toolkit}} {{gpu}} {{rows}}

# Run the app (release) on a directory, default: the current one.
run dir=".":
    cargo build --release -p bibli -p bibli-pdf-worker -p bibli-poppler-worker -p bibli-mupdf-worker -p bibli-media-worker -p bibli-archive-worker -p bibli-mermaid-worker -p bibli-model-worker -p bibli-module-worker
    ./target/release/bibli {{dir}}

# Profile the app with samply (opens the Firefox Profiler UI when done).
flame dir=".":
    cargo build --release -p bibli -p bibli-pdf-worker -p bibli-poppler-worker -p bibli-mupdf-worker -p bibli-media-worker -p bibli-archive-worker -p bibli-mermaid-worker -p bibli-model-worker -p bibli-module-worker
    samply record ./target/release/bibli {{dir}}

# Profile a bench profile's listing with a flamegraph SVG.
flame-bench profile="flat100k":
    cargo flamegraph --release -p bibli-bench -o flamegraph-{{profile}}.svg -- list {{profile}}

# Watch and re-check on change.
watch:
    bacon clippy
