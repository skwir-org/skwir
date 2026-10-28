#!/usr/bin/env bash
# Run a command against KeePassXC's Secret Service of its own: a private
# D-Bus session (activating nothing), KeePassXC (nixpkgs) on a display of
# its own with a database made here from KeePass XML, its root group
# exposed (`FDO_SECRETS_EXPOSED_GROUP`), its integration on, asking no
# confirmation; HOME, every XDG folder and its temporary folder temporary.
# Never the user's KeePassXC, its files, bus or display: KeePassXC keeps its
# single-instance lock and socket in the temporary folder, by user name, so
# with the user's own one running it would hand the database over to it;
# here it is told to be no single instance too, and the command runs only
# once its socket is seen in the private folder. Cargo keeps the user's
# CARGO_HOME, which a temporary HOME would empty. The command sees
# SKWIR_PRIVATE_KEYRING=1, which the keyring tests require:
#   tools/test/keepassxc-keyring.sh cargo nextest run -p skwir-platform \
#       -p skwir-plugin-remote --run-ignored only -E 'test(secret) | test(password)'
set -euo pipefail

# The flake's nixpkgs, as its lock pins it: never the registry's.
repo=$(realpath "$(dirname "$0")/../..")
tools=$(nix build --no-link --print-out-paths --inputs-from "git+file://$repo" nixpkgs#keepassxc nixpkgs#xorg.xorgserver \
    nixpkgs#dbus)
out() { grep -E "/[a-z0-9]{32}-$1-[0-9.]+$" <<< "$tools" | head -1; }
keepassxc=$(out keepassxc)
xvfb=$(out xorg-server)
dbus=$(out dbus)

base="$HOME/.cache/skwir-tests"
mkdir -p "$base"
dir=$(mktemp -d "$base/x.XXXXXX")
mkdir -p "$dir/home" "$dir/config" "$dir/cache" "$dir/data" "$dir/run" "$dir/tmp"
chmod 700 "$dir/run"

xvfb_pid=
cleanup() {
    [ -z "$xvfb_pid" ] || kill "$xvfb_pid" 2>/dev/null || true
    rm -rf "$dir"
}
trap cleanup EXIT
# A display of its own (never :0, `own-display.sh`).
# shellcheck source=tools/test/own-display.sh
. "$(dirname "${BASH_SOURCE[0]}")/own-display.sh"
own_display "$xvfb/bin/Xvfb" "$dir/xvfb.log"

# The database: its root group exposed, by its UUID.
cat > "$dir/db.xml" <<'EOF'
<?xml version="1.0" encoding="utf-8" standalone="yes"?>
<KeePassFile>
  <Meta>
    <Generator>skwir test</Generator>
    <DatabaseName>skwir test</DatabaseName>
    <CustomData>
      <Item><Key>FDO_SECRETS_EXPOSED_GROUP</Key><Value>{6d2b6f6c-1a7e-4c2a-9b0e-5a1c2d3e4f50}</Value></Item>
    </CustomData>
  </Meta>
  <Root>
    <Group>
      <UUID>bStvbBp+TCqbDlocLT5PUA==</UUID>
      <Name>Root</Name>
    </Group>
  </Root>
</KeePassFile>
EOF
printf 'test\ntest\n' | "$keepassxc/bin/keepassxc-cli" import -q -p "$dir/db.xml" "$dir/db.kdbx"
cat > "$dir/keepassxc.ini" <<'EOF'
[General]
ConfigVersion=2
SingleInstance=false
[FdoSecrets]
Enabled=true
ShowNotification=false
ConfirmAccessItem=false
ConfirmDeleteItem=false
UnlockBeforeSearch=false
EOF
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
export CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}" RUSTUP_HOME="${RUSTUP_HOME:-$HOME/.rustup}"
export HOME="$dir/home" XDG_CONFIG_HOME="$dir/config" XDG_CACHE_HOME="$dir/cache" \
    XDG_DATA_HOME="$dir/data" XDG_RUNTIME_DIR="$dir/run" DISPLAY="$display" \
    QT_QPA_PLATFORM=xcb
unset WAYLAND_DISPLAY
"$dbus/bin/dbus-run-session" --config-file="$dir/bus.conf" -- sh -c '
    printf test | TMPDIR="$2/tmp" "$1/bin/keepassxc" --config "$2/keepassxc.ini" \
        --pw-stdin "$2/db.kdbx" > "$2/keepassxc.log" 2>&1 &
    # Stopped with the command (which is not the last word, so the shell
    # does not exec it): left behind its display, it spins and ignores
    # SIGTERM.
    trap "kill -9 $! 2>/dev/null" EXIT
    tries=0
    until "$3/bin/dbus-send" --session --print-reply --dest=org.freedesktop.DBus \
        /org/freedesktop/DBus org.freedesktop.DBus.GetNameOwner \
        string:org.freedesktop.secrets >/dev/null 2>&1; do
        tries=$((tries + 1))
        if [ "$tries" -ge 300 ]; then
            echo "keepassxc-keyring: KeePassXC did not serve; refusing to run" >&2
            cat "$2/keepassxc.log" >&2
            exit 1
        fi
        sleep 0.1
    done
    # Its own instance, not a hand-over to another: its socket is here.
    if ! ls "$2"/tmp/keepassxc*.socket >/dev/null 2>&1; then
        echo "keepassxc-keyring: no KeePassXC socket in the private folder; refusing to run" >&2
        exit 1
    fi
    shift 3
    status=0
    SKWIR_PRIVATE_KEYRING=1 "$@" || status=$?
    exit "$status"
' sh "$keepassxc" "$dir" "$dbus" "$@"
