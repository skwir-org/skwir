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
# Nor the user's tools' own configurations (ADR-086): rclone's remotes, the
# ssh agent and its keys, AWS's credentials. A test wanting one makes its
# own (a configuration file, an agent of its own) and names it. The user
# manager of systemd is out of reach already: `systemctl --user` looks for
# it in the runtime folder, the tests' own.
mkdir -p "$root/config/rclone" "$root/config/aws"
{
    [ -z "$bus" ] || echo "DBUS_SESSION_BUS_ADDRESS=$bus"
    echo "XDG_DATA_HOME=$root/data"
    echo "XDG_CONFIG_HOME=$root/config"
    echo "XDG_CACHE_HOME=$root/cache"
    echo "XDG_STATE_HOME=$root/state"
    echo "XDG_RUNTIME_DIR=$root/runtime"
    echo "RCLONE_CONFIG=$root/config/rclone/rclone.conf"
    echo "SSH_AUTH_SOCK=$root/runtime/no-ssh-agent"
    echo "AWS_CONFIG_FILE=$root/config/aws/config"
    echo "AWS_SHARED_CREDENTIALS_FILE=$root/config/aws/credentials"
} >> "$NEXTEST_ENV"
