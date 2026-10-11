#!/bin/sh
# A portable AppImage built on any x86-64 Linux, including NixOS, in an
# Ubuntu 22.04 root of its own (ADR-052). No container engine or root.
# The build root, downloads and Cargo artifacts stay in target/appimage;
# the distributable and its checksum land in target/package/appimage.
# Set SKWIR_PACKAGE_OUT to select a different destination for verified outputs.
set -eu
case "$(uname -sm)" in
'Linux x86_64') ;;
*) echo 'AppImage packaging is available on x86-64 Linux only' >&2; exit 1 ;;
esac
for program in bwrap python3 git tar flock; do
    command -v "$program" >/dev/null || { echo "$program is needed" >&2; exit 1; }
done
tree="$(cd "$(dirname "$0")/../.." && pwd)"
cache="$tree/target/appimage"
root="$cache/root"
out="$cache/candidate"
dist="${SKWIR_PACKAGE_OUT:-$tree/target/package/appimage}"
mkdir -p "$cache" "$out" "$dist"
exec 9>"$cache/build.lock"
flock -n 9 || { echo 'An AppImage build is already running' >&2; exit 1; }
python3 "$tree/tools/package/appimage/fetch.py" "$cache/downloads"
if [ ! -d "$root" ]; then
    mkdir -p "$root"
    tar --no-same-owner -xzf "$cache/downloads/ubuntu-base.tar.gz" -C "$root" --exclude='./dev/*'
    chmod u+rwx "$root/root"
    echo 'APT::Sandbox::User "root";' > "$root/etc/apt/apt.conf.d/99skwir-sandbox"
fi
rm -f "$root/etc/resolv.conf"
cp -L /etc/resolv.conf "$root/etc/resolv.conf"
mkdir -p "$root/src" "$root/out" "$root/inputs"
# No git history, personal build paths or host toolchains enter the root.
# Preserve Cargo's artifacts separately from this fresh source snapshot.
rm -rf "${root:?}/src"
mkdir -p "$root/src"
(cd "$tree" && git ls-files -co --exclude-standard -z | tar --null -T - -cf -) |
    tar -xf - -C "$root/src"
enter() {
    entered_root="$1"
    shift
    bwrap --unshare-user --uid 0 --gid 0 --cap-add ALL --unshare-pid --share-net --die-with-parent \
        --bind "$entered_root" / --proc /proc --dev /dev --tmpfs /tmp \
        --bind "$root/src" /src \
        --ro-bind "$cache/downloads" /inputs --bind "$out" /out \
        --clearenv --setenv HOME /root --setenv LANG C.UTF-8 \
        --setenv PATH /root/.cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin \
        --setenv CARGO_BUILD_JOBS "${CARGO_BUILD_JOBS:-4}" \
        --chdir /src sh -c '
            # Linux 6.7+ gives this user namespace a private registry.
            # Otherwise NixOS may intercept AppImages and try to invoke
            # its host appimage-run, which is deliberately absent here.
            # Older kernels without such a handler need no registry.
            mount -t binfmt_misc binfmt_misc /proc/sys/fs/binfmt_misc 2>/dev/null || :
            exec "$@"
        ' sh "$@"
}
enter "$root" sh tools/package/appimage/prepare.sh
enter "$root" sh tools/package/appimage/build.sh
# A separate root has desktop runtime packages, no compiler, development
# libraries, Nix store or host profile. Hidden build dependencies fail here.
test_root="$cache/test-root"
if [ ! -d "$test_root" ]; then
    mkdir -p "$test_root"
    tar --no-same-owner -xzf "$cache/downloads/ubuntu-base.tar.gz" -C "$test_root" --exclude='./dev/*'
    chmod u+rwx "$test_root/root"
    echo 'APT::Sandbox::User "root";' > "$test_root/etc/apt/apt.conf.d/99skwir-sandbox"
fi
rm -f "$test_root/etc/resolv.conf"
cp -L /etc/resolv.conf "$test_root/etc/resolv.conf"
mkdir -p "$test_root/src" "$test_root/out" "$test_root/inputs"
# shellcheck disable=SC2046 # one argument per runtime dependency
enter "$test_root" sh tools/package/appimage/install-deps.sh $(cat "$tree/tools/package/appimage/test-deps")
version="$(sed -n 's/^version = "\(.*\)"$/\1/p' "$tree/Cargo.toml" | head -1)"
enter "$test_root" python3 tools/package/appimage/check.py "/out/skwir-$version-x86_64.AppImage"
# Only a verified candidate replaces the previous distributable.
name="skwir-$version-x86_64.AppImage"
install -m755 "$out/$name" "$dist/$name.part"
mv "$dist/$name.part" "$dist/$name"
install -m644 "$out/$name.sha256" "$dist/$name.sha256.part"
mv "$dist/$name.sha256.part" "$dist/$name.sha256"
echo "AppImage and checksum: $dist"
