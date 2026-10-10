#!/bin/sh
# What installed Linux packages must hold (ADR-040, ADR-041), checked on
# the system they were installed on. Every program skwir always has
# (tools/package/programs, but those tools/package/optional lists) is
# where skwir finds it and works: skwir finds its libraries, each helper
# answers `--probe` (the PDFium helper finding the library the package
# carries). An optional one is absent, or there and working; installed
# `minimal` (without what the packages only recommend), it may be there
# with its libraries absent, which its probe says, as skwir's settings
# do. skwir's licence and PDFium's notices are installed, and the
# libraries skwir opens at run time are there.
#
#     tools/package/linux/check.sh [minimal]
set -eu
here="$(cd "$(dirname "$0")/.." && pwd)"
minimal="${1:-}"
failed=0
optional() {
    grep -qx "$1" "$here/optional"
}
said="$(mktemp)"
while read -r program; do
    found=""
    for dir in /usr/bin /usr/lib/skwir /usr/libexec/skwir; do
        if [ -x "$dir/$program" ]; then
            found="$dir/$program"
            break
        fi
    done
    if [ -z "$found" ]; then
        if optional "$program"; then
            echo "absent, optional: $program"
        else
            echo "missing: $program"
            failed=1
        fi
    elif [ "$program" = skwir ]; then
        if ldd "$found" | grep -q "not found"; then
            echo "$found lacks:"
            ldd "$found" | grep "not found"
            failed=1
        else
            echo "ok: $found"
        fi
    elif "$found" --probe >/dev/null 2>"$said"; then
        echo "works: $found"
    elif [ "$minimal" = minimal ] && optional "$program"; then
        echo "not working without what is only recommended: $found ($(head -c 120 "$said" | head -1))"
    else
        echo "does not work: $found ($(head -c 200 "$said" | head -1))"
        failed=1
    fi
done < "$here/programs"
rm -f "$said"
# skwir's licence, PDFium's notices (ADR-041, ADR-042) and those of the
# crates its programs link: Debian's in its copyright file and the
# package's documents, the others' in the licences' folder.
if [ -f /usr/share/doc/skwir/copyright ] && [ -f /usr/share/doc/skwir/pdfium/LICENSE ] &&
    [ -s /usr/share/doc/skwir/THIRD-PARTY-NOTICES ]; then
    echo "ok: the licences, in /usr/share/doc/skwir"
elif [ -f /usr/share/licenses/skwir/LICENSE ] && [ -f /usr/share/licenses/skwir/pdfium/LICENSE ] &&
    [ -s /usr/share/licenses/skwir/THIRD-PARTY-NOTICES ]; then
    echo "ok: the licences, in /usr/share/licenses/skwir"
else
    echo "missing: skwir's licence, PDFium's notices or the crates' notices"
    failed=1
fi
for library in libvulkan.so.1 libfontconfig.so.1; do
    if ldconfig -p | grep -q "$library"; then
        echo "ok: $library"
    else
        echo "missing: $library, which skwir opens at run time"
        failed=1
    fi
done
exit "$failed"
