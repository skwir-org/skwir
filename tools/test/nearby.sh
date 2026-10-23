#!/usr/bin/env bash
# The Nearby place (ADR-084) against servers announced on a network of the
# test's own, for `remote_folders`'s nearby_lists_what_avahi_and_wsdd_find:
#   tools/test/nearby.sh <remote_folders test binary>
# In user, network, mount and pid namespaces: a veth pair whose other end
# is a namespace standing for a Windows PC (wsdd announcing it), a system
# bus of the test's own with Avahi on it publishing an SMB and a WebDAV
# server, and wsdd on the PATH for Skwir to start in discovery mode (no
# system wsdd answers here). Never the user's network, bus or Avahi: what
# is sent goes between the two namespaces, and everything ends with them.
set -euo pipefail

if [ "${1:-}" != --inside ]; then
    tested=$(realpath "$1")
    tools=$(nix build --no-link --print-out-paths nixpkgs#avahi nixpkgs#dbus nixpkgs#iproute2 \
        nixpkgs#util-linux nixpkgs#wsdd nixpkgs#coreutils)
    out() { grep -E "/[a-z0-9]{32}-$1-[0-9.]+(-bin)?$" <<< "$tools" | head -1; }
    path=""
    for p in util-linux coreutils iproute2 dbus avahi wsdd; do
        path="$path$(out "$p")/bin:"
    done
    scratch=$(mktemp -d "${TMPDIR:-/tmp}/skwir-nearby-XXXXXX")
    status=0
    env PATH="${path}$PATH" AVAHI="$(out avahi)" RUNTIME="$scratch" TEST_BINARY="$tested" \
        timeout 300 unshare -r -n -m -p --fork --propagation private --mount-proc "$0" --inside \
        || status=$?
    rmdir "$scratch"
    exit "$status"
fi

# Inside. RUNTIME, TEST_BINARY and AVAHI come from the outer half.
mount -t tmpfs tmpfs "$RUNTIME"
# Avahi's runtime folder is its user's: here root, the one user mapped.
printf 'root:x:0:0::/:/bin/sh\navahi:x:0:0::/:/bin/sh\n' > "$RUNTIME/passwd"
printf 'root:x:0:\navahi:x:0:\n' > "$RUNTIME/group"
mount --bind "$RUNTIME/passwd" /etc/passwd
mount --bind "$RUNTIME/group" /etc/group
mount -t tmpfs tmpfs /run
mkdir -p /run/dbus

# The network: this side, and a PC's on the other end of a veth pair.
ip link set lo up
ip link add va type veth peer name vb
unshare -n sleep infinity &
pc=$!
for _ in $(seq 50); do
    [ "$(readlink "/proc/$pc/ns/net")" != "$(readlink /proc/self/ns/net)" ] && break
    sleep 0.05
done
ip link set vb netns "$pc"
nsenter -t "$pc" -n ip link set lo up
nsenter -t "$pc" -n ip addr add 10.89.0.2/24 dev vb
nsenter -t "$pc" -n ip link set vb multicast on up
ip addr add 10.89.0.1/24 dev va
ip link set va multicast on up

# A system bus of its own, Avahi on it.
cat > "$RUNTIME/bus.conf" <<EOF
<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <type>system</type>
  <listen>unix:path=/run/dbus/system_bus_socket</listen>
  <policy context="default">
    <allow send_destination="*" eavesdrop="true"/>
    <allow eavesdrop="true"/>
    <allow own="*"/>
  </policy>
</busconfig>
EOF
dbus-daemon --config-file="$RUNTIME/bus.conf" --nofork --nopidfile > "$RUNTIME/bus.log" 2>&1 &
for _ in $(seq 100); do
    [ -S /run/dbus/system_bus_socket ] && break
    sleep 0.05
done
export DBUS_SYSTEM_BUS_ADDRESS=unix:path=/run/dbus/system_bus_socket
cat > "$RUNTIME/avahi.conf" <<EOF
[server]
host-name=skwir-nas
use-ipv4=yes
use-ipv6=no
allow-interfaces=va
enable-dbus=yes
[publish]
publish-hinfo=no
publish-workstation=no
EOF
"$AVAHI/sbin/avahi-daemon" --file="$RUNTIME/avahi.conf" --no-drop-root --no-rlimits \
    --no-proc-title > "$RUNTIME/avahi.log" 2>&1 &
for _ in $(seq 100); do
    grep -q "Server startup complete" "$RUNTIME/avahi.log" && break
    sleep 0.05
done
avahi-publish -s "Test NAS" _smb._tcp 445 > "$RUNTIME/publish-smb.log" 2>&1 &
avahi-publish -s "Test Files" _webdav._tcp 8080 "path=/dav" > "$RUNTIME/publish-dav.log" 2>&1 &

# The PC: wsdd announcing it, by a UUID of its own (wsdd takes the
# machine's otherwise, which the one discovering would take for its own).
nsenter -t "$pc" -n wsdd -i vb -n WINPC -w TESTGROUP -U 6a3e4c1e-0000-4000-8000-000000000001 \
    > "$RUNTIME/wsdd-pc.log" 2>&1 &

mkdir -m 700 "$RUNTIME/rt"
mkdir -p "$RUNTIME/home" "$RUNTIME/xdg/config" "$RUNTIME/xdg/cache" "$RUNTIME/xdg/data" \
    "$RUNTIME/xdg/state"
status=0
HOME="$RUNTIME/home" XDG_RUNTIME_DIR="$RUNTIME/rt" XDG_CONFIG_HOME="$RUNTIME/xdg/config" \
    XDG_CACHE_HOME="$RUNTIME/xdg/cache" XDG_DATA_HOME="$RUNTIME/xdg/data" \
    XDG_STATE_HOME="$RUNTIME/xdg/state" SKWIR_TEST_NEARBY=1 \
    "$TEST_BINARY" --exact nearby_lists_what_avahi_and_wsdd_find --nocapture || status=$?
if [ "$status" != 0 ]; then
    tail -5 "$RUNTIME/avahi.log" "$RUNTIME/wsdd-pc.log" >&2 || true
fi
exit "$status"
