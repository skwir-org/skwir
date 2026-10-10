# Vendored shared-mime-info 2.5.1

The type database's source, `data/freedesktop.org.xml.in`, from
shared-mime-info 2.5.1 (https://gitlab.freedesktop.org/xdg/shared-mime-info,
tag `2.5.1`; release tarball sha256
`ea248ea157b7fa0165f4fe282c84919fa84c3f175553642f229c8f1ab7539128`),
unchanged, with its licence (`COPYING`, GPL-2.0-or-later). `VERSION`
says which release it is.

It is data, not code skwir builds: `tools/package/mime-database.sh`
compiles it with shared-mime-info's own `update-mime-database` into the
files the macOS and Windows packages carry beside skwir, the fallback
of each system's own types (ADR-037). On Linux skwir reads the database
the desktop installed, never this one.

Left out: the translations (`po/`), which the packages do not carry
(each system names types in its own words), the compiler and its tests.

To update: replace `freedesktop.org.xml.in` and `COPYING` with a new
release's, and `VERSION`.
