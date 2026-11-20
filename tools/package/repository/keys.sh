#!/bin/sh
# What rpm and dnf keep of Skwir's repository keys, brought to what
# skwir-repository carries (ADR-142), as /usr/libexec/skwir-repository/keys.
#
# rpm keeps a key as dnf first imported it, its expiry too, and dnf
# imports a key only when rpm has none of it: an extension, which
# skwir-repository brings to /etc/pki/rpm-gpg, would never reach rpm, and
# a package signed after the first expiry would be refused (Fedora 44,
# tried). rpm also keeps a key skwir-repository no longer carries, and
# dnf the keys it read the repository's index with. So each key the
# package carries that rpm holds is imported again (rpm takes the newer
# copy), and each key /usr/share/skwir-repository/keys says is retired
# leaves rpm, and dnf's keys for the index, which dnf then reads again
# from what the package carries, asking as it did when the repository
# was added. A key rpm does not hold is left to dnf, which asks before
# it imports one.
#
# skwir-repository's %posttrans starts this apart from the transaction,
# which holds rpm's database until it ends: rpmkeys is tried again each
# second until then. Run by hand, as root, it does the same.
set -u
list=/usr/share/skwir-repository/keys
[ -r "$list" ] || exit 0

# rpmkeys "$@", once rpm's database is free (reading it waits too, and
# `--list` says the lock is held with success): ten minutes at most. What
# it says, once it succeeds.
rpm_keys() {
    tries=0
    while :; do
        if said="$(rpmkeys "$@" 2>&1)"; then done=yes; else done=no; fi
        case "$said" in
        *"transaction lock"*) ;;
        *)
            if [ "$done" = yes ]; then
                echo "$said"
                return 0
            fi
            echo "rpmkeys $*: $said" >&2
            return 1
            ;;
        esac
        tries=$((tries + 1))
        if [ "$tries" -ge 600 ]; then
            echo "rpmkeys $*: rpm's database stayed in use" >&2
            return 1
        fi
        sleep 1
    done
}

# dnf's keys for Skwir's index, kept with its cache (dnf5's one file a
# key, named by its long key ID; dnf's and PackageKit's a keyring).
index_keys() {
    for folder in /var/cache/libdnf5/skwir-*/pubring /var/cache/dnf/skwir-*/pubring \
        /var/cache/PackageKit/*/metadata/skwir-*/pubring; do
        [ -d "$folder" ] && echo "$folder"
    done
}

held="$(rpm_keys --list)" || exit 1
held="$(echo "$held" | tr 'A-F' 'a-f')"
status=0
while read -r what fingerprint file; do
    case "$what" in
    carried | retired) ;;
    *) continue ;;
    esac
    fingerprint="$(echo "$fingerprint" | tr 'A-F' 'a-f')"
    case "$held" in
    *"$fingerprint"*) holds=yes ;;
    *) holds=no ;;
    esac
    if [ "$what" = carried ]; then
        [ "$holds" = no ] || rpm_keys --import "$file" >/dev/null || status=1
        continue
    fi
    [ "$holds" = no ] || rpm_keys --delete "$fingerprint" >/dev/null || status=1
    long="$(echo "$fingerprint" | cut -c25- | tr 'a-f' 'A-F')"
    for folder in $(index_keys); do
        if [ "$holds" = yes ] || [ -e "$folder/$long.pub" ]; then
            rm -rf "$folder"
        fi
    done
done < "$list"
exit "$status"
