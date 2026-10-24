#!/usr/bin/env bash
# GVfs's NFS and AFP mounts against servers on this machine (ADR-078, 15),
# for `remote_folders`'s gvfs_nfs_and_afp_take_by_gio_what_it_does_better:
#   tools/test/gvfs-nfs-afp.sh <remote_folders test binary>
# Their servers run as root (unfs3 with rpcbind, netatalk), so everything
# runs in user, network, mount and pid namespaces of its own: root mapped
# to this user, subordinate ids beside it (newuidmap; rpcbind drops its
# groups), loopback alone, fresh tmpfs on /run and /var/lib, a copy of
# /etc, a session bus and a GVfs of its own; every process ends with it.
# Never the session's bus, GVfs or files. Skipped, saying why, where user
# namespaces with subordinate ids cannot be made.
set -euo pipefail

# This user's first block of subordinate ids in `file`, as `1:start:count`
# (the ids after root, which is this user).
block() {
    local name start count
    while IFS=: read -r name start count; do
        if [ "$name" = "$(id -un)" ] || [ "$name" = "$(id -u)" ]; then
            echo "1:$start:$((count - 1))"
            return
        fi
    done < "$1"
}

if [ "${1:-}" != --inside ]; then
    tested=$(realpath "$1")
    users=$(block /etc/subuid 2>/dev/null || true)
    groups=$(block /etc/subgid 2>/dev/null || true)
    namespaces=(unshare -U --map-user=0 --map-group=0 "--map-users=$users" "--map-groups=$groups")
    if [ -z "$users" ] || [ -z "$groups" ] || ! "${namespaces[@]}" true 2>/dev/null; then
        echo "skipped: no user namespace with subordinate ids (newuidmap, /etc/subuid)"
        exit 0
    fi
    tools=$(nix build --no-link --print-out-paths 'nixpkgs#glib^bin' nixpkgs#dbus \
        nixpkgs#unfs3 nixpkgs#rpcbind nixpkgs#netatalk nixpkgs#iproute2 nixpkgs#coreutils \
        nixpkgs#util-linux nixpkgs#gnused nixpkgs#gnugrep nixpkgs#findutils)
    # GVfs's AFP backend dies now and then against this server, which
    # answers at once: it frees a request whose reply came before its last
    # write called back, and changes its request hash from two threads.
    # Built with the fix, until upstream has it
    # (gvfs-afp-request-lifetime.patch).
    fix=$(realpath "$(dirname "$0")/gvfs-afp-request-lifetime.patch")
    gvfs=$(nix build --impure --no-link --print-out-paths --expr "
        let pkgs = (builtins.getFlake \"nixpkgs\").legacyPackages.\${builtins.currentSystem};
        in pkgs.gvfs.overrideAttrs (o: { patches = (o.patches or [ ]) ++ [ (/. + \"$fix\") ]; })")
    out() { grep -E "/[a-z0-9]{32}-$1-[0-9.]+(p[0-9]+)?(-bin)?$" <<< "$tools" | head -1; }
    path=""
    for p in glib coreutils util-linux iproute2 gnused gnugrep findutils dbus unfs3 rpcbind netatalk; do
        path="$path$(out "$p")/bin:"
    done
    base="${HOME}/.cache/skwir-tests"
    mkdir -p "$base"
    scratch=$(mktemp -d "$base/nfs-afp-XXXXXX")
    status=0
    env PATH="${path}$(out iproute2)/sbin:$(out netatalk)/sbin:$PATH" GVFS="$gvfs" \
        NETATALK="$(out netatalk)" FIXTURE="$scratch" TEST_BINARY="$tested" \
        timeout 300 "${namespaces[@]}" -n -m -p --fork --propagation private "$0" --inside \
        || status=$?
    rm -rf "$scratch"
    exit "$status"
fi

# Inside: the servers, the bus, GVfs, then the test. FIXTURE, TEST_BINARY,
# GVFS come from the outer half's environment.
ip link set lo up
mkdir -p "$FIXTURE/etc" "$FIXTURE/export" "$FIXTURE/home"
cp -a --no-preserve=ownership /etc/. "$FIXTURE/etc/" 2>/dev/null || true
printf '%s\n' \
    'udp tpi_clts v inet udp - -' 'tcp tpi_cots_ord v inet tcp - -' \
    'udp6 tpi_clts v inet6 udp - -' 'tcp6 tpi_cots_ord v inet6 tcp - -' \
    'local tpi_cots_ord - loopback - - -' 'unix tpi_cots_ord - loopback - - -' \
    > "$FIXTURE/etc/netconfig"
cp -L --remove-destination /etc/passwd "$FIXTURE/etc/passwd"
echo "rpc:x:0:0:rpc:/run/rpcbind:/bin/false" >> "$FIXTURE/etc/passwd"
mount --bind "$FIXTURE/etc" /etc
mount -t tmpfs tmpfs /run
mount -t tmpfs tmpfs /var/lib
mkdir -p /run/rpcbind /run/lock /run/afp/share /run/afp/cnid /var/lib/netatalk /run/rt
chmod 1777 /run/afp/share /run/afp/cnid /var/lib/netatalk
chmod 700 /run/rt

rpcbind -f > "$FIXTURE/rpcbind.log" 2>&1 &
sleep 0.5
echo "$FIXTURE/export 127.0.0.1(rw,no_root_squash)" > "$FIXTURE/exports"
unfsd -d -t -e "$FIXTURE/exports" > "$FIXTURE/unfsd.log" 2>&1 &
# AFP's guest is nobody, who reaches what is in /run alone.
cat > /run/afp/afp.conf <<EOF
[Global]
uam list = uams_guest.so
guest account = nobody
log file = /run/afp/afpd.log
afp listen = 127.0.0.1
vol dbpath = /run/afp/cnid/\$v/
zeroconf = no

[share]
path = /run/afp/share
EOF
netatalk -d -F /run/afp/afp.conf > /run/afp/netatalk.log 2>&1 &
sleep 2

export HOME="$FIXTURE/home" XDG_RUNTIME_DIR=/run/rt
export XDG_DATA_DIRS="$GVFS/share" GIO_EXTRA_MODULES="$GVFS/lib/gio/modules"
export GIO_USE_VOLUME_MONITOR=unix
cat > /run/bus.conf <<EOF
<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <type>session</type>
  <listen>unix:dir=/run/rt</listen>
  <servicedir>$GVFS/share/dbus-1/services</servicedir>
  <policy context="default">
    <allow send_destination="*" eavesdrop="true"/>
    <allow eavesdrop="true"/>
    <allow own="*"/>
  </policy>
</busconfig>
EOF
exec 3< <(dbus-daemon --config-file=/run/bus.conf --nofork --print-address=1 2>/run/bus.log)
read -r address <&3
export DBUS_SESSION_BUS_ADDRESS="$address"
gio mount "nfs://127.0.0.1$FIXTURE/export"
gio mount -a "afp://127.0.0.1/share"
nfs=$(ls -d /run/rt/gvfs/nfs*)
afp=$(ls -d /run/rt/gvfs/afp-volume*)

mkdir -p /run/xdg/config /run/xdg/cache /run/xdg/data
XDG_CONFIG_HOME=/run/xdg/config XDG_CACHE_HOME=/run/xdg/cache XDG_DATA_HOME=/run/xdg/data \
    SKWIR_TEST_GVFS_NFS="$nfs" SKWIR_TEST_GVFS_AFP="$afp" \
    "$TEST_BINARY" --exact gvfs_nfs_and_afp_take_by_gio_what_it_does_better --nocapture
