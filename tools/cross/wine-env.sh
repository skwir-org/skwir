# Sourced by every script that runs Wine (ADR-035, ADR-074), once
# WINEPREFIX names the prefix of its own: a Wine that leaves nothing in
# the user's desktop. tools/package/gstreamer.py sets the same for its
# installer.
#
# - No Mono or Gecko: their installers would ask on first start.
# - No winemenubuilder: at every wineboot it writes the prefix's file
#   associations and Start menu into the user's applications, menus and
#   type database (~/.local/share/applications, ~/.config/menus,
#   ~/.local/share/mime), where they outlive the prefix.
# - XDG data and config folders beside the prefix, for anything else Wine
#   would write there. The cache stays: Zig keeps its own in it.
export WINEDLLOVERRIDES="mscoree,mshtml=;winemenubuilder.exe=d"
export WINEDEBUG="${WINEDEBUG:--all}"
export XDG_DATA_HOME="$WINEPREFIX-xdg/data" XDG_CONFIG_HOME="$WINEPREFIX-xdg/config"
mkdir -p "$XDG_DATA_HOME" "$XDG_CONFIG_HOME"
