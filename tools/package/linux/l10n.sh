#!/bin/sh
# skwir's desktop entry and AppStream metainfo in each language they are
# translated to (ADR-139, 13), merged where each package is made.
#
# The English is the two files beside this script, valid as they are:
# org.skwir.skwir.desktop and org.skwir.skwir.metainfo.xml. What of them
# is translated is extracted by xgettext into po/skwir.pot: the desktop
# entry's GenericName, Comment and Keywords (its Name is the brand, never
# translated), and the metainfo's text as AppStream's own rules say
# (po/its: carried, so that every system extracts and merges alike,
# whichever gettext and AppStream it has; `translate="no"` keeps the
# brand out there). Each language is a PO file, po/<language>.po, that
# po/LINGUAS names. A message marked fuzzy is a model's draft (ADR-139,
# 13): it ships, as the program's drafted catalogs do, until a person
# who speaks the language accepts it and removes the mark. msgmerge never
# guesses (`--no-fuzzy-matching`): an English sentence changed leaves
# its translation obsolete, never a stale one shipped under new English.
#
#     tools/package/linux/l10n.sh merge <share>      # into <share>/applications and <share>/metainfo
#     tools/package/linux/l10n.sh installed <share>  # whether those speak every language of po/LINGUAS
#     tools/package/linux/l10n.sh update             # po/skwir.pot made again, each PO merged with it
#     tools/package/linux/l10n.sh check              # po/ current and sound, merging into valid files
#
# `merge`, `update` and `check` need gettext (0.21 or later: Ubuntu
# 22.04's, which the AppImage is built on); `installed`, a shell alone.
# `check` also validates what it merged with desktop-file-validate and
# `appstreamcli validate --no-net` where they are on PATH.
set -eu
here="$(cd "$(dirname "$0")" && pwd)"
po="$here/po"
desktop=org.skwir.skwir.desktop
metainfo=org.skwir.skwir.metainfo.xml

# The languages po/LINGUAS names, one a line.
languages() {
    sed 's/#.*//' "$po/LINGUAS" | tr -s ' \t' '\n\n' | grep -v '^$' || :
}

# The two files in every language, drafts included.
merge() {
    mkdir -p "$1/applications" "$1/metainfo"
    msgfmt --desktop --use-fuzzy --template="$here/$desktop" -d "$po" \
        -o "$1/applications/$desktop"
    GETTEXTDATADIRS="$po${GETTEXTDATADIRS:+:$GETTEXTDATADIRS}" \
        msgfmt --xml --use-fuzzy -L skwir-metainfo --template="$here/$metainfo" -d "$po" \
        -o "$1/metainfo/$metainfo"
    chmod 644 "$1/applications/$desktop" "$1/metainfo/$metainfo"
}

# Whether the files installed in $1 hold each language: a key of the
# desktop entry, and an element of the metainfo, in it.
installed() {
    missing=0
    for language in $(languages); do
        if grep -q "^[A-Za-z-]*\[$language\]=" "$1/applications/$desktop" &&
            grep -q "xml:lang=\"$language\"" "$1/metainfo/$metainfo"; then
            echo "ok: the desktop entry and the metainfo in $language"
        else
            echo "missing: the desktop entry or the metainfo in $language, in $1"
            missing=1
        fi
    done
    return "$missing"
}

# What to translate, from the two files, into $1.
extract() {
    (
        cd "$here"
        xgettext --from-code=UTF-8 -L Desktop -k -kGenericName -kComment -kKeywords \
            --add-comments=Translators: --package-name=skwir \
            --copyright-holder='The skwir authors' \
            --msgid-bugs-address=https://skwir.org/issues -o "$1.desktop" "$desktop"
        # The charset named, as gettext 0.22 and later name it where all is
        # ASCII; then the metainfo's messages added.
        sed 's/charset=CHARSET/charset=UTF-8/' "$1.desktop" > "$1.made"
        xgettext -j --from-code=UTF-8 --its="$po/its/metainfo.its" \
            --add-comments=Translators: --package-name=skwir \
            --copyright-holder='The skwir authors' \
            --msgid-bugs-address=https://skwir.org/issues -o "$1.made" "$metainfo"
    )
    sed -e "1s/.*/# skwir's desktop entry and AppStream metainfo, to translate./" \
        -e '/^# FIRST AUTHOR/d' "$1.made" > "$1"
    rm -f "$1.desktop" "$1.made"
}

# A PO file's messages, without its header (and the date it bears) and
# where each is found, which gettext 0.21 counts a line off in a desktop
# entry: what translators read, the same whichever gettext made it.
messages() {
    sed -e '1,/^$/d' -e '/^#:/d' "$1"
}

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
case "${1:-}" in
merge)
    [ "$#" -eq 2 ] || { echo "usage: $0 merge <share>" >&2; exit 2; }
    merge "$2"
    ;;
installed)
    [ "$#" -eq 2 ] || { echo "usage: $0 installed <share>" >&2; exit 2; }
    installed "$2"
    ;;
update)
    extract "$work/skwir.pot"
    if [ -f "$po/skwir.pot" ] && [ "$(messages "$work/skwir.pot")" = "$(messages "$po/skwir.pot")" ]; then
        echo "po/skwir.pot: as the files say"
    else
        cp "$work/skwir.pot" "$po/skwir.pot"
        echo "po/skwir.pot: made again"
    fi
    for language in $(languages); do
        msgmerge --quiet --update --backup=none --no-fuzzy-matching "$po/$language.po" "$po/skwir.pot"
        echo "po/$language.po: merged with it"
    done
    ;;
check)
    failed=0
    extract "$work/skwir.pot"
    if [ "$(messages "$work/skwir.pot")" = "$(messages "$po/skwir.pot")" ]; then
        echo "ok: po/skwir.pot is what the files say"
    else
        echo "po/skwir.pot is not what the files say: run $0 update" >&2
        failed=1
    fi
    for language in $(languages); do
        if ! msgfmt --check --use-fuzzy -o /dev/null "$po/$language.po"; then
            echo "po/$language.po: not sound" >&2
            failed=1
        elif ! msgcmp --use-fuzzy --use-untranslated "$po/$language.po" "$po/skwir.pot"; then
            echo "po/$language.po is not merged with po/skwir.pot: run $0 update" >&2
            failed=1
        else
            echo "ok: po/$language.po"
        fi
    done
    merge "$work/share"
    installed "$work/share" || failed=1
    if command -v desktop-file-validate >/dev/null; then
        for file in "$here/$desktop" "$work/share/applications/$desktop"; do
            if desktop-file-validate "$file"; then
                echo "ok: desktop-file-validate $file"
            else
                failed=1
            fi
        done
    else
        echo "not validated: no desktop-file-validate on PATH"
    fi
    if command -v appstreamcli >/dev/null; then
        for file in "$here/$metainfo" "$work/share/metainfo/$metainfo"; do
            if appstreamcli validate --no-net --pedantic "$file" >"$work/said" 2>&1; then
                echo "ok: appstreamcli validate $file"
            else
                cat "$work/said" >&2
                failed=1
            fi
        done
    else
        echo "not validated: no appstreamcli on PATH"
    fi
    exit "$failed"
    ;;
*)
    echo "usage: $0 merge <share> | installed <share> | update | check" >&2
    exit 2
    ;;
esac
