# Releasing from Linux

Linux releases are built on an x86-64 workstation, including NixOS
(ADR-053). GitHub hosts the source and release assets. Routine CI runs
`just ci-check`: workspace formatting, core and plugin API lint/tests,
and release-tool integration tests. It does not compile the UI or native
workers, check performance or build packages.

## Prepare the source

1. Set the workspace version in `Cargo.toml`, `pkgver` in
   `tools/package/aur/PKGBUILD` and `tools/package/aur/skwir-bin/PKGBUILD`
   (`skwir-git` names its own), the AppStream release version and date
   in `tools/package/linux/org.skwir.skwir.metainfo.xml`, and the file
   names in `README.md`'s install commands.
2. Date that version's `CHANGELOG.md` heading, for example
   `## 0.1.0 (2026-10-11)`. AppStream must have the same date. The section
   supplies the GitHub release notes, so it must be the first: an
   `## Unreleased` section above it, which the tag would hold but its
   notes leave out, stops the release. Its links stay relative, as
   anywhere in the repository (`docs/HELP-WANTED.md`): on the release's
   page each leads to its file at the tag, and a link to no file of the
   source stops the release. The page shows what is above the section's
   `### All changes` heading, if it has one, then **Full details**, a
   link to the section: there go the summary, a line for each highlight
   and what changed that a user of the last release would notice, and
   below it every change in full. A short section without the heading
   is shown whole. Only stable
   `v<major>.<minor>.<patch>` tags are currently supported.
3. Run `just release-metadata v0.1.0`, review and commit the release source,
   then create its tag (`git tag -a v0.1.0 -m 'Release 0.1.0'`). Check out
   that commit. Builds and uploads require HEAD to equal the tag and the
   tree to be clean, including untracked files. Ignored build outputs do
   not count as source changes.

These commands do not commit, create tags or push source for you. Choose
the actual version and date; the examples do not declare 0.1.0 released.

## Build and inspect

Enter `nix develop` (or use the direnv shell), then:

```sh
just release-build v0.1.0
just release-verify v0.1.0
```

`release-build` first runs `just release-check`: `lint-all` (including
Windows/macOS cross checks), `deny`, `notices-check` (the packages' notices
in `tools/package/notices` are current, ADR-063), `test-all`, `smoke`,
`a11y-check` (what a screen reader hears, ADR-065), `poppler-check`, `mupdf-check`, `djvu-check`, `bench-check` and
`release-tools-check`. There is no skip
checks option. These checks are also useful before committing a change;
`release-check` itself does not require a tag or a clean tree.

It then runs the existing packagers with separate fresh output directories:

| Command | Build and installation checks |
| --- | --- |
| `just package-appimage` | Ubuntu 22.04 build root; the AppImage, and its tree as a tarball, each checked in a separate runtime root: ELF/glibc checks, helpers, PDFs, audio and window commands |
| `just package-try debian` | Debian 13 build; fresh installation of main package alone, then optional packages |
| `just package-try ubuntu` | Ubuntu 24.04 build, its revision naming the release (`-1ubuntu24.04`); the same installations |
| `just package-try fedora` | Fedora 44 build; fresh installation of main package alone, then optional packages |
| `just package-try arch` | Arch build; fresh installation of main package alone, then optional packages |

The final bundle is `target/releases/v0.1.0/`. It contains the lowercase
`skwir-0.1.0-x86_64.AppImage` and `skwir-0.1.0-linux-x86_64.tar.gz`, each
with its companion checksum, all current-version Debian, Ubuntu, RPM and
Arch split packages, `notes.md`, `manifest.json` and `SHA256SUMS`. The
package manifests supply the expected split package names. Missing,
duplicate, unexpected or old-version assets fail the build. The manifest
records the source commit, platform, checks and each asset's size and SHA256.
It is a local build record, not signed CI provenance.

A bundle appears only when all checks and packaging succeed. An existing
bundle is never overwritten: move it aside explicitly before rebuilding.
Builds reuse the AppImage and Debian/Ubuntu/Fedora Cargo caches and
prepared roots; Arch currently retains downloads but rebuilds its source
tree. First builds still take substantial time and disk space.
Distribution roots are cached under `~/.cache/skwir-tests/roots`; AppImage
inputs and build cache live in `target/appimage`. Package scripts also
accept `SKWIR_PACKAGE_OUT` to direct their verified outputs elsewhere.

Manually try the AppImage on the desktop and hardware you intend to support.
The automated AppImage and tarball checks use software Vulkan in their
baseline root; they cannot establish actual audio output or every GPU
driver's behavior. The manual `packages` workflow additionally tests the
same AppImage and tarball on Ubuntu 24.04 and Fedora 44. Those checks
are not part of the local release command.

## Upload a draft

Authenticate GitHub CLI once with `gh auth login --hostname github.com`.
Push the reviewed release commit and its exact tag yourself, then run:

```sh
just release-upload v0.1.0
```

The destination is the GitHub repository named by `origin` (SSH or HTTPS).
The uploader rechecks source identity, the complete asset set and checksums,
and resolves the remote tag, including annotated tags, to the recorded
commit. It creates a draft if none exists, or replaces matching assets in
an existing draft and updates its title and notes. The draft's notes are
the bundle's `notes.md` (what is above its `### All changes`, then the
link to the rest) with each paragraph and list item on one line
(GitHub breaks a release's notes at each line's end, where the changelog
wraps its lines; fenced code keeps its own) and each relative link made
absolute (`https://github.com/<repo>/blob/<tag>/<path>`, `tree/` for a
folder, `raw/` for an image), so they lead to the files from the
release's page. `notes.md` itself keeps the changelog's lines and links.
Unexpected assets already
in the draft require manual review. Published or immutable releases are
refused. The command never publishes a release or changes a tag.
After uploading it compares GitHub's stored asset names, sizes and SHA256
digests with the local files; a mismatch fails and leaves the draft unpublished.

Review the draft's notes and assets on GitHub, then publish it there.
Upload replacement is not atomic: an interrupted upload can leave a partial
draft. Rerun the command to finish it. Keep the draft unpublished while the
upload runs, and use one release operator at a time. Local commands share a
lock; GitHub does not offer an atomic draft-update transaction.

This workstation must be available to build a release. Keep the complete
bundle until publication is verified; GitHub cannot reproduce it from the
manifest alone. Publishing the AUR's PKGBUILD and `.SRCINFO` is a separate
step described below.
macOS and Windows release packages are not included in this Linux command.
A Windows build to try is made apart (ADR-074): `just package-windows`
builds it with cargo-zigbuild into `target/package/windows` (a zip and an
installer, each with its `.sha256`) and tries both under Wine. It is not
part of the release bundle, and nothing uploads it. A macOS build to try
(ADR-075) is made the same way: `just macos-sdk` once (Apple's licence is
yours to accept), then `just package-macos`, into `target/package/macos`,
looked over but never run.

## Publish on the AUR

The Linux Nix shell includes `pacman` (for `makepkg`), Git, OpenSSH and
curl. Arch package builds still run in the Arch root through
`just package-try arch`; the host's pacman never installs packages.
The shell sets `MAKEPKG_CONF` to Nix's installed configuration, since
Nixpkgs' makepkg otherwise looks in `/etc`.

Create an AUR account and register your SSH public key in its settings.
The account must own or co-maintain the `skwir`, `skwir-bin` and
`skwir-git` package bases; pushing to an unused package base creates it.
Configure SSH for `aur@aur.archlinux.org` and verify the server's host
key normally. Split packages share their base's repository on the AUR:
`skwir.git` holds `skwir`, `skwir-mermaid` and `skwir-mupdf`, `skwir-git.git`
their `-git` twins.

Each base has its PKGBUILD in `tools/package/aur`: `skwir` builds the
release's source, `skwir-bin` installs its portable tarball in `/opt/skwir`
(ADR-052), and `skwir-git` builds `main`. After building the release bundle,
pushing its exact source tag and publishing the GitHub release:

```sh
just aur-prepare v0.1.0
git -C target/aur-publish/skwir diff --cached
git -C target/aur-publish/skwir-bin diff --cached
git -C target/aur-publish/skwir-git diff --cached
just aur-publish v0.1.0
```

Preparation verifies the local release bundle and GitHub's tag, then
makes each base's files: `skwir`'s source archive is downloaded over HTTPS
and its SHA256 checksum filled; `skwir-bin`'s tarball is downloaded too and
must be the bundle's own (so the GitHub release must be published);
`skwir-git` is named after the release's commit (`0.1.0.r0.g34a7b9b`) and
its Git source stays unsummed. `.SRCINFO` comes from `makepkg`. Only when
all three are made does it write them, each in a separate AUR checkout it
clones or fast-forwards (`target/aur-publish/<base>`), staging only
`PKGBUILD` and `.SRCINFO`. The source repository's templates keep their
placeholders, so the tagged source stays clean. `just package-aur` writes
each `.SRCINFO` beside its template; `release-tools-check` checks they are
current and that `skwir-git`'s build and package functions are the release
PKGBUILD's.

Review the staged diffs, including the source URLs, checksums,
dependencies and split packages. The checkouts use your normal Git author
settings; configure `user.name` and `user.email` in each if the global ones
are not the maintainer's. Publishing rechecks the release bundle, the
prepared files and each checkout's history, commits them all, then pushes
each `master` over SSH. It refuses altered files, unrelated changes and a
redirected origin. It never force-pushes. If authentication or the network
fails, rerun `aur-publish` to push what is left. Concurrent AUR updates
reject the push; reconcile them in the checkout before preparing again. A
dirty checkout is never overwritten.

## GitHub fallback

`full checks` (`.github/workflows/full.yml`) runs the former full check matrix,
native macOS/Windows jobs and package builds, only through **Run workflow**.
Native jobs retain their existing nonblocking Phase 5b status. `packages`
can also run on its own; it builds and install-tests Linux packages and
checks the AppImage's and the tarball's portability. Both upload
workflow artifacts, not GitHub release assets. A tag push only checks
release metadata; it starts no compilation and creates no release.

After pushing the workflow changes, review any repository branch protection
that requires the old `lint`, `test`, `window` or `readers` jobs. Routine CI's
new required job is `essentials`. Existing Actions runs are not cancelled by
editing workflow files locally.
