#!/bin/sh
# Run a command against a Secret Service of its own: a private D-Bus session
# with a gnome-keyring whose files live in a temporary directory, never the
# user's keyring. Needs dbus-run-session, dbus-send and gnome-keyring-daemon
# on PATH.
#
#   tools/test/private-keyring.sh cargo nextest run -p bibli-platform \
#       --run-ignored only secrets
#
# The bus activates nothing (its configuration names no service directory),
# so no other keyring (KeePassXC, KWallet) can be started on it, and the
# command runs only once this keyring owns org.freedesktop.secrets. The
# command sees BIBLI_PRIVATE_KEYRING=1, which keyring tests require.
set -eu
# Short on purpose: the keyring's control socket lives under it, and a Unix
# socket's path is at most 108 bytes.
base="$HOME/.cache/bibli-tests"
mkdir -p "$base"
dir=$(mktemp -d "$base/k.XXXXXX")
trap 'rm -rf "$dir"' EXIT
mkdir -p "$dir/data" "$dir/run"
chmod 700 "$dir/run"
cat > "$dir/bus.conf" <<EOF
<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <type>session</type>
  <listen>unix:dir=$dir/run</listen>
  <auth>EXTERNAL</auth>
  <policy context="default">
    <allow send_destination="*" eavesdrop="true"/>
    <allow eavesdrop="true"/>
    <allow own="*"/>
  </policy>
</busconfig>
EOF
export XDG_DATA_HOME="$dir/data" XDG_RUNTIME_DIR="$dir/run"
dbus-run-session --config-file="$dir/bus.conf" -- sh -c '
    printf test | gnome-keyring-daemon --unlock --components=secrets >/dev/null
    # The daemon claims the name a moment after --unlock returns.
    tries=0
    until dbus-send --session --print-reply --dest=org.freedesktop.DBus \
        /org/freedesktop/DBus org.freedesktop.DBus.GetNameOwner \
        string:org.freedesktop.secrets >/dev/null 2>&1; do
        tries=$((tries + 1))
        if [ "$tries" -ge 50 ]; then
            echo "private-keyring: the private keyring did not start; refusing to run" >&2
            exit 1
        fi
        sleep 0.1
    done
    BIBLI_PRIVATE_KEYRING=1 "$@"
' sh "$@"
