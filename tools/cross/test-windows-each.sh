#!/bin/sh
# Every default member's tests under Wine (ADR-035, ADR-053): what `just
# test-windows` runs given no arguments, and `just release-check` with it.
# One crate at a time through test-windows.sh: nextest lists every test
# program before it runs one, each under a Wine of its own, and all of
# them at once take more memory than a busy machine holds (the listing of
# skwir's tests was killed so on 2026-11-24). The build is made once, by
# the first crate's run, and kept for the others. Fails if any crate's
# tests fail, naming each.
set -eu
if ! command -v wine >/dev/null; then
    exec nix develop .#windows --command "$0" "$@"
fi
crates="$(cargo metadata --no-deps --format-version 1 | python3 -c '
import json, sys
m = json.load(sys.stdin)
members = set(m["workspace_default_members"])
print(" ".join(sorted(p["name"] for p in m["packages"] if p["id"] in members)))
')"
# A test that fails, then passes when run again, is told FLAKY in the log
# and fails nothing: Wine now and then crashes as a test program exits,
# after its tests passed, and its change notices come late when many
# tests share it. One that fails twice fails the run.
export NEXTEST_RETRIES="${NEXTEST_RETRIES:-1}"
failed=""
for crate in $crates; do
    echo "== $crate"
    status=0
    tools/cross/test-windows.sh -p "$crate" --no-fail-fast || status=$?
    # 4: nextest found no test to run (a crate without tests).
    if [ "$status" -ne 0 ] && [ "$status" -ne 4 ]; then
        failed="$failed $crate"
    fi
done
if [ -n "$failed" ]; then
    echo "Failed under Wine:$failed" >&2
    exit 1
fi
echo "Every crate's tests passed under Wine."
