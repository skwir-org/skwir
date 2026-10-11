# skwir task runner. Run `just` to list recipes.

default:
    @just --list

# Build the default members (no native workers; see the root Cargo.toml).
build:
    cargo build

# Build the optional native workers (Poppler, MuPDF, GStreamer, libarchive).
workers:
    cargo build -p skwir-poppler-worker -p skwir-mupdf-worker -p skwir-media-worker -p skwir-archive-worker

# Build everything, native workers included.
build-all:
    cargo build --workspace

# Build what is shipped: skwir and every helper, without debug info, into
# target/dist (the `dist` profile). Helpers sit beside skwir, where it finds them.
dist:
    cargo build --profile dist $(sed 's/^/-p /' tools/package/programs)

# Build the plugins from outside skwir this workspace holds as WebAssembly
# modules, into target/plugins (ADR-046): SKWIR_PLUGIN_PATH=$PWD/target/plugins.
plugins:
    tools/plugins/build.sh

# Run the tests of the default members.
test:
    cargo nextest run --no-tests=pass

# Run every test, native workers included (what CI runs).
test-all:
    cargo nextest run --workspace --no-tests=pass

# The real window on an X display of its own, driven by a script and checked
# on disk (tools/test/smoke.py): Xvfb and Mesa's software Vulkan come from
# the flake's nixpkgs, so no display or GPU is needed.
smoke:
    cargo build --release -p skwir
    tools/test/smoke.py \
        --xvfb "$(nix build --no-link --print-out-paths --inputs-from 'git+file://{{justfile_directory()}}' nixpkgs#xorg.xorgserver)/bin/Xvfb" \
        --icd "$(nix build --no-link --print-out-paths --inputs-from 'git+file://{{justfile_directory()}}' nixpkgs#mesa)/share/vulkan/icd.d/lvp_icd.x86_64.json"

# Run after the window changes, and look at docs/images/ before committing
# (tools/screenshots/readme.py: an X display of its own, synthetic files).
# Make the README's screenshots again, light and dark.
screenshots *args:
    cargo build --release -p skwir
    tools/screenshots/readme.py {{args}}

# Exercise real Poppler with synthetic text, geometry and navigation fixtures.
poppler-check:
    cargo build -p skwir-poppler-worker
    python3 tools/pdf/check.py --engine poppler --worker target/debug/skwir-poppler-worker

# Exercise MuPDF with the same synthetic fixtures (its first build compiles MuPDF).
mupdf-check:
    cargo build -p skwir-mupdf-worker
    python3 tools/pdf/check.py --engine mupdf --worker target/debug/skwir-mupdf-worker

# Exercise GStreamer playback using a synthetic two-track video and subtitles.
media-check:
    cargo build -p skwir-media-worker
    python3 tools/media/check.py

# Exercise audio and archive helpers against generated inputs outside the repo.
previews-check:
    cargo build -p skwir-media-worker -p skwir-archive-worker
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

# The dependencies against deny.toml: their licences, known advisories
# (read from RustSec's database, fetched), and where they come from (CI).
deny:
    cargo deny check

# Lint everything, native workers and the Windows and macOS builds included (CI).
lint-all: && check-cross
    cargo fmt --all -- --check
    cargo clippy --workspace --all-targets -- -D warnings

# Phase 5 (ADR-035): Zig is the C compiler of these builds, `target/cross`
# their build directory (not incremental: checks, rarely rebuilt in a loop,
# whose caches only fill the disk), and nothing is run. On macOS skwir-ui
# and skwir are left out: gpui's Metal backend generates its bindings from
# Apple's SDK.

# Compile and lint the Windows and macOS builds from Linux.
check-cross: check-windows check-macos

# Compile and lint the Windows build (default members, tests included).
check-windows:
    CARGO_TARGET_DIR=target/cross CARGO_INCREMENTAL=0 cargo-zigbuild clippy --target x86_64-pc-windows-gnu --all-targets -- -D warnings

# Compile and lint the macOS build, but for the crates built on gpui.
check-macos:
    CARGO_TARGET_DIR=target/cross CARGO_INCREMENTAL=0 cargo-zigbuild clippy --target aarch64-apple-darwin --all-targets --workspace --exclude skwir-ui --exclude skwir --exclude skwir-poppler-worker --exclude skwir-mupdf-worker --exclude skwir-media-worker --exclude skwir-archive-worker -- -D warnings

# Run the Windows build's tests under Wine (args go to nextest: `-p skwir-core`).
test-windows *args:
    tools/cross/test-windows.sh {{args}}

# Linux's package (Phase 5, step K): skwir and its helpers, by Nix, from the git tree.
package-nix:
    nix build "git+file://{{justfile_directory()}}#default"

# The notices of the crates each package's programs link, into
# target/notices (the package scripts make them; cargo-about, offline).
notices:
    tools/package/notices.py

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
    cargo run --release -p skwir-bench -- gen {{profiles}}

# Print raw filesystem measurements as JSON.
bench-list *profiles:
    cargo run --release -p skwir-bench -- list {{profiles}}

# Fail on any performance budget regression (what CI runs).
bench-check:
    cargo run --release -p skwir-bench -- check

# Idle memory and first frame (ADR-033) on this display; close other skwir first.
idle-pss:
    cargo build --release -p skwir
    tools/measure/idle-pss.py

# Build both toolkit spikes.
spike-build:
    cd spikes && cargo build --release

# Run a spike: `just spike gpui nvidia` or `just spike floem intel`.
spike toolkit gpu="intel" rows="100000":
    ./spikes/run.sh {{toolkit}} {{gpu}} {{rows}}

# Run the app (release) on a directory, default: the current one.
run dir=".":
    cargo build --release -p skwir -p skwir-pdf-worker -p skwir-poppler-worker -p skwir-mupdf-worker -p skwir-media-worker -p skwir-archive-worker -p skwir-mermaid-worker -p skwir-model-worker -p skwir-module-worker
    ./target/release/skwir {{dir}}

# Profile the app with samply (opens the Firefox Profiler UI when done).
flame dir=".":
    cargo build --release -p skwir -p skwir-pdf-worker -p skwir-poppler-worker -p skwir-mupdf-worker -p skwir-media-worker -p skwir-archive-worker -p skwir-mermaid-worker -p skwir-model-worker -p skwir-module-worker
    samply record ./target/release/skwir {{dir}}

# Profile a bench profile's listing with a flamegraph SVG.
flame-bench profile="flat100k":
    cargo flamegraph --release -p skwir-bench -o flamegraph-{{profile}}.svg -- list {{profile}}

# Watch and re-check on change.
watch:
    bacon clippy
