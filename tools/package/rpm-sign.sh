#!/bin/sh
# `.rpm` packages signed by the repositories' key (ADR-142, amended), so
# a system that installs only what is signed by a key it trusts
# (PackageKit's, under GNOME Software and Discover) takes them, and dnf
# checks each (`gpgcheck=1`). Signed here, by GnuPG's keyring, never in
# the distribution's root that built them: the secret part stays with
# whoever publishes. A signature another key made goes first, so each
# file ends signed by this key alone; each is then read back with this
# key alone, and one that does not say so stops the run.
#
#     tools/package/rpm-sign.sh FINGERPRINT [--at SECONDS] FILE...
#
# `--at` dates the signatures (seconds since the epoch, or the key's own
# making where that is later: gpg never signs before it): an EdDSA
# signature is then the same each time, and a package signed again stays
# the same file byte for byte. Without it, they are dated now. Needs
# rpmsign and rpmkeys (`just package-try`, `just repositories` and `just
# repository-check` find them in the flake's nixpkgs) and gpg, whose agent
# asks for the key's passphrase.
set -eu
usage() {
    echo "usage: $0 FINGERPRINT [--at SECONDS] FILE..." >&2
    exit 2
}
[ $# -ge 2 ] || usage
key="$1"
shift
at=""
if [ "$1" = --at ]; then
    [ $# -ge 3 ] || usage
    at="$2"
    shift 2
fi
for tool in rpmsign rpmkeys gpg; do
    command -v "$tool" >/dev/null || { echo "$0: $tool is needed" >&2; exit 1; }
done
gpg --batch --list-secret-keys "$key" >/dev/null 2>&1 ||
    { echo "$0: $key: its secret part is not in GnuPG's keyring" >&2; exit 1; }
gpg_program="$(command -v gpg)"
extra=""
if [ -n "$at" ]; then
    # The key's newest part (it or a subkey that signs) was made then.
    made="$(gpg --batch --with-colons --list-keys "$key" | awk -F: '$1 == "pub" || $1 == "sub" { if ($6 > m) m = $6 } END { print m + 0 }')"
    [ "$at" -ge "$made" ] || at="$made"
    extra="--faked-system-time $at!"
fi

# This key alone, as the files are read back.
keyring="$(mktemp -d)"
trap 'rm -rf "$keyring"' EXIT INT TERM
gpg --batch --armor --export "$key" > "$keyring/key.asc"
rpmkeys --dbpath "$keyring" --import "$keyring/key.asc"

for file in "$@"; do
    rpmsign --delsign "$file" >/dev/null
    rpmsign --define "_gpg_name $key" --define "__gpg $gpg_program" \
        ${extra:+--define "_gpg_sign_cmd_extra_args $extra"} --addsign "$file" >/dev/null
    said="$(LC_ALL=C rpmkeys --dbpath "$keyring" --checksig "$file")"
    case "$said" in
    *" signatures OK") echo "signed: $file" ;;
    *)
        echo "$0: $file: not signed by $key: $said" >&2
        exit 1
        ;;
    esac
done
