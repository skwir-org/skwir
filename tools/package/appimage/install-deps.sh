#!/bin/sh
# In a rootless Ubuntu/Debian root: package scripts cannot really chown
# to unmapped service users. Emulate ownership only while installing.
set -eu
apt-get update -q
printf '#!/bin/sh\nexit 101\n' > /usr/sbin/policy-rc.d
chmod +x /usr/sbin/policy-rc.d
if ! command -v fakeroot-sysv >/dev/null; then
    mkdir -p /tmp/fakeroot
    (cd /tmp/fakeroot && apt-get download fakeroot libfakeroot)
    for package in /tmp/fakeroot/*.deb; do
        dpkg-deb --extract "$package" /
    done
fi
export DEBIAN_FRONTEND=noninteractive FAKEROOTDONTTRYCHOWN=1
# Refresh dependencies already present in the base image too, rather
# than retaining their old security fixes when reusing a build root.
fakeroot-sysv apt-get upgrade -y -q --no-install-recommends
fakeroot-sysv apt-get install -y -q --no-install-recommends "$@"
apt-get clean
