#!/bin/sh
# In a root entered as its root user in a user namespace of one user
# (tools/package/try.sh), a package's script cannot give a file to
# another user or group (Ubuntu's fontconfig gives /usr/local/share/fonts
# to `staff`): dpkg runs under fakeroot there, which keeps such owners in
# memory only, as the AppImage's roots do (appimage/install-deps.sh).
#
#     tools/package/linux/rootless-apt.sh     # as the root's root, once
set -eu
if ! command -v fakeroot-sysv >/dev/null; then
    apt-get update -q
    mkdir -p /tmp/fakeroot
    (cd /tmp/fakeroot && apt-get download -q fakeroot libfakeroot)
    for package in /tmp/fakeroot/*.deb; do
        dpkg-deb --extract "$package" /
    done
    rm -rf /tmp/fakeroot
fi
cat > /usr/local/sbin/dpkg-rootless <<'SCRIPT'
#!/bin/sh
FAKEROOTDONTTRYCHOWN=1 exec fakeroot-sysv /usr/bin/dpkg "$@"
SCRIPT
chmod +x /usr/local/sbin/dpkg-rootless
echo 'Dir::Bin::dpkg "/usr/local/sbin/dpkg-rootless";' > /etc/apt/apt.conf.d/99skwir-rootless
