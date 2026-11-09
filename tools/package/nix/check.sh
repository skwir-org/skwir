#!/bin/sh
# What the Nix package's variants must hold (ADR-124), each built from the
# git tree: `minimal` and `default` built, `full` evaluated. In each built
# one, every helper it carries is in libexec/skwir, answers `--build` as
# skwir's own build and `--probe` (the PDFium helper with the library
# skwir's wrapper names, the player with the plugins its own wrapper
# names); one that needs FUSE (tools/package/fuse) may say this system has
# none once it has answered `--build`. No path of the
# Rust toolchain is in its closure, nor a library of a helper it leaves
# out; skwir opens the window's libraries through its own search path,
# not LD_LIBRARY_PATH, and names no GStreamer plugins for what it starts;
# its fonts, notices, desktop entry, metainfo and icon are there; and it
# checks a settings file without a window. Each closure's
# size is printed, for the STATUS-LOG.
#
#     tools/package/nix/check.sh [variant…]    (default: minimal default)
set -eu
root="$(cd "$(dirname "$0")/../../.." && pwd)"
here="$root/tools/package"
flake="git+file://$root"
system="$(nix eval --raw --impure --expr builtins.currentSystem)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
failed=0
[ "$#" -gt 0 ] || set -- minimal default

# What each flag brings, by store name, as package.nix lists it: a variant
# without the flag holds none of these.
libraries() {
    case "$1" in
    skwir-poppler-worker) echo "poppler-glib" ;;
    skwir-djvulibre-worker) echo "djvulibre" ;;
    skwir-archive-worker) echo "libarchive" ;;
    skwir-media-worker) echo "gstreamer gst-plugins-base" ;;
    skwir-pdf-worker) echo "pdfium" ;;
    esac
}
# A variable the wrapper sets by default (`--set-default`), as makeWrapper
# writes it: export NAME=${NAME-'value'}.
wrapped() {
    sed -n "s/^export $2=\${$2-'\(.*\)'}\$/\1/p" "$1"
}

# The helpers a flag builds (package.nix's `features`); every other one is
# in every variant.
flagged="$(sed -n '/features = {/,/enabled =/p' "$here/nix/package.nix" |
    grep -o '"skwir-[a-z-]*-worker"' | tr -d '"')"

nix eval --raw "$flake#packages.$system.full.drvPath" >/dev/null && echo "ok: full evaluates"

for variant in "$@"; do
    out="$work/$variant"
    nix build "$flake#packages.$system.$variant" -o "$out"
    out="$(readlink -f "$out")"
    echo "== $variant: $out"
    nix path-info -rSh "$out" | tail -1 | sed 's/^/closure: /'
    closure="$(nix path-info -r "$out")"

    if printf '%s\n' "$closure" | grep -E -- '-rust-(minimal|default|std|src|docs)-|-rustc-[0-9]|-cargo-[0-9]'; then
        echo "the toolchain is in the closure"
        failed=1
    fi

    build="$("$out/bin/skwir" --version | sed 's/^skwir //')"
    echo "ok: $out/bin/skwir ($build)"
    launcher="$out/bin/skwir"
    [ -e "$out/bin/.skwir-wrapped" ] && launcher="$out/bin/.skwir-wrapped"
    if readelf -d "$launcher" | grep -q 'vulkan-loader'; then
        echo "ok: the window's libraries on skwir's own search path"
    else
        echo "missing: the window's libraries on skwir's search path"
        failed=1
    fi
    # The wrapper, where there is one (`minimal` names nothing: its skwir is
    # the program itself, which mentions the variable).
    if [ "$launcher" != "$out/bin/skwir" ] && grep -q LD_LIBRARY_PATH "$out/bin/skwir"; then
        echo "the wrapper sets LD_LIBRARY_PATH"
        failed=1
    fi
    pdfium="$(wrapped "$out/bin/skwir" SKWIR_PDFIUM_LIBRARY)"
    # The player's plugins are its helper's: what skwir starts inherits
    # skwir's environment.
    if [ "$launcher" != "$out/bin/skwir" ] && grep -q GST_PLUGIN "$out/bin/skwir"; then
        echo "the wrapper names GStreamer's plugins for what skwir starts"
        failed=1
    fi
    if [ -x "$out/libexec/skwir/skwir-media-worker" ] &&
        [ -z "$(wrapped "$out/libexec/skwir/skwir-media-worker" GST_PLUGIN_SYSTEM_PATH_1_0)" ]; then
        echo "the player's wrapper names no plugins"
        failed=1
    fi

    ls "$out/bin" | grep -vx -e skwir -e .skwir-wrapped && {
        echo "programs on the PATH beside skwir"
        failed=1
    }
    while read -r program; do
        [ "$program" = skwir ] && continue
        found="$out/libexec/skwir/$program"
        # Linux's alone (ADR-097), as package.nix builds it.
        if [ "$program" = skwir-kmtp-worker ] && [ "$(uname -s)" != Linux ]; then
            continue
        fi
        if [ ! -x "$found" ]; then
            if [ "$variant" = full ] || ! printf '%s\n' "$flagged" | grep -qx "$program"; then
                echo "missing: $program"
                failed=1
            else
                echo "left out: $program"
                for library in $(libraries "$program"); do
                    if printf '%s\n' "$closure" | grep -q -- "-$library-[0-9]"; then
                        echo "  but its $library is in the closure"
                        failed=1
                    fi
                done
            fi
        elif SKWIR_PDFIUM_LIBRARY="$pdfium" "$found" --probe >/dev/null 2>"$work/said"; then
            if [ "$("$found" --build 2>/dev/null)" = "$build" ]; then
                echo "works: $program"
            else
                echo "not skwir's build ($build): $program"
                failed=1
            fi
        elif grep -qx "$program" "$here/fuse" && [ ! -e /dev/fuse ] &&
            [ "$("$found" --build 2>/dev/null)" = "$build" ]; then
            echo "starts, but this system has no FUSE: $program"
        else
            echo "does not work: $program ($(head -c 200 "$work/said" | head -1))"
            failed=1
        fi
    done < "$here/programs"

    for file in share/skwir/fonts/Inter-Regular.ttf share/skwir/fonts/JetBrainsMono-Regular.ttf \
        share/licenses/skwir/LICENSE share/licenses/skwir/fonts/Inter/LICENSE.txt \
        share/licenses/skwir/THIRD-PARTY-NOTICES \
        share/applications/org.skwir.skwir.desktop share/metainfo/org.skwir.skwir.metainfo.xml \
        share/icons/hicolor/scalable/apps/org.skwir.skwir.svg; do
        if [ -s "$out/$file" ]; then
            echo "ok: $file"
        else
            echo "missing: $file"
            failed=1
        fi
    done

    # A settings file checked as Home Manager's module checks it.
    mkdir -p "$work/home" "$work/run"
    printf '[ui]\ntheme = "dark"\n' > "$work/settings.toml"
    if HOME="$work/home" XDG_RUNTIME_DIR="$work/run" \
        "$out/bin/skwir" cmd settings.check "{\"file\":\"$work/settings.toml\"}" >/dev/null; then
        echo "ok: settings checked without a window"
    else
        echo "does not check settings without a window"
        failed=1
    fi
done
exit "$failed"
