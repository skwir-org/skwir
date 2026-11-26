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
   names in `README.md`'s install commands. Then run `cargo update
   --workspace`, which writes the workspace's crates into `Cargo.lock` at
   that version and changes nothing else (the packages build `--locked`),
   and `just package-aur`, which makes each `.SRCINFO` again from its
   PKGBUILD; commit both. The user's documents
   (`README.md`, `docs/guide.md`, `docs/HELP-WANTED.md`) say no version
   in their prose; `release-metadata` stops at a package's file name of
   another version there, or at "Skwir 0.2" or "Version 0.2" naming
   another, and at a `Cargo.lock` or a `.SRCINFO` of another version.
2. Date that version's `CHANGELOG.md` heading, for example
   `## 0.1.0 (2026-10-11)`. AppStream must have the same date. The section
   supplies the GitHub release notes, so it must be the first: an
   `## Unreleased` section above it, which the tag would hold but its
   notes leave out, stops the release. Its links stay relative, as
   anywhere in the repository (`docs/HELP-WANTED.md`): on the release's
   page each leads to its file at the tag, and a link to no file of the
   source stops the release. The page also offers Windows' and macOS's
   builds to try (below): the section says they are to try, never run on
   either system. The page shows what is above the section's
   `### All changes` heading, if it has one, then **Full details**, a
   link to the section: there go the summary, a line for each highlight
   and what changed that a user of the last release would notice, and
   below it every change in full. A short section without the heading
   is shown whole. Only stable
   `v<major>.<minor>.<patch>` tags are currently supported.
3. Check the languages the release offers (ADR-139, 13):
   `just words --release`. `tools/words/offered` lists the languages
   Automatic speaks, reviewed or as a draft (`zh-Hans draft`); any other
   language a catalog is in is listed in Settings as incomplete, with how
   much of it is translated and reviewed, for whoever chooses it. `just
   words` adds a language there as a draft once 90% of all messages are
   translated in it and every message of what is on by default is (the
   program's own crates and each plugin on by default), takes its `draft`
   away once as much is reviewed (no `# draft` left above them), and
   counts each language's share into `crates/skwir/src/catalogs.rs`.
   Only a person takes a language out: between releases it stays offered
   whatever it lacks, said in English, but a release refuses one offered
   as reviewed (no `draft` after it) with less than 75% of its messages
   reviewed, and one offered as a draft short of the draft's bar (a draft
   needs no review). Draft its new messages (each marked
   `# draft`), review more of it, or take its line out of
   `tools/words/offered`, which lists it as incomplete again until it
   meets a bar anew; then run `just words` and commit both files.
   `lint-all` runs the same check, so `release-check` refuses it too.
4. After the last fix, run `just startup-order` and commit
   `tools/build/startup-order.txt`: what a start of skwir runs, then what
   a viewer's own program runs beside it (`skwir view`, ADR-151), which
   the packages lay out together (ADR-128). It traces real starts on displays
   of its own and needs a CPU with Intel PT. A list left stale makes the
   packages heavier, not wrong.
5. Run `just release-metadata v0.1.0`, review and commit the release source,
   then create its tag (`git tag -a v0.1.0 -m 'Release 0.1.0'`). Check out
   that commit. Builds and uploads require HEAD to equal the tag and the
   tree to be clean, including untracked files. Ignored build outputs do
   not count as source changes.

These commands do not commit, create tags or push source for you. Choose
the actual version and date; the examples do not declare 0.1.0 released.

## What the packages say in other languages

The program carries its own catalogs (ADR-139). What the system shows of
it before it runs is the packages':

- **The desktop entry and the AppStream metainfo** keep their English in
  `tools/package/linux/org.skwir.skwir.desktop` and
  `org.skwir.skwir.metainfo.xml`, valid as they are. Their translations
  are gettext's PO files in `tools/package/linux/po`: `skwir.pot`, what
  xgettext extracts (the entry's GenericName, Comment and Keywords; the
  metainfo's text by AppStream's own rules, carried in `po/its`; never the
  brand), and one `<language>.po` for each language `po/LINGUAS` names.
  Each Linux package merges them where it is made (`msgfmt --desktop`,
  `--xml`, through `tools/package/linux/l10n.sh merge`): `deb.sh` and
  `rpm.sh` into `target/l10n`, where `crates/skwir/Cargo.toml`'s assets
  take them; the AUR's PKGBUILDs in `build()`; the AppImage's `stage.py`,
  whose tree is also the tarball `skwir-bin` installs; the Nix package in
  `postInstall`. Each needs gettext 0.21 or later: Debian's and Fedora's
  build dependencies list it, Arch's base-devel holds it, the flake's
  shell and package have it. `tools/package/linux/check.sh`,
  `tools/package/nix/check.sh` and the AppImage's build say whether the
  installed files speak every language of `po/LINGUAS`.
- **After changing their English,** run `just package-l10n update`: it
  makes `skwir.pot` again and merges each PO with it, without guessing (a
  sentence changed loses its translation rather than ship an old one under
  new English). `just package-l10n` checks that `po/` is current and
  sound and merges into files that `desktop-file-validate` and
  `appstreamcli validate --no-net` accept, when they are on `PATH`.
- **To add a language,** make its PO from the template
  (`msginit --no-translator -l de -i tools/package/linux/po/skwir.pot -o
  tools/package/linux/po/de.po`), translate it, name it in `po/LINGUAS`,
  then run `just package-l10n`.
- **A model's draft** is marked fuzzy (`#, fuzzy`), as Weblate marks a
  string needing review. The packages ship drafts, as the program ships its
  drafted catalogs; whoever accepts one removes its mark. When Weblate
  takes these files, its msgmerge add-on is set without fuzzy matching,
  so that a fuzzy mark never means a guess there either.
- **macOS's bundle** declares English and each language the program's
  catalogs ship (`crates/skwir/src/catalogs.rs`), with an `.lproj` each
  (`tools/package/macos.sh`), so macOS's own menus and its per-app language
  follow; a new language needs nothing more. Each `.lproj` holds what
  `Info.plist` says to a person, the brand alone today.
- **Windows' installer** stays English.

## Build and inspect

Enter `nix develop` (or use the direnv shell). The macOS build needs
Apple's SDK on this machine: run `just macos-sdk` once (ADR-075; Apple's
licence is yours to accept); `release-build` stops at once without it.
Fedora's packages are signed by the repositories' key (ADR-142, amended
2026-11-20; below, "The repositories' key"). `release-build` first checks
it as `just repositories` will (`tools/package/repository.py
--check-key`), and stops at once too where it fails: one key in
`tools/package/repository/skwir.asc`, its secret part in GnuPG's keyring
and signing there (not a stub whose secret part is kept elsewhere), valid
for 180 days yet, and what it signs read back with `skwir.asc` alone, as
systems hold it. gpg asks for its passphrase then, and again once Fedora's
packages are built, past the hour of checks, unless gpg-agent still holds
it (by default for 10 minutes after its last use, gpg-agent's
`default-cache-ttl`): a prompt cancelled or a passphrase refused there
stops the run, and the next starts with the checks again. Then:

```sh
just release-build v0.1.0
just release-verify v0.1.0
```

`release-build` first runs `just release-check`: `lint-all` (including
Windows/macOS cross checks, and each crate's catalogs as its code says
them with the languages offered, `just words --release`), `deny`,
`notices-check` (the packages' notices
in `tools/package/notices` are current, ADR-063), `test-all`,
`test-windows` (the Windows build's tests under Wine, every crate's,
one at a time: ADR-053 amended), `smoke`,
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
| `just package-try fedora` | Fedora 44 build, each `.rpm` signed here by the repositories' key (`SKWIR_RPM_KEY` names its file, `tools/package/rpm-sign.sh`); in the fresh root, every package read back as signed by Fedora's rpm with `skwir.asc` alone, then the main package installed alone, then the optional packages |
| `just package-try arch` | Arch build; fresh installation of main package alone, then optional packages |
| `just package-windows` | Windows build to try (ADR-074), made with cargo-zigbuild: a zip and a per-user installer, tried under Wine (programs answering, the archive helper making and listing each format, the media helper playing H.264 and VP9 clips, installing and removing); GStreamer's own build is fetched and installed once into the build cache (ADR-076) |
| `just package-macos` | macOS build to try for Apple silicon (ADR-075): skwir.app, zipped and in a disk image, looked over (architecture, libraries, seal); nothing runs it here |

The Nix package is the flake's, built from the tagged tree by whoever
installs it, so `release-build` makes nothing of it. Run
`just package-nix-check` before tagging (ADR-124): it builds the `minimal`
and `default` variants from scratch (about two full builds), checks that
every helper answers, that no closure holds the Rust toolchain or a library
of a helper it leaves out, and prints each closure's size; `full` is
evaluated.

The final bundle is `target/releases/v0.1.0/`. It contains the lowercase
`skwir-0.1.0-x86_64.AppImage` and `skwir-0.1.0-linux-x86_64.tar.gz`, each
with its companion checksum, all current-version Debian, Ubuntu, RPM and
Arch split packages, Windows' `.msi` and `.zip` and macOS's `.dmg` and `.zip`
(`skwir-0.1.0-windows-x86_64.*`, `skwir-0.1.0-macos-arm64.*`), each with its
companion checksum, the AppImage's `skwir-x86_64.AppImage.zsync` (named
without a version, so its updaters find the latest by its name; it holds
the image's own checksums, and has no companion), `notes.md`,
`manifest.json` and `SHA256SUMS`. The
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
Windows' and macOS's builds in it are to try (ADR-074, ADR-075): made on
Linux, Windows' tried under Wine, macOS's looked over, neither ever run on
its system. `just package-windows` and `just package-macos` make them alone,
into `target/package/windows` and `target/package/macos`.

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
`skwir.git` holds `skwir`, `skwir-mermaid`, `skwir-math` and `skwir-mupdf`,
`skwir-git.git` their `-git` twins.

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

## Tell copies of the release (ADR-142)

A copy of Skwir downloaded from the website asks it once a day for
`/updates/1.json` (the `updates` plugin), once its user said yes; the
tools that update a copy read what the release and the site give them.

**Before the first release that carries the `updates` plugin**, once:
make the repositories' key (below), publish the repositories from the
bundle of the release before (`just repositories v0.5.1`), set
`REPOSITORY_PUBLISHED` and deploy the site. (Done for 0.5.1 on
2026-11-20, its packages unsigned, with `skwir-repository` 1. Since then
`just repositories` refuses a release whose `.rpm` the key did not sign,
and `skwir-repository` 2 checks each package.) A downloaded `.deb` or `.rpm`
copy's question links to the download page's repository section
(`/download?package=<id>#repository`, `<id>` the system's: `fedora`,
`debian` or `ubuntu`, or `/download#repository` where the copy cannot
tell; ADR-142's amendments), which the site shows only once that is set:
released first, the plugin would send its users to a page without it.

After the GitHub release is published:

- **The site.** Its `src/lib/data/release-manifest.json` is the bundle's
  `manifest.json` (as for its download page): `/updates/1.json` is made
  from it, so it names the release the site offers and no other. A
  release that fixes a loss of data or a hole in security sets
  `importantSince` in the site's `src/lib/data/site.ts` to its version: a
  copy older than it is told at once, not two weeks after the last
  notice. The next release that brings such a fix moves it; it is never
  taken back. `/updates/1.json` stays a file the site serves itself,
  never one `_redirects` sends on: a copy follows no redirect and takes
  one for a failed check, so it would never be told. Then, as the site's
  README says: `pnpm sync-docs` (the release's notes and the contributor
  docs on skwir.org/docs, from the tag), `pnpm sync-l10n` (the languages
  the release offers), and, for a feature release (0.6.0, not 0.5.1), the
  front page's notice (`releaseNews` in `site.ts`), whose build fails
  while it names the last series. The site has no draft state: a
  language the release offers, as a draft too (`zh-Hans draft` in
  `tools/words/offered`), goes live at its prefix (`/zh-hans`), with
  `hreflang`, wherever the site has its catalog; decide that before
  running `pnpm sync-l10n`.
- **The AppImage.** Nothing to do: the bundle's
  `skwir-x86_64.AppImage.zsync`, uploaded with the release, is what the
  image's update information names through `skwir.org/latest/`, and
  AppImageUpdate or Gear Lever reach it once the release is published.
- **The repositories.** `just repositories v0.1.0` writes the apt and dnf
  repositories' signed indexes and `skwir-repository` into the site's
  `static/` (`../skwir-site` by default); gpg asks for the key's
  passphrase. It refuses a release whose `.rpm` packages the key did not
  sign (`release-build` signs them). Review them (`git -C ../skwir-site status`), then commit and
  deploy the site once, with the version file: an index never names a
  package before the release that holds it is published.
- **winget.** `just winget v0.1.0` writes the release's three manifest
  files beside its bundle (`target/releases/v0.1.0-winget`). Submit them
  as a pull request to `microsoft/winget-pkgs` (by hand, `wingetcreate
  submit` or `komac submit`); its checks install the release's installer.
- **Homebrew's cask.** `just cask v0.1.0` writes `Casks/skwir.rb` into a
  checkout of `skwir-org/homebrew-tap` (`../homebrew-tap` by default, or
  `just cask v0.1.0 <folder>`): the release's version and its macOS zip's
  SHA-256, from the bundle's manifest. Once the release is published:
  bring the tap's branch holding the cask's workflow job (`cask/skwir`
  for the first, then the tap's `main`) up to date with the tap's main,
  write the cask on it, `git add Casks/skwir.rb`, review it (`git -C
  ../homebrew-tap diff --cached`), then commit and push. The tap's
  workflow installs the zip from the release's page, checks that it is
  still quarantined and that Homebrew's Caskroom holds it (what a copy
  reads to stay quiet about new versions, ADR-142), and uninstalls it. A
  copy of 0.6.1 or before does not read the Caskroom, and would be told
  of its own updates: the tap holds no cask of one.
- **skwird**, Skwir's index of names, is released apart, from its own
  repository and process (its RELEASING); a release of Skwir that needs a
  new skwird says so in its notes, linking that release's tag, and goes
  out the same day as it (0.7.0 with skwird 0.2.0: drives indexed,
  names in both forms answered from the index).

**The repositories' key, once.** Make it where releases are made, with a
passphrase, and keep its secret part off the site's host (a backup apart):
every system that added the repositories trusts it with what it installs.

```sh
gpg --quick-generate-key 'Skwir repositories' ed25519 sign 5y
gpg --armor --export <its fingerprint> > tools/package/repository/skwir.asc
```

Commit the public key, publish the first repositories, then set
`REPOSITORY_PUBLISHED` in the site's `src/lib/data/downloads.ts`, all
before the first release that carries the `updates` plugin (above): its
download page then shows how to add them. What `skwir-repository` holds
changing raises `tools/package/repository/version`, and its update
brings the change to every system that has the repositories; another
address only while the old one still answers. Until then it is made again
the same file, dated by that file's last commit: `just repositories`
refuses a version the site holds made again with other bytes (what it
holds changed: raise the version), and the version file or a key not
committed. `just repository-check`
tries the repositories in fresh Debian, Ubuntu and Fedora roots with
throwaway keys: each starts from `skwir-repository` 1 as the site
published it (`tools/package/repository/published`) and takes the update
every system will; an index a stranger's key signed refused, a change of
key followed and the old key let go of, an extension taken by Fedora's
rpm, `skwir-repository` removed with its source and its key. It needs
Debian's, Ubuntu's and Fedora's mirrors, and waits two minutes for a key
to expire.

**What rpm keeps of a key.** rpm keeps a key as dnf first imported it,
its expiry included, and keeps a key `skwir-repository` no longer
carries; dnf keeps the keys it read the index with (Fedora 44, tried). A
package's scriptlet cannot change rpm's keys while the transaction holds
its database, so `skwir-repository`'s `%posttrans` starts
`/usr/libexec/skwir-repository/keys` (`tools/package/repository/keys.sh`)
apart, which waits for the transaction to end: it imports again each key
the package carries that rpm holds (rpm takes the newer copy) and lets go
of each key `tools/package/repository/retired` names, in rpm and in dnf's
keys for the index. Run as root, it does the same by hand.

**Its expiry.** Once a key has expired, a system that trusts it refuses
everything it signed, the update that would extend it included. `just
repositories` says when each key expires and refuses one within 180 days
of it (a key with subkeys that sign, the last of them). Extend it before
then, and bring the extension to every system as an update:

```sh
gpg --quick-set-expire <its fingerprint> 5y
gpg --quick-set-expire <its fingerprint> 5y '*'   # where subkeys sign
gpg --armor --export <its fingerprint> > tools/package/repository/skwir.asc
```

(`next.asc` for the key coming, below), then raise
`tools/package/repository/version`, commit both and publish the
repositories (with the next release, or alone from the last one's bundle:
its packages are signed by the same key). The update brings it to apt's
keyring and to Fedora's key file, and its `%posttrans` to the copy rpm
keeps (above).

**A new key** reaches a system only through an index signed by a key it
already has, and dnf refuses an index signed by two, so a change takes two
versions of `skwir-repository` (a key lost cannot: every user then installs
a new `skwir-repository` by hand):

1. Make the new key as the first was and export it beside the old
   (`gpg --armor --export <new> > tools/package/repository/next.asc`),
   raise the version, commit and publish (with the next release, or alone
   from the last one's bundle). The old key still signs the indexes, and
   the update brings the new one to every system that has the old.
2. After a grace period of months, which lets the systems that update
   rarely take the first step, name the old key's fingerprint in
   `tools/package/repository/retired` (one a line, kept for good), put
   the new key in the old one's place (`git mv -f
   tools/package/repository/next.asc tools/package/repository/skwir.asc`:
   it replaces the old file), raise the version again and commit, before
   the tag of the release that
   publishes it. That release's `.rpm` files are signed by the new key,
   as the indexes are; a release made before is never published with it
   (`just repositories` refuses its packages, the old key's), so this
   step waits for a release. The new key signs the indexes, and the
   update takes the old one away: from apt's keyring and Fedora's key
   files, and through its `%posttrans` from rpm and dnf (above), so that
   a key that leaked is trusted no more. A Fedora system that removed
   `skwir-repository` before still holds it in rpm: `sudo rpmkeys --delete
   <its fingerprint>` (the release's notes say so).

`just repositories` signs with the key of `skwir.asc`, whose secret part
must be in the keyring, and carries the key of `next.asc` beside it while
there is one; each file holds one key. `release-build` signs Fedora's
packages with the key of `skwir.asc` too, so a release's `.rpm` files are
signed by the key that signs the indexes listing them: the old one until
step 2, then the new.

**Packagers.** A package manager that repackages a release (as the AUR's
`skwir-bin` repackages the tarball) removes `share/skwir/channel`: what it
updates is then never told of new versions by Skwir
(`tools/package/channel/README.md`). A `.deb` or an `.rpm` built from the
source by `deb.sh` or `rpm.sh` carries the word as the release's do (it is
`crates/skwir/Cargo.toml`'s asset): one built for a repository of its own
(a PPA, a Copr) leaves that asset's line out, or its copies ask about new
versions while that repository updates them.

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
