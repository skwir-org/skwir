# How a copy came (ADR-142)

Each of skwir's own packages carries one of these words as
`share/skwir/channel`, in the data folder beside its programs (above
`bin` on Linux, beside the program on Windows, a Mac's
`Resources/share`): `skwir_platform::channel` reads it, and the
`updates` plugin asks a new version only of a copy that came so and
that nothing else updates.

| File | Written by | In |
| --- | --- | --- |
| `appimage` | `appimage/stage.py` | the AppImage |
| `tarball` | `appimage/build.sh` | the AppImage's tree as a tarball |
| `windows-zip`, `windows-installer` | `windows.sh` | Windows' zip, then its installer |
| `macos` | `macos.sh` | the Mac app |
| `deb`, `rpm` | `crates/skwir/Cargo.toml`'s assets | the `.deb` and the `.rpm` |

A build from source has none, nor a package manager's: the AUR's
`skwir` and `skwir-git`, Nix's, a distribution's. `skwir-bin`, which
installs the tarball as it is, removes the tarball's. A package manager
that repackages a release (as `skwir-bin` does) removes it too: what it
updates is never told of a new version by skwir.

A `.deb` or an `.rpm` built from the source by `deb.sh` or `rpm.sh` (or
by cargo-deb or cargo-generate-rpm, from `crates/skwir/Cargo.toml`'s
assets) carries the word as the release's do: cargo-deb refuses an asset
no file matches, and neither tool takes one from its command line, so the
word is the metadata's, not the release script's alone. One built for a
repository of its own (a PPA, a Copr) leaves the asset's line out, or its
copies ask about new versions while that repository updates them.

Each file is the word and a line's end, nothing else: skwir reads the
word and refuses any other.
