#!/usr/bin/env bash
# Connect at Login under your own systemd user manager (ADR-087): the one
# check Skwir's tests never make, since they never ask the user's manager.
# Run it yourself, in your session, once:
#
#   nix develop --command tools/test/login-user-systemd.sh
#
# It builds Skwir's tests, then runs one of them
# (`remote_login::connect_at_login_under_the_user_s_own_systemd`), which,
# through Skwir's own code, installs one unit in your user unit folder for
# a remote of an rclone configuration of the test's own, served by an
# rclone of the test's own on 127.0.0.1. Your manager starts it and it
# mounts. The test kills its program and your manager mounts it again.
# It turns the unit off (the mount stays until let go of) and on and off
# again, then checks that nothing is left: no unit, no link, no mount.
# About a minute, half of it waiting for systemd's restart delay.
#
# Nothing of yours is read or changed beyond that one unit, named
# `skwir-remote-rclone-rclone:--skwir\x2dlogin\x2dcheck\x2d<pid>-.service`,
# which is removed however the test ends. Your rclone configuration, ssh
# and agent are never touched. Its mount and the test's files are under
# ~/.cache/skwir-tests.
set -euo pipefail
cd "$(dirname "$0")/../.."

units="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user"
runtime="${XDG_RUNTIME_DIR:?no runtime folder: run this in your session}"
state=$(XDG_RUNTIME_DIR="$runtime" systemctl --user is-system-running 2>&1 || true)
case "$state" in
    running | degraded) echo "your user manager: $state" ;;
    *)
        echo "no user manager answers ($state): nothing to check" >&2
        exit 1
        ;;
esac

# rclone as the remote checks have it: the flake's nixpkgs, as remote-check.
repo=$(realpath "$(dirname "$0")/../..")
rclone=$(nix build --no-link --print-out-paths --inputs-from "git+file://$repo" nixpkgs#rclone)
export PATH="$rclone/bin:$PATH"

# What a run that died before its own clean-up could leave: said, never
# removed here, so nothing else of yours is touched by a pattern.
leftovers() {
    find "$units" -maxdepth 2 -name '*skwir\\x2dlogin\\x2dcheck*' 2>/dev/null || true
}
before=$(leftovers)
if [ -n "$before" ]; then
    echo "already in $units, from an earlier run:" >&2
    echo "$before" >&2
fi

if SKWIR_TEST_USER_UNITS="$units" SKWIR_TEST_USER_RUNTIME="$runtime" \
    cargo nextest run -p skwir --run-ignored only --no-capture \
    -E 'binary(remote_login) & test(connect_at_login_under_the_user_s_own_systemd)'; then
    result=0
else
    result=$?
fi

left=$(leftovers)
if [ -n "$left" ]; then
    echo "left in $units (take each out with systemctl --user disable <name>, then rm, then systemctl --user daemon-reload):" >&2
    echo "$left" >&2
    result=1
else
    echo "nothing left in $units"
fi
if grep -q 'skwir-login-check' /proc/self/mountinfo; then
    echo "a test mount is left: fusermount3 -uz it (grep skwir-login-check /proc/self/mountinfo)" >&2
    result=1
fi
if [ "$result" -eq 0 ]; then
    echo "Connect at Login works under your own systemd."
else
    echo "the check failed: its output above says where; nothing of yours but the unit named was touched" >&2
fi
exit "$result"
