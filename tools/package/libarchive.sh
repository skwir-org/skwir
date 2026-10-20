#!/bin/sh
# libarchive as Windows' and macOS's packages carry it (ADR-041: linked
# statically into skwir-archive-worker), built on Linux for <target> by
# the compilers those packages are built with: Zig for Windows, as
# cargo-zigbuild; for macOS, clang and LLVM's linker with Apple's SDK,
# through xmac's toolchain (`just macos-sdk`). Its sources are nixpkgs',
# patched as nixpkgs ships them and pinned by the flake's lock
# (SKWIR_LIBARCHIVE_SOURCES, in the `windows` and `macos` shells).
#
# What the system has, it gives: Windows its cryptography (CNG) and
# XmlLite, macOS its zlib, bzip2, iconv, Expat and CommonCrypto. The rest
# is built here, static: libarchive, xz's liblzma, zstd and lz4, and on
# Windows zlib and bzip2. It reads what Linux's libarchive reads, but LZO
# (GPL).
#
# Built once for a target, its sources and this recipe, in the build
# cache, where it is kept: prints that folder, which holds lib (the
# libraries), link (what the helper links, as cargo names them, read by
# crates/archive-worker/build.rs), target, and licenses (each library's
# notice and the versions, for the package to copy).
#
#     tools/package/libarchive.sh <x86_64-pc-windows-gnu|aarch64-apple-darwin>
set -eu
case "${1:-}" in
x86_64-pc-windows-gnu) shell=windows ;;
aarch64-apple-darwin) shell=macos ;;
*)
    echo "usage: $0 <x86_64-pc-windows-gnu|aarch64-apple-darwin>" >&2
    exit 2
    ;;
esac
target="$1"
root="$(cd "$(dirname "$0")/../.." && pwd)"
if [ -z "${SKWIR_LIBARCHIVE_SOURCES:-}" ] || { [ "$shell" = macos ] && [ -z "${XMAC_CLANG:-}" ]; }; then
    exec nix develop "$root#$shell" --command "$0" "$@"
fi
sources="$(readlink -f "$SKWIR_LIBARCHIVE_SOURCES")"

# What builds it, which the folder's name answers for with the rest.
if [ "$shell" = macos ]; then
    xmac="${XDG_DATA_HOME:-$HOME/.local/share}/skwir-build/macos-sdk"
    if [ ! -f "$xmac/env.sh" ]; then
        echo "No macOS SDK in $xmac: run \`just macos-sdk\` (Apple's licence is yours to accept)" >&2
        exit 1
    fi
    # shellcheck disable=SC2016 # env.sh's own words, `$_xmac_root` in them
    sdk="$xmac/$(sed -n 's/^export SDKROOT="\$_xmac_root\/\(.*\)"$/\1/p' "$xmac/env.sh")"
    toolchain="$XMAC_CLANG $XMAC_LLD $sdk"
else
    toolchain="zig $(zig version)"
fi
key="$( (printf '%s\n' "$target" "$sources" "$toolchain" && cat "$0") | sha256sum | cut -c1-16)"
cache="${XDG_CACHE_HOME:-$HOME/.cache}/skwir-build/libarchive"
out="$cache/$target-$key"
if [ -f "$out/link" ]; then
    echo "$out"
    exit 0
fi

work="$cache/.$target-$key"
rm -rf "${work:?}"
prefix="$work/prefix"
mkdir -p "$prefix/include" "$prefix/lib" "$work/build"
log="$work/build.log"
echo "Building libarchive for $target (a few minutes, once; log: $log)" >&2

# The dev shell's Linux flags stay out (NIX_CFLAGS_COMPILE, CMake's
# paths): they would reach a build for another system, and the shell's
# 200 KB of them make each of CMake's checks a dozen times slower.
clean() {
    env -i PATH="$PATH" HOME="$HOME" XDG_CACHE_HOME="${XDG_CACHE_HOME:-$HOME/.cache}" "$@" >>"$log" 2>&1 || {
        tail -n 30 "$log" >&2
        echo "libarchive for $target: failed ($*); log: $log" >&2
        exit 1
    }
}
jobs="$(nproc)"
# No folder of this machine in what is built (a package's check refuses a
# program that names one).
cflags="-O2"
for name in libarchive zlib bzip2 xz zstd lz4; do
    cflags="$cflags -ffile-prefix-map=$(readlink -f "$sources/$name")=/$name -ffile-prefix-map=$work/build/$name=/$name"
done

if [ "$shell" = windows ]; then
    mkdir -p "$work/bin"
    for tool in "cc:zig cc -target x86_64-windows-gnu" "c++:zig c++ -target x86_64-windows-gnu" \
        "ar:zig ar" "ranlib:zig ranlib"; do
        printf '#!/bin/sh\nexec %s "$@"\n' "${tool#*:}" > "$work/bin/${tool%%:*}"
        chmod +x "$work/bin/${tool%%:*}"
    done
    cat > "$work/toolchain.cmake" <<EOF
set(CMAKE_SYSTEM_NAME Windows)
set(CMAKE_SYSTEM_PROCESSOR x86_64)
set(CMAKE_C_COMPILER "$work/bin/cc")
set(CMAKE_CXX_COMPILER "$work/bin/c++")
set(CMAKE_AR "$work/bin/ar")
set(CMAKE_RANLIB "$work/bin/ranlib")
set(CMAKE_FIND_ROOT_PATH "$prefix")
set(CMAKE_FIND_ROOT_PATH_MODE_PROGRAM NEVER)
set(CMAKE_FIND_ROOT_PATH_MODE_LIBRARY ONLY)
set(CMAKE_FIND_ROOT_PATH_MODE_INCLUDE ONLY)
set(CMAKE_FIND_ROOT_PATH_MODE_PACKAGE ONLY)
EOF
    # Static liblzma: its header otherwise declares what a DLL exports.
    cflags="$cflags -DLZMA_API_STATIC"
else
    # xmac's toolchain, the libraries built here found before the SDK's
    # (which has a liblzma without its header).
    cat > "$work/toolchain.cmake" <<EOF
set(XMAC_CLANG "$XMAC_CLANG")
set(XMAC_LLD "$XMAC_LLD")
include("$xmac/arm64-apple-darwin.toolchain.cmake")
set(CMAKE_FIND_ROOT_PATH "$prefix;\${XMAC_SDK}")
EOF
fi

# A CMake project, configured, built and installed into the prefix.
configure() {
    name="$1" source="$2"
    shift 2
    clean cmake -S "$source" -B "$work/build/$name" \
        -DCMAKE_TOOLCHAIN_FILE="$work/toolchain.cmake" -DCMAKE_BUILD_TYPE=Release \
        -DCMAKE_INSTALL_PREFIX="$prefix" -DCMAKE_C_FLAGS="$cflags" \
        -DBUILD_SHARED_LIBS=OFF -DBUILD_TESTING=OFF "$@"
    clean cmake --build "$work/build/$name" --parallel "$jobs" --target install
}

if [ "$shell" = windows ]; then
    configure zlib "$sources/zlib" -DZLIB_BUILD_SHARED=OFF -DZLIB_BUILD_STATIC=ON -DZLIB_BUILD_TESTING=OFF
    # bzip2 1.0 has a makefile alone: the objects it lists, each named
    # (Zig names an object for Windows .obj).
    mkdir -p "$work/build/bzip2"
    objects="$(sed -n '/^OBJS=/,/[^\\]$/p' "$sources/bzip2/Makefile" | sed 's/^OBJS=//; s/\\//')"
    for object in $objects; do
        # shellcheck disable=SC2086 # the flags, one argument each
        clean "$work/bin/cc" $cflags -D_FILE_OFFSET_BITS=64 -c "$sources/bzip2/${object%.o}.c" -o "$work/build/bzip2/$object"
    done
    # shellcheck disable=SC2086 # one argument per object
    (cd "$work/build/bzip2" && clean "$work/bin/ar" rcs libbz2.a $objects)
    cp "$sources/bzip2/bzlib.h" "$prefix/include/"
    cp "$work/build/bzip2/libbz2.a" "$prefix/lib/"
fi
configure xz "$sources/xz" -DXZ_NLS=OFF -DXZ_DOC=OFF -DXZ_TOOL_XZ=OFF -DXZ_TOOL_XZDEC=OFF \
    -DXZ_TOOL_LZMADEC=OFF -DXZ_TOOL_LZMAINFO=OFF -DXZ_TOOL_SCRIPTS=OFF -DXZ_TOOL_SYMLINKS=OFF \
    -DXZ_TOOL_SYMLINKS_LZMA=OFF
configure zstd "$sources/zstd/build/cmake" -DZSTD_BUILD_PROGRAMS=OFF -DZSTD_BUILD_SHARED=OFF \
    -DZSTD_BUILD_STATIC=ON -DZSTD_BUILD_TESTS=OFF -DZSTD_MULTITHREAD_SUPPORT=OFF -DZSTD_LEGACY_SUPPORT=OFF
configure lz4 "$sources/lz4/build/cmake" -DLZ4_BUILD_CLI=OFF -DLZ4_BUILD_LEGACY_LZ4C=OFF -DBUILD_STATIC_LIBS=ON

# libarchive, given each library built here by name (the SDK has a
# liblzma of its own). What it reads needs no program of the system's.
# Neither OpenSSL nor its like: the system's cryptography does ZIP's
# encryption.
set -- -DENABLE_OPENSSL=OFF -DENABLE_MBEDTLS=OFF -DENABLE_NETTLE=OFF -DENABLE_LIBB2=OFF \
    -DENABLE_PCREPOSIX=OFF -DENABLE_PCRE2POSIX=OFF -DENABLE_LZO=OFF -DENABLE_LIBXML2=OFF \
    -DENABLE_ZLIB=ON -DENABLE_BZip2=ON -DENABLE_LZMA=ON -DENABLE_ZSTD=ON -DENABLE_LZ4=ON \
    -DLIBLZMA_INCLUDE_DIR="$prefix/include" -DLIBLZMA_LIBRARY="$prefix/lib/liblzma.a" \
    -DZSTD_INCLUDE_DIR="$prefix/include" -DZSTD_LIBRARY="$prefix/lib/libzstd.a" \
    -DLZ4_INCLUDE_DIR="$prefix/include" -DLZ4_LIBRARY="$prefix/lib/liblz4.a" \
    -DENABLE_TAR=OFF -DENABLE_CPIO=OFF -DENABLE_CAT=OFF -DENABLE_UNZIP=OFF -DENABLE_TEST=OFF
if [ "$shell" = windows ]; then
    zlib="$(cd "$prefix/lib" && ls libz.a libzs.a 2>/dev/null | head -n 1)"
    configure libarchive "$sources/libarchive" "$@" -DENABLE_CNG=ON -DENABLE_WIN32_XMLLITE=ON \
        -DENABLE_EXPAT=OFF -DENABLE_ICONV=OFF -DENABLE_LIBGCC=OFF -DPOSIX_REGEX_LIB=NONE \
        -DZLIB_INCLUDE_DIR="$prefix/include" -DZLIB_LIBRARY="$prefix/lib/$zlib" \
        -DBZIP2_INCLUDE_DIR="$prefix/include" -DBZIP2_LIBRARIES="$prefix/lib/libbz2.a"
    zlib="${zlib#lib}"
    links="static=archive static=lz4 static=zstd static=lzma static=bz2 static=${zlib%.a} bcrypt xmllite ole32"
    carried="libarchive zlib bzip2 xz zstd lz4"
else
    configure libarchive "$sources/libarchive" "$@" -DENABLE_EXPAT=ON -DENABLE_ICONV=ON
    links="static=archive static=lz4 static=zstd static=lzma z bz2 iconv expat"
    carried="libarchive xz zstd lz4"
fi

# What libarchive found, said where a mistake would show: the formats it
# reads come from these.
config="$work/build/libarchive/config.h"
if [ "$shell" = windows ]; then
    system="HAVE_BCRYPT_H HAVE_XMLLITE_H"
else
    system="HAVE_ICONV HAVE_EXPAT_H"
fi
for wanted in HAVE_LIBLZMA HAVE_LIBZSTD HAVE_LIBLZ4 HAVE_LIBBZ2 HAVE_ZLIB_H $system; do
    grep -q "^#define $wanted 1" "$config" || {
        echo "libarchive for $target was built without $wanted" >&2
        exit 1
    }
done

stage="$work/out"
mkdir -p "$stage/lib"
for lib in $links; do
    case "$lib" in
    static=*) cp "$prefix/lib/lib${lib#static=}.a" "$stage/lib/" ;;
    esac
done
printf '%s\n' $links > "$stage/link"
printf '%s\n' "$target" > "$stage/target"
for name in $carried; do
    mkdir -p "$stage/licenses/$name"
    case "$name" in
    libarchive) cp "$sources/libarchive/COPYING" "$stage/licenses/$name/" ;;
    xz) cp "$sources/xz/COPYING" "$sources/xz/COPYING.0BSD" "$stage/licenses/$name/" ;;
    lz4) cp "$sources/lz4/lib/LICENSE" "$stage/licenses/$name/" ;;
    *) cp "$sources/$name/LICENSE" "$stage/licenses/$name/" ;;
    esac
    grep "^$name " "$sources/versions" >> "$stage/licenses/libarchive/VERSIONS"
done
chmod -R u+w "$stage/licenses"

# Kept, and the builds of other sources or recipes let go, with what a
# failed one left.
for old in "$cache/$target"-* "$cache/.$target"-*; do
    [ -d "$old" ] && [ "$old" != "$work" ] && rm -rf "${old:?}"
done
mv "$stage" "$out"
rm -rf "${work:?}"
echo "$out"
