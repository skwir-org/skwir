#!/bin/sh
# Every default member's tests under Wine (ADR-035, ADR-053): what `just
# test-windows` runs given no arguments, and `just release-check` with it,
# in the environment test-windows.sh makes (run alone, this asks it to).
#
# Built once, as `cargo test` builds the default members: each crate with
# the features the workspace unifies, as `just test-all` has them. A
# `-p <crate>` build per crate resolves each crate's features alone: the
# stack is built again for each set, which filled target/cross (154 GB by
# 2026-11-25), and skwir-platform alone has no `http`, whose tests then
# never ran.
#
# Then run one crate at a time from that build (`--binaries-metadata`, no
# cargo asked again): nextest lists every test program before it runs one,
# each under a Wine of its own, and all of them at once take more memory
# than a busy machine holds (the listing of skwir's tests was killed so on
# 2026-11-24). A crate's filter, `package(=…)`, leaves the other crates'
# programs unlisted.
#
# Fails if any crate's tests fail, naming each; names, in its last lines,
# the tests that passed only when run again (FLAKY), which fail nothing.
set -eu
if [ -z "${SKWIR_WINE_HARNESS:-}" ]; then
    exec tools/cross/test-windows.sh
fi
crates="$(cargo metadata --no-deps --format-version 1 | python3 -c '
import json, sys
m = json.load(sys.stdin)
members = set(m["workspace_default_members"])
print(" ".join(sorted(p["name"] for p in m["packages"] if p["id"] in members)))
')"
# What this run builds and says, apart from any other run's (agents, two
# terminals) sharing the target folder.
work="$(mktemp -d "$CARGO_TARGET_DIR/each.XXXXXX")"
work="$(cd "$work" && pwd)"
trap 'rm -rf "${work:?}"' EXIT
echo "== building every default member's tests"
SKWIR_NEXTEST_BINARIES="$work/binaries.json" CARGO="$(pwd)/tools/cross/zig-nextest" \
    cargo-zigbuild test --target x86_64-pc-windows-gnu
cargo metadata --format-version 1 --all-features --no-deps >"$work/metadata.json"
# A test that fails, then passes when run again, is told FLAKY and fails
# nothing: Wine now and then crashes as a test program exits, after its
# tests passed, and its change notices come late when many tests share
# it. One that fails twice fails the run.
export NEXTEST_RETRIES="${NEXTEST_RETRIES:-1}"
if [ -t 1 ]; then color=always; else color=never; fi
failed=""
for crate in $crates; do
    echo "== $crate"
    rm -f "$work/status"
    {
        cargo-nextest nextest run --binaries-metadata "$work/binaries.json" \
            --cargo-metadata "$work/metadata.json" -E "package(=$crate)" \
            --no-fail-fast --color "$color" 2>&1 || echo "$?" >"$work/status"
    } | tee "$work/run.log"
    status="$(cat "$work/status" 2>/dev/null || echo 0)"
    # 4: nextest found no test to run (a crate without tests).
    if [ "$status" -ne 0 ] && [ "$status" -ne 4 ]; then
        failed="$failed $crate"
    fi
    # Its final lines name each test that passed only on its retry.
    sed 's/\x1b\[[0-9;]*m//g' "$work/run.log" | grep -E '^ *FLAKY ' |
        sed 's/^ *//' | sort -u >>"$work/flaky" || true
done
if [ -s "$work/flaky" ]; then
    echo "Passed under Wine only when run again (FLAKY):"
    sed 's/^/  /' "$work/flaky"
fi
if [ -n "$failed" ]; then
    echo "Failed under Wine:$failed" >&2
    exit 1
fi
echo "Every crate's tests passed under Wine."
