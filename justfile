# skwir task runner. Run `just` to list recipes.

default:
    @just --list

# Cheap routine CI: workspace formatting, core/API contracts and release tooling.
ci-check: release-tools-check
    cargo fmt --all --check
    cargo clippy --locked -p skwir-core -p skwir-plugin-api --all-targets -- -D warnings
    cargo nextest run --locked -p skwir-core -p skwir-plugin-api --no-tests=pass

# All required checks before packaging a release (ADR-053).
release-check:
    python3 tools/release/release.py check

# Exercise release validation, isolated staging and draft uploads with fake tools.
release-tools-check:
    python3 -B -m unittest discover -s tools/release -p 'test_*.py'

# Check that tag, crates, package metadata and dated release notes agree.
release-metadata tag:
    python3 tools/release/release.py metadata {{quote(tag)}}

# Check a clean tagged source, build/install-test Linux packages, stage one bundle.
release-build tag:
    python3 tools/release/release.py build {{quote(tag)}}

# Recheck an existing local bundle's identity, asset set and checksums.
release-verify tag:
    python3 tools/release/release.py verify {{quote(tag)}}

# Create/update a GitHub draft from a verified bundle; never publish it.
release-upload tag:
    python3 tools/release/release.py upload {{quote(tag)}}

# Build the default members (no native workers; see the root Cargo.toml).
build:
    cargo build

# Build the optional native workers (Poppler, MuPDF, DjVuLibre, GStreamer, libarchive).
workers:
    cargo build -p skwir-poppler-worker -p skwir-mupdf-worker -p skwir-djvulibre-worker -p skwir-media-worker -p skwir-archive-worker

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

# Run every test, native workers included (full release gate and manual CI).
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

# What a screen reader hears of the real window (tools/test/a11y.py): a
# display, a session bus and the AT-SPI registry of its own, keys pressed,
# the window's accessibility tree read back. Every tool from the flake's
# nixpkgs. `just a11y-check --skwir target/debug/skwir` checks a debug build.
a11y-check *args:
    #!/usr/bin/env bash
    set -euo pipefail
    nixpkgs() { nix build --no-link --print-out-paths --inputs-from 'git+file://{{justfile_directory()}}' "nixpkgs#$1"; }
    case " {{args}} " in *" --skwir "*) ;; *) cargo build --release -p skwir ;; esac
    atspi=$(nixpkgs at-spi2-core)
    python=$(python3 -c 'import sys; print(f"python{sys.version_info[0]}.{sys.version_info[1]}")')
    export PYTHONPATH="$(nixpkgs python3Packages.pygobject3)/lib/$python/site-packages"
    export GI_TYPELIB_PATH="$atspi/lib/girepository-1.0:$(nixpkgs glib.out)/lib/girepository-1.0:$(nixpkgs gobject-introspection.out)/lib/girepository-1.0"
    tools/test/a11y.py \
        --xvfb "$(nixpkgs xorg.xorgserver)/bin/Xvfb" \
        --icd "$(nixpkgs mesa)/share/vulkan/icd.d/lvp_icd.x86_64.json" \
        --atspi "$atspi" --dbus "$(nixpkgs dbus.out)" \
        --xdotool "$(nixpkgs xdotool)/bin/xdotool" {{args}}

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

# Exercise both DjVu readers, DjVuLibre and djvu-rs, with the same synthetic
# documents, made by DjVuLibre's own tools (ADR-061).
djvu-check:
    cargo build -p skwir-djvulibre-worker -p skwir-djvu-worker
    python3 tools/djvu/check.py --engine djvulibre --worker target/debug/skwir-djvulibre-worker
    python3 tools/djvu/check.py --engine djvu-rs --worker target/debug/skwir-djvu-worker

# Remote folders (ADR-077) against servers of rclone's on this machine,
# mounted for real: rclone, sshfs, and a GVfs and a KIO (kio-fuse, ADR-081)
# of the test's own, each on a bus of its own, taken from nixpkgs (never the
# session's); OpenSSH's own server and Samba run as the user, for copies
# through rsync (ADR-078) and SMB. The tests skip what is missing when run
# alone. Then GVfs's NFS and AFP, whose servers run as root, in namespaces
# of their own (tools/test/gvfs-nfs-afp.sh); GVfs's AFC against a stand-in
# iPhone (tools/test/gvfs-afc.sh); and passwords in a keyring of the tests'
# own (ADR-082).
remote-check:
    #!/usr/bin/env bash
    set -euo pipefail
    tools=$(nix build --no-link --print-out-paths nixpkgs#sshfs nixpkgs#gvfs nixpkgs#rclone 'nixpkgs#glib^bin' nixpkgs#openssh nixpkgs#rsync nixpkgs#kdePackages.kio-fuse nixpkgs#kdePackages.kio nixpkgs#kdePackages.kio-extras nixpkgs#kdePackages.kde-cli-tools nixpkgs#samba nixpkgs#uid_wrapper nixpkgs#nss_wrapper)
    out() { grep -E "/[a-z0-9]{32}-$1-[0-9.]+(p[0-9]+)?(-bin)?$" <<< "$tools" | head -1; }
    export PATH="$(out sshfs-fuse)/bin:$(out rclone)/bin:$(out glib)/bin:$(out openssh)/bin:$(out rsync)/bin:$(out kde-cli-tools)/bin:$PATH"
    export SKWIR_TEST_GVFS="$(out gvfs)"
    export SKWIR_TEST_KIO="$(out kio-fuse):$(out kio):$(out kio-extras)"
    # Samba run as the user, believing it is root (ADR-082's SMB tests).
    export SKWIR_TEST_SAMBA="$(out samba):$(out uid_wrapper):$(out nss_wrapper)"
    cargo nextest run -p skwir -p skwir-plugin-remote --no-fail-fast \
        -E 'binary(remote_folders) | package(skwir-plugin-remote)'
    binary=$(cargo nextest list -p skwir -p skwir-plugin-remote -E 'binary(remote_folders)' \
        --message-format json | python3 -c 'import json, sys; print(json.load(sys.stdin)["rust-suites"]["skwir::remote_folders"]["binary-path"])')
    tools/test/gvfs-nfs-afp.sh "$binary"
    tools/test/gvfs-afc.sh "$binary"
    # Servers' passwords in a keyring of the tests' own (ADR-082).
    keyring=$(nix build --no-link --print-out-paths nixpkgs#gnome-keyring)
    PATH="$keyring/bin:$PATH" tools/test/private-keyring.sh cargo nextest run \
        -p skwir-platform -p skwir-plugin-remote --run-ignored only \
        -E 'test(secret) | test(password)'

# A phone over MTP, then as a camera over gPhoto2, through a GVfs of the
# test's own (tools/test/gvfs-mtp.sh): run on purpose alone, it writes to the
# phone plugged in (unlocked, in file transfer mode), in a folder of its own
# that it removes after, each way.
mtp-check:
    #!/usr/bin/env bash
    set -euo pipefail
    cargo nextest list -p skwir -E 'binary(remote_folders)' > /dev/null
    binary=$(cargo nextest list -p skwir -E 'binary(remote_folders)' \
        --message-format json | python3 -c 'import json, sys; print(json.load(sys.stdin)["rust-suites"]["skwir::remote_folders"]["binary-path"])')
    tools/test/gvfs-mtp.sh "$binary"

# Exercise GStreamer playback using a synthetic two-track video and subtitles.
media-check:
    cargo build -p skwir-media-worker
    python3 tools/media/check.py

# Exercise audio and archive helpers against generated inputs outside the repo.
previews-check:
    cargo build -p skwir-media-worker -p skwir-archive-worker
    python3 tools/previews/check.py --temporary

# Run the vendored gpui crates' tests that cover our patches, on a copy of
# vendor/zed (`just gpui-check gpui_wgpu`: one crate; the atlas's and the
# renderer's tests need a wgpu adapter, lavapipe will do).
gpui-check *args:
    python3 tools/gpui/check.py {{args}}

# Run gpui_windows's tests of our patches under Wine, its Direct3D 11 on
# Mesa on an X display of its own (args go to the test binary).
gpui-check-windows *args:
    tools/gpui/check-windows.sh {{args}}

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

# Compile and lint the Windows build (default members and the archive helper, which its package carries; tests included).
check-windows:
    CARGO_TARGET_DIR=target/cross CARGO_INCREMENTAL=0 cargo-zigbuild clippy --target x86_64-pc-windows-gnu --all-targets --workspace --exclude skwir-poppler-worker --exclude skwir-mupdf-worker --exclude skwir-djvulibre-worker --exclude skwir-media-worker -- -D warnings

# xmac (the flake's `.#macos` shell) downloads the SDK from Apple's servers
# into ~/.local/share/skwir-build/macos-sdk; it is never committed or
# shipped. It is under Apple's licence, which xmac shows and asks you to
# accept (or pass --accept-license yourself; `xmac license` prints it in
# full). Arguments go to `xmac splat` (`--arch arm64`). The SDK is 26.5
# unless `--sdk` says another: LLVM's linker (21 to 23) cannot read 27's
# stubs, which name an architecture it does not know (arm64e.x1).
# Apple's macOS SDK, for building for macOS from Linux (you accept Apple's licence).
macos-sdk *args:
    nix develop '{{justfile_directory()}}#macos' --command sh -c 'case " $* " in *" --sdk "*) ;; *) set -- --sdk 26.5 "$@" ;; esac; xmac splat --output "${XDG_DATA_HOME:-$HOME/.local/share}/skwir-build/macos-sdk" "$@"' xmac {{args}}

# Compile and lint the macOS build, but for the crates built on gpui.
check-macos:
    CARGO_TARGET_DIR=target/cross CARGO_INCREMENTAL=0 cargo-zigbuild clippy --target aarch64-apple-darwin --all-targets --workspace --exclude skwir-ui --exclude skwir --exclude skwir-poppler-worker --exclude skwir-mupdf-worker --exclude skwir-djvulibre-worker --exclude skwir-media-worker -- -D warnings

# Run the Windows build's tests under Wine (args go to nextest: `-p skwir-core`).
test-windows *args:
    tools/cross/test-windows.sh {{args}}

# Linux's package (Phase 5, step K): skwir and its helpers, by Nix, from the git tree.
package-nix:
    nix build "git+file://{{justfile_directory()}}#default"

# The notices of the crates each package's programs link, into
# tools/package/notices, kept with the source for the packages to copy
# (ADR-063): made again when a dependency, or what a package ships,
# changes. cargo-about, every platform's crates read offline.
notices:
    tools/package/notices.py

# Whether tools/package/notices is what the crates say now (release-check).
notices-check:
    tools/package/notices.py --check

# Debian's and Ubuntu's package, made on the distribution it is for
# (ADR-040), into target/debian.
package-deb:
    tools/package/deb.sh

# The RPM package, made on Fedora or the distribution it is for (ADR-040),
# into target/generate-rpm.
package-rpm:
    tools/package/rpm.sh

# The AUR packages (ADR-040): skwir, skwir-bin and skwir-git, each .SRCINFO
# beside its PKGBUILD, which the AUR wants with it (makepkg, from pacman in
# the Linux Nix shell).
package-aur:
    cd tools/package/aur && makepkg --printsrcinfo > .SRCINFO
    cd tools/package/aur/skwir-bin && makepkg --printsrcinfo > .SRCINFO
    cd tools/package/aur/skwir-git && makepkg --printsrcinfo > .SRCINFO

# Prepare a verified release's PKGBUILDs and .SRCINFOs in separate AUR checkouts,
# one per package base (cloned over SSH: requires the AUR SSH key already).
aur-prepare tag:
    python3 tools/release/aur.py prepare {{quote(tag)}}

# Commit and push the reviewed files from aur-prepare (requires an AUR SSH key).
aur-publish tag:
    python3 tools/release/aur.py publish {{quote(tag)}}

# A Linux package built in a distribution's own root and installed in a
# fresh one, from any Linux with bubblewrap (ADR-040): debian, ubuntu, fedora,
# arch.
package-try distribution:
    tools/package/try.sh {{distribution}}

# Windows' build to try, made on Linux (ADR-074): skwir and its helpers
# that carry no library, as a zip and a per-user installer (wixl), into
# target/package/windows, then both tried under Wine.
package-windows:
    tools/package/windows.sh
    tools/package/windows-check.sh

# macOS's build to try, made on Linux (ADR-075): skwir.app for Apple silicon,
# signed ad hoc, zipped and in a disk image in target/package/macos, then
# looked over (nothing here runs it). Needs Apple's SDK: `just macos-sdk`.
package-macos:
    tools/package/macos.sh
    tools/package/macos-check.sh

# A portable x86-64 AppImage, and the same tree as a tarball, built in Ubuntu
# 22.04 and checked in a separate runtime-only root; builds on NixOS too. Into
# target/package/appimage.
package-appimage:
    tools/package/appimage.sh

# Format the workspace.
fmt:
    cargo fmt --all

# Generate the synthetic benchmark trees under bench-data/ (idempotent).
bench-gen *profiles:
    cargo run --release -p skwir-bench -- gen {{profiles}}

# Print raw filesystem measurements as JSON.
bench-list *profiles:
    cargo run --release -p skwir-bench -- list {{profiles}}

# Fail on any performance budget regression (full release gate and manual CI).
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
    cargo build --release -p skwir -p skwir-pdf-worker -p skwir-poppler-worker -p skwir-mupdf-worker -p skwir-djvu-worker -p skwir-djvulibre-worker -p skwir-media-worker -p skwir-archive-worker -p skwir-mermaid-worker -p skwir-model-worker -p skwir-module-worker
    ./target/release/skwir {{dir}}

# Profile the app with samply (opens the Firefox Profiler UI when done).
flame dir=".":
    cargo build --release -p skwir -p skwir-pdf-worker -p skwir-poppler-worker -p skwir-mupdf-worker -p skwir-djvu-worker -p skwir-djvulibre-worker -p skwir-media-worker -p skwir-archive-worker -p skwir-mermaid-worker -p skwir-model-worker -p skwir-module-worker
    samply record ./target/release/skwir {{dir}}

# Profile a bench profile's listing with a flamegraph SVG.
flame-bench profile="flat100k":
    cargo flamegraph --release -p skwir-bench -o flamegraph-{{profile}}.svg -- list {{profile}}

# Watch and re-check on change.
watch:
    bacon clippy
