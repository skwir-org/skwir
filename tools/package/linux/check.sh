#!/bin/sh
# What an installed Linux package must hold (ADR-040), checked on the
# system it was installed on: every program shipped
# (tools/package/programs) where bibli finds it, each finding its
# libraries, and the libraries bibli opens at run time, which no
# packaging tool sees (the Vulkan loader, fontconfig).
#
#     tools/package/linux/check.sh
set -eu
programs="$(dirname "$0")/../programs"
failed=0
while read -r program; do
    found=""
    for dir in /usr/bin /usr/lib/bibli /usr/libexec/bibli; do
        if [ -x "$dir/$program" ]; then
            found="$dir/$program"
            break
        fi
    done
    if [ -z "$found" ]; then
        echo "missing: $program"
        failed=1
    elif ldd "$found" | grep -q "not found"; then
        echo "$found lacks:"
        ldd "$found" | grep "not found"
        failed=1
    else
        echo "ok: $found"
    fi
done < "$programs"
for library in libvulkan.so.1 libfontconfig.so.1; do
    if ldconfig -p | grep -q "$library"; then
        echo "ok: $library"
    else
        echo "missing: $library, which bibli opens at run time"
        failed=1
    fi
done
exit "$failed"
