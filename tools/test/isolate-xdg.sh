#!/bin/sh
# nextest setup script: every test process gets XDG data, config and cache
# homes of its own under ~/.cache/bibli-tests, so a test that trashes a
# fixture, saves settings or writes a thumbnail never touches the user's
# real Trash, config or cache (AGENTS.md, "Tests that need a real filesystem
# feature"). The directory is on the home filesystem, not tmpfs, so trash
# and xattrs behave as they do for the user. The runtime folder too, where
# a control channel is opened (ADR-043): never the session's.
set -eu
root="${HOME}/.cache/bibli-tests/xdg"
mkdir -p "$root/data" "$root/config" "$root/cache" "$root/runtime"
chmod 700 "$root/runtime"
{
    echo "XDG_DATA_HOME=$root/data"
    echo "XDG_CONFIG_HOME=$root/config"
    echo "XDG_CACHE_HOME=$root/cache"
    echo "XDG_RUNTIME_DIR=$root/runtime"
} >> "$NEXTEST_ENV"
