#!/usr/bin/env bash
# A phone offered by GVfs's monitor, opened and let go of (ADR-096), for
# `remote_folders`'s gvfs_offers_a_phone_opens_it_and_lets_go_of_it; then
# over MTP through GVfs (ADR-078, 16), for gvfs_mtp_takes_by_gio_every_way,
# then the same phone as a camera through GVfs's gphoto2 backend (PTP),
# for gvfs_gphoto2_takes_by_gio_up_and_within; then the phone held by KDE's
# kmtpd in a `kiod6` of its own, as on Plasma, opened through Skwir's
# helper (ADR-097), for kmtpd_opens_a_phone_kde_holds_through_skwir_s_helper:
#   tools/test/gvfs-mtp.sh <remote_folders test binary> <skwir-kmtp-worker>
# Run on purpose alone (`just mtp-check`): it writes to the phone plugged
# in, in a folder of its own on its first storage (`skwir-test-…`), which
# it removes after. The phone is unlocked, in file transfer mode, and not
# open in the session's file manager (one program holds it at a time).
# A GVfs of its own, in user, mount and pid namespaces: its bus, its
# daemons and its bridge end with them. Never the session's bus or GVfs.
# Skipped, saying why, without a phone.
set -euo pipefail

if [ "${1:-}" != --inside ]; then
    tested=$(realpath "$1")
    worker=$(realpath "$2")
    serial=""
    for device in /sys/bus/usb/devices/*; do
        [ -f "$device/idVendor" ] || continue
        said=$(udevadm info -q property -p "$device" 2>/dev/null || true)
        if grep -qx ID_MTP_DEVICE=1 <<< "$said"; then
            serial=$(sed -n 's/^ID_SERIAL=//p' <<< "$said")
            # Its USB port, as libgphoto2 names it: `usb:BBB,DDD`.
            port=$(printf '%03d,%03d' "$(cat "$device/busnum")" "$(cat "$device/devnum")")
            break
        fi
    done
    if [ -z "$serial" ]; then
        echo "skipped: no phone over MTP (plugged in, unlocked, in file transfer mode)"
        exit 0
    fi
    # The flake's nixpkgs, as its lock pins it: never the registry's.
    repo=$(realpath "$(dirname "$0")/../..")
    tools=$(nix build --no-link --print-out-paths --inputs-from "git+file://$repo" nixpkgs#gvfs 'nixpkgs#glib^bin' nixpkgs#dbus \
        nixpkgs#coreutils nixpkgs#util-linux nixpkgs#kdePackages.kio \
        nixpkgs#kdePackages.kio-extras)
    out() { grep -E "/[a-z0-9]{32}-$1-[0-9.]+(p[0-9]+)?(-bin)?$" <<< "$tools" | head -1; }
    path=""
    for p in glib coreutils util-linux dbus; do
        path="$path$(out "$p")/bin:"
    done
    scratch=$(mktemp -d "${TMPDIR:-/tmp}/skwir-mtp-XXXXXX")
    status=0
    env PATH="${path}$PATH" GVFS="$(out gvfs)" PHONE="$serial" PORT="$port" RUNTIME="$scratch" \
        TEST_BINARY="$tested" KMTP_WORKER="$worker" KIO="$(out kio)" KIO_EXTRAS="$(out kio-extras)" \
        timeout 900 unshare -r -m -p --fork --propagation private "$0" --inside \
        || status=$?
    rmdir "$scratch"
    exit "$status"
fi

# Inside: a tmpfs of its own on RUNTIME, the bus, GVfs, then the test.
# PHONE, RUNTIME, TEST_BINARY, GVFS come from the outer half's environment.
mount -t tmpfs tmpfs "$RUNTIME"
rt="$RUNTIME/rt"
mkdir -m 700 "$rt"
mkdir -p "$RUNTIME/home" "$RUNTIME/xdg/config" "$RUNTIME/xdg/cache" "$RUNTIME/xdg/data"
export HOME="$RUNTIME/home" XDG_RUNTIME_DIR="$rt"
export XDG_DATA_DIRS="$GVFS/share" GIO_EXTRA_MODULES="$GVFS/lib/gio/modules"
export GIO_USE_VOLUME_MONITOR=unix
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
status=0
# Offered before it is mounted: the plugin finds, opens and lets go of it
# itself, through the monitor this bus starts.
XDG_CONFIG_HOME="$RUNTIME/xdg/config" XDG_CACHE_HOME="$RUNTIME/xdg/cache" \
    XDG_DATA_HOME="$RUNTIME/xdg/data" SKWIR_TEST_GVFS_PHONE="$PORT" \
    "$TEST_BINARY" --exact gvfs_offers_a_phone_opens_it_and_lets_go_of_it --nocapture || status=$?
if ! gio mount "mtp://$PHONE/" < /dev/null; then
    echo "the phone could not be mounted: unlocked, in file transfer mode, closed in the file manager?" >&2
    exit 1
fi
bridge=$(ls -d "$rt"/gvfs/mtp*)
storage=$(find "$bridge" -mindepth 1 -maxdepth 1 -type d | head -1)
folder="$storage/skwir-test-$(date +%s%N)"
mkdir "$folder"
XDG_CONFIG_HOME="$RUNTIME/xdg/config" XDG_CACHE_HOME="$RUNTIME/xdg/cache" \
    XDG_DATA_HOME="$RUNTIME/xdg/data" SKWIR_TEST_GVFS_MTP="$folder" \
    "$TEST_BINARY" --exact gvfs_mtp_takes_by_gio_every_way --nocapture || status=$?
rm -r -- "$folder"
gio mount -u "mtp://$PHONE/" < /dev/null || true

# The same phone as a camera (one program holds it at a time): its
# storage is the camera's top, a folder of its own there.
camera="gphoto2://[usb:$PORT]/"
mounted=no
for try in 1 2 3; do
    if gio mount "$camera" < /dev/null; then
        mounted=yes
        break
    fi
    sleep 2
done
if [ "$mounted" = no ]; then
    echo "the phone could not be mounted as a camera (gPhoto2)" >&2
    exit 1
fi
top=$(ls -d "$rt"/gvfs/gphoto2*)
folder="$top/skwir-test-$(date +%s%N)"
mkdir "$folder"
XDG_CONFIG_HOME="$RUNTIME/xdg/config" XDG_CACHE_HOME="$RUNTIME/xdg/cache" \
    XDG_DATA_HOME="$RUNTIME/xdg/data" SKWIR_TEST_GVFS_GPHOTO2="$folder" \
    "$TEST_BINARY" --exact gvfs_gphoto2_takes_by_gio_up_and_within --nocapture || status=$?
rm -r -- "$folder"
gio mount -u "$camera" < /dev/null || true

# The phone held by KDE's kmtpd, as on Plasma (ADR-097): a `kiod6` of its
# own on a bus of its own, which the test's first question starts and
# loads kmtpd in, as Dolphin would. GVfs's bus stays, its mounts gone.
# kmtpd opens only a phone free as it loads: GVfs's camera program lets go
# of it a moment after its unmount, waited for (at most ten seconds).
node="/dev/bus/usb/${PORT%,*}/${PORT#*,}"
for _ in $(seq 100); do
    held=no
    for fd in /proc/[0-9]*/fd/*; do
        if [ "$(readlink "$fd" 2>/dev/null)" = "$node" ]; then
            held=yes
            break
        fi
    done
    [ "$held" = no ] && break
    sleep 0.1
done
cat > "$RUNTIME/kio-bus.conf" <<EOF
<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <type>session</type>
  <listen>unix:dir=$rt</listen>
  <servicedir>$KIO/share/dbus-1/services</servicedir>
  <policy context="default">
    <allow send_destination="*" eavesdrop="true"/>
    <allow eavesdrop="true"/>
    <allow own="*"/>
  </policy>
</busconfig>
EOF
exec 4< <(QT_QPA_PLATFORM=offscreen QT_PLUGIN_PATH="$KIO_EXTRAS/lib/qt-6/plugins" \
    dbus-daemon --config-file="$RUNTIME/kio-bus.conf" --nofork --print-address=1 \
    2>"$RUNTIME/kio-bus.log")
read -r kio_address <&4
XDG_CONFIG_HOME="$RUNTIME/xdg/config" XDG_CACHE_HOME="$RUNTIME/xdg/cache" \
    XDG_DATA_HOME="$RUNTIME/xdg/data" DBUS_SESSION_BUS_ADDRESS="$kio_address" \
    SKWIR_TEST_KMTP_PHONE=1 SKWIR_KMTP_WORKER="$KMTP_WORKER" \
    "$TEST_BINARY" --exact kmtpd_opens_a_phone_kde_holds_through_skwir_s_helper --nocapture \
    || status=$?
exit "$status"
