#!/bin/sh
# nextest setup script: every test process gets XDG data, config, cache and state
# homes of its own under ~/.cache/skwir-tests, so a test that trashes a
# fixture, saves settings or writes a thumbnail never touches the user's
# real Trash, config or cache (AGENTS.md, "Tests that need a real filesystem
# feature"). The directory is on the home filesystem, not tmpfs, so trash
# and xattrs behave as they do for the user. The runtime folder too, where
# a control channel is opened (ADR-043): never the session's.
set -eu
root="${HOME}/.cache/skwir-tests/xdg"
mkdir -p "$root/data" "$root/config" "$root/cache" "$root/state" "$root/runtime"
chmod 700 "$root/runtime"
# Nor the user's session bus, where a test window would register a player
# (MPRIS) or read the desktop's portal: an address nothing listens at, as
# a session without a bus. A bus a harness made for its tests (a keyring's,
# GVfs's) is kept.
bus="${DBUS_SESSION_BUS_ADDRESS:-}"
if [ -n "${XDG_RUNTIME_DIR:-}" ] && [ "$bus" = "unix:path=$XDG_RUNTIME_DIR/bus" ]; then
    bus="unix:path=$root/runtime/no-session-bus"
fi
{
    [ -z "$bus" ] || echo "DBUS_SESSION_BUS_ADDRESS=$bus"
    echo "XDG_DATA_HOME=$root/data"
    echo "XDG_CONFIG_HOME=$root/config"
    echo "XDG_CACHE_HOME=$root/cache"
    echo "XDG_STATE_HOME=$root/state"
    echo "XDG_RUNTIME_DIR=$root/runtime"
} >> "$NEXTEST_ENV"
