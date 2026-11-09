# Sourced from the repository's root before a `dist` build: on x86-64 Linux
# skwir then links through tools/build/link-ordered (ADR-128), which hands
# everything else to the linker that was going to link it. Elsewhere it
# does nothing.
case "$(uname -s)-$(uname -m)" in
Linux-x86_64)
    if [ -z "${SKWIR_REAL_LINKER:-}" ]; then
        SKWIR_REAL_LINKER=${CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER:-cc}
        CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=$PWD/tools/build/link-ordered
        export SKWIR_REAL_LINKER CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER
    fi
    ;;
esac
