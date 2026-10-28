#!/usr/bin/env bash
# GVfs's AFC mount (an iPhone's) against a stand-in device on this machine
# (tools/test/afc-standin.py: usbmuxd's socket, lockdownd, AFC over a
# folder), for `remote_folders`'s gvfs_afc_takes_by_gio_what_it_does_better:
#   tools/test/gvfs-afc.sh <remote_folders test binary>
# libimobiledevice reaches it through USBMUXD_SOCKET_ADDRESS; the ways it
# checks were chosen by the requests each makes, which the stand-in counts
# (ADR-078). A GVfs of its own, in user, mount and pid namespaces: its bus,
# its daemons, its bridge and the stand-in end with them. Never the
# session's bus, GVfs or usbmuxd.
set -euo pipefail

if [ "${1:-}" != --inside ]; then
    tested=$(realpath "$1")
    # The flake's nixpkgs, as its lock pins it: never the registry's.
    repo=$(realpath "$(dirname "$0")/../..")
    tools=$(nix build --no-link --print-out-paths --inputs-from "git+file://$repo" nixpkgs#gvfs 'nixpkgs#glib^bin' nixpkgs#dbus \
        nixpkgs#coreutils nixpkgs#util-linux)
    out() { grep -E "/[a-z0-9]{32}-$1-[0-9.]+(p[0-9]+)?(-bin)?$" <<< "$tools" | head -1; }
    path=""
    for p in glib coreutils util-linux dbus; do
        path="$path$(out "$p")/bin:"
    done
    scratch=$(mktemp -d "${TMPDIR:-/tmp}/skwir-afc-XXXXXX")
    status=0
    env PATH="${path}$PATH" GVFS="$(out gvfs)" RUNTIME="$scratch" TEST_BINARY="$tested" \
        STANDIN="$(realpath "$(dirname "$0")/afc-standin.py")" \
        timeout 300 unshare -r -m -p --fork --propagation private "$0" --inside \
        || status=$?
    rmdir "$scratch"
    exit "$status"
fi

# Inside: a tmpfs of its own on RUNTIME, the stand-in, the bus, GVfs, then
# the test. RUNTIME, TEST_BINARY, GVFS, STANDIN come from the outer half.
mount -t tmpfs tmpfs "$RUNTIME"
rt="$RUNTIME/rt"
mkdir -m 700 "$rt"
mkdir -p "$RUNTIME/home" "$RUNTIME/phone/DCIM" "$RUNTIME/xdg/config" "$RUNTIME/xdg/cache" \
    "$RUNTIME/xdg/data"
export HOME="$RUNTIME/home" XDG_RUNTIME_DIR="$rt"
export XDG_DATA_DIRS="$GVFS/share" GIO_EXTRA_MODULES="$GVFS/lib/gio/modules"
export GIO_USE_VOLUME_MONITOR=unix USBMUXD_SOCKET_ADDRESS="UNIX:$RUNTIME/usbmuxd"
python3 -I "$STANDIN" "$RUNTIME/usbmuxd" "$RUNTIME/phone" "$RUNTIME/afc.log" \
    2> "$RUNTIME/standin.log" &
cat > "$RUNTIME/bus.conf" <<EOF
<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <type>session</type>
  <listen>unix:dir=$rt</listen>
  <servicedir>$GVFS/share/dbus-1/services</servicedir>
  <policy context="default">
    <allow send_destination="*" eavesdrop="true"/>
    <allow eavesdrop="true"/>
    <allow own="*"/>
  </policy>
</busconfig>
EOF
exec 3< <(dbus-daemon --config-file="$RUNTIME/bus.conf" --nofork --print-address=1 \
    2>"$RUNTIME/bus.log")
read -r address <&3
export DBUS_SESSION_BUS_ADDRESS="$address"
# The stand-in's socket, then its device.
for _ in $(seq 50); do
    [ -S "$RUNTIME/usbmuxd" ] && break
    sleep 0.1
done
udid=0123456789abcdef0123456789abcdef01234567
if ! gio mount "afc://$udid/" < /dev/null; then
    echo "GVfs could not mount the stand-in iPhone" >&2
    cat "$RUNTIME/standin.log" >&2
    exit 1
fi
folder="$(ls -d "$rt"/gvfs/afc*)/skwir-test"
mkdir "$folder"
status=0
XDG_CONFIG_HOME="$RUNTIME/xdg/config" XDG_CACHE_HOME="$RUNTIME/xdg/cache" \
    XDG_DATA_HOME="$RUNTIME/xdg/data" SKWIR_TEST_GVFS_AFC="$folder" \
    "$TEST_BINARY" --exact gvfs_afc_takes_by_gio_what_it_does_better --nocapture || status=$?
gio mount -u "afc://$udid/" < /dev/null || true
exit "$status"
