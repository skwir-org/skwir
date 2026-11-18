#!/usr/bin/env bash
# Build the plugins from outside skwir this workspace holds
# (crates/plugins-outside/*, ADR-046) as WebAssembly modules, each into a
# folder of target/plugins as an installed plugin is laid out: its
# plugin.toml beside the module it names, and its catalogs (l10n/, protocol
# 2, ADR-139) beside them. Try them with
#   SKWIR_PLUGIN_PATH=$PWD/target/plugins just run
# or copy a folder into ~/.local/share/skwir/plugins.
set -euo pipefail
root=$(cd "$(dirname "$0")/../.." && pwd)
out="$root/target/plugins"
mkdir -p "$out"
for crate in "$root"/crates/plugins-outside/*/; do
    manifest="$crate/plugin.toml"
    [ -f "$manifest" ] || continue
    package=$(sed -n 's/^name = "\(.*\)"$/\1/p' "$crate/Cargo.toml" | head -1)
    id=$(sed -n 's/^id = "\(.*\)"$/\1/p' "$manifest" | head -1)
    module=$(sed -n 's/^module = "\(.*\)"$/\1/p' "$manifest" | head -1)
    # An id is lowercase letters, digits and dashes (as skwir checks it):
    # nothing else names a folder here, nor what is removed in it.
    case "$id" in
        "" | *[!a-z0-9-]* | -*)
            echo "$crate: no plugin id skwir takes, left out" >&2
            continue
            ;;
    esac
    # Without debug information: the module is what the plugin ships.
    CARGO_PROFILE_RELEASE_DEBUG=false cargo rustc --release --lib --crate-type cdylib \
        --target wasm32-unknown-unknown -p "$package" -- -C strip=debuginfo
    built="$root/target/wasm32-unknown-unknown/release/${package//-/_}.wasm"
    mkdir -p "$out/$id"
    cp "$manifest" "$out/$id/plugin.toml"
    cp "$built" "$out/$id/$module"
    # Its catalogs as they are now: none left from an earlier build.
    rm -rf "$out/$id/l10n"
    if [ -d "$crate/l10n" ]; then
        mkdir -p "$out/$id/l10n"
        cp "$crate"/l10n/*.ftl "$out/$id/l10n/"
    fi
    echo "$id: $out/$id ($(du -h "$out/$id/$module" | cut -f1))"
done
