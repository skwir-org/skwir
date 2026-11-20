#!/bin/sh
# skwir-repository's preinst (its .deb). Removed and not purged, the
# package leaves dpkg remembering its source, which the postrm took away,
# as one its user deleted, and dpkg never puts back a configuration file
# its user deleted: added again, the repository would not be. So, added
# again ("install" with the version it had), the package puts its source
# back itself first, as it carries it, which dpkg then finds its own.
set -e
if [ "$1" = install ] && [ -n "${2:-}" ] && [ ! -e /etc/apt/sources.list.d/skwir.sources ]; then
    cat > /etc/apt/sources.list.d/skwir.sources <<'SKWIR_SOURCE'
@source@SKWIR_SOURCE
fi
exit 0
