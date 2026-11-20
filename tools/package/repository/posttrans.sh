# skwir-repository's %posttrans: what rpm and dnf keep of Skwir's
# repository keys brought to what the package carries, once this
# transaction is done (rpm's database is its own until then):
# /usr/libexec/skwir-repository/keys, started apart, waits for it, and
# says what fails to the system's log.
if command -v setsid >/dev/null 2>&1; then
    setsid sh -c 'if command -v logger >/dev/null 2>&1; then
        /usr/libexec/skwir-repository/keys 2>&1 | logger -t skwir-repository
    else
        /usr/libexec/skwir-repository/keys
    fi' </dev/null >/dev/null 2>&1 &
fi
exit 0
