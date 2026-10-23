#!/usr/bin/env bash
# Run a command against KeePassXC's Secret Service of its own: a private
# D-Bus session (activating nothing), KeePassXC (nixpkgs) on a display of
# its own with a database made here from KeePass XML, its root group
# exposed (`FDO_SECRETS_EXPOSED_GROUP`), its integration on, asking no
# confirmation; HOME and every XDG folder temporary. Never the user's
# KeePassXC, its files, bus or display. The command sees
# SKWIR_PRIVATE_KEYRING=1, which the keyring tests require:
#   tools/test/keepassxc-keyring.sh cargo nextest run -p skwir-platform \
#       -p skwir-plugin-remote --run-ignored only -E 'test(secret) | test(password)'
set -euo pipefail

tools=$(nix build --no-link --print-out-paths nixpkgs#keepassxc nixpkgs#xorg.xorgserver \
    nixpkgs#dbus)
out() { grep -E "/[a-z0-9]{32}-$1-[0-9.]+$" <<< "$tools" | head -1; }
keepassxc=$(out keepassxc)
xvfb=$(out xorg-server)
dbus=$(out dbus)

base="$HOME/.cache/skwir-tests"
mkdir -p "$base"
dir=$(mktemp -d "$base/x.XXXXXX")
mkdir -p "$dir/home" "$dir/config" "$dir/cache" "$dir/data" "$dir/run"
chmod 700 "$dir/run"

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
export HOME="$dir/home" XDG_CONFIG_HOME="$dir/config" XDG_CACHE_HOME="$dir/cache" \
    XDG_DATA_HOME="$dir/data" XDG_RUNTIME_DIR="$dir/run" DISPLAY="$display" \
    QT_QPA_PLATFORM=xcb
unset WAYLAND_DISPLAY
"$dbus/bin/dbus-run-session" --config-file="$dir/bus.conf" -- sh -c '
    printf test | "$1/bin/keepassxc" --config "$2/keepassxc.ini" --pw-stdin "$2/db.kdbx" \
        > "$2/keepassxc.log" 2>&1 &
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
    shift 3
    SKWIR_PRIVATE_KEYRING=1 "$@"
' sh "$keepassxc" "$dir" "$dbus" "$@"
