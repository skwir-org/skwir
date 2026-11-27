#!/bin/sh
# An installer of skwir (ADR-074): skwir.wxs, or the copy of it named last,
# with the files.wxs given, made by msitools' wixl, then the rows of the
# search wixl has no element for, added by msibuild. windows.sh makes
# skwir's with it; windows-check.sh the small ones whose upgrades it tries,
# so that both carry the same rows.
#
#     tools/package/windows/msi.sh <version> <folder> <icon> <files.wxs> <out.msi> [<skwir.wxs>]
set -eu
version="$1" folder="$2" icon="$3" files="$4" msi="$5"
wxs="${6:-$(dirname "$0")/skwir.wxs}"
wixl -a x64 -D "Version=$version" -D "Dist=$folder" -D "Icon=$icon" -D Win64=yes -o "$msi" "$wxs" "$files"
# skwir.wxs's INSTALLED_SKWIRDW: the skwirdw.exe in the folder the package
# installs into, where one is there, as WiX's DirectorySearch and
# FileSearch, which wixl 0.106 lacks, would search for it (AppSearch,
# DrLocator, Signature; windows-check.sh reads them).
# shellcheck disable=SC2016 # SQL's backquotes, which name a table or a column
msibuild "$msi" \
    -q 'CREATE TABLE `DrLocator` (`Signature_` CHAR(72) NOT NULL, `Parent` CHAR(72), `Path` CHAR(255), `Depth` SHORT PRIMARY KEY `Signature_`, `Parent`, `Path`)' \
    -q "INSERT INTO \`DrLocator\` (\`Signature_\`, \`Path\`, \`Depth\`) VALUES ('InstalledSkwirdw', '[LocalAppDataFolder]Programs\\skwir', 0)" \
    -q "INSERT INTO \`Signature\` (\`Signature\`, \`FileName\`) VALUES ('InstalledSkwirdw', 'skwirdw.exe')" \
    -q "INSERT INTO \`AppSearch\` (\`Property\`, \`Signature_\`) VALUES ('INSTALLED_SKWIRDW', 'InstalledSkwirdw')"
