#!/usr/bin/env bash
# Run a command against a Secret Service of its own that asks to be
# unlocked through its own prompt: a private D-Bus session whose services
# are GNOME Keyring's and GCR's prompter (gcr-prompter, GTK), drawing on a
# display of its own (Xvfb); the keyring made with the password `test`,
# its files in a temporary folder. Never the user's keyring, bus or
# display. The command sees SKWIR_LOCKED_KEYRING=1 and XDOTOOL (to answer
# the prompt as a user would):
#   tools/test/locked-keyring.sh cargo nextest run -p skwir-platform \
#       -p skwir-plugin-remote --run-ignored only -E 'test(locked_keyring)' -j 1
set -euo pipefail

tools=$(nix build --no-link --print-out-paths nixpkgs#gnome-keyring nixpkgs#gcr_3 \
    nixpkgs#xorg.xorgserver nixpkgs#xdotool nixpkgs#dbus)
out() { grep -E "/[a-z0-9]{32}-$1-[0-9.]+$" <<< "$tools" | head -1; }
keyring=$(out gnome-keyring)
gcr=$(out gcr)
xvfb=$(out xorg-server)
xdotool=$(out xdotool)
dbus=$(out dbus)

base="$HOME/.cache/skwir-tests"
mkdir -p "$base"
dir=$(mktemp -d "$base/l.XXXXXX")
mkdir -p "$dir/data" "$dir/run" "$dir/config"
chmod 700 "$dir/run"

# A display of its own, its number chosen by Xvfb.
exec 3> "$dir/display"
"$xvfb/bin/Xvfb" -displayfd 3 -screen 0 1024x768x24 -nolisten tcp > "$dir/xvfb.log" 2>&1 &
xvfb_pid=$!
for _ in $(seq 100); do
    [ -s "$dir/display" ] && break
    sleep 0.05
done
display=":$(head -1 "$dir/display")"
cleanup() {
    kill "$xvfb_pid" 2>/dev/null || true
    rm -rf "$dir"
}
trap cleanup EXIT

cat > "$dir/bus.conf" <<EOF
<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <type>session</type>
  <listen>unix:dir=$dir/run</listen>
  <auth>EXTERNAL</auth>
  <servicedir>$gcr/share/dbus-1/services</servicedir>
  <policy context="default">
    <allow send_destination="*" eavesdrop="true"/>
    <allow eavesdrop="true"/>
    <allow own="*"/>
  </policy>
</busconfig>
EOF
# What the bus starts (the prompter) draws on the test's display.
export DISPLAY="$display" GDK_BACKEND=x11 XDG_DATA_HOME="$dir/data" \
    XDG_CONFIG_HOME="$dir/config" XDG_RUNTIME_DIR="$dir/run" XDOTOOL="$xdotool/bin/xdotool"
unset WAYLAND_DISPLAY
"$dbus/bin/dbus-run-session" --config-file="$dir/bus.conf" -- sh -c '
    printf test | "$1/bin/gnome-keyring-daemon" --unlock --components=secrets >/dev/null
    tries=0
    until "$2/bin/dbus-send" --session --print-reply --dest=org.freedesktop.DBus \
        /org/freedesktop/DBus org.freedesktop.DBus.GetNameOwner \
        string:org.freedesktop.secrets >/dev/null 2>&1; do
        tries=$((tries + 1))
        if [ "$tries" -ge 50 ]; then
            echo "locked-keyring: the keyring did not start; refusing to run" >&2
            exit 1
        fi
        sleep 0.1
    done
    shift 2
    SKWIR_LOCKED_KEYRING=1 "$@"
' sh "$keyring" "$dbus" "$@"
