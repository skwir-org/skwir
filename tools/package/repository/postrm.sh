#!/bin/sh
# skwir-repository's postrm (its .deb): removing the package removes the
# repository. Its source is a configuration file, so that a local edit is
# kept across updates (Debian's rule), which dpkg itself takes only on
# purge; the key it names goes with the package, and the source left would
# fail every `apt update`. What dpkg left of it beside goes too.
set -e
case "$1" in
remove | purge)
    for file in /etc/apt/sources.list.d/skwir.sources /etc/apt/sources.list.d/skwir.sources.dpkg-dist \
        /etc/apt/sources.list.d/skwir.sources.dpkg-old /etc/apt/sources.list.d/skwir.sources.dpkg-new; do
        rm -f "$file"
    done
    ;;
esac
exit 0
