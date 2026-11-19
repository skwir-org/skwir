# ADR-142: A new version is told where nothing else tells it

Status: accepted 2026-11-19, on the user's answers. They chose: ask
once; a static file on the website; AppImage's update information and
winget; apt and dnf repositories added by a command from the website,
never by a package or by Skwir; no Copr and no OBS; a check once a day;
a mark for an important release; What's new after an update, with a
setting that turns it off. Built 2026-11-19 on Linux (the plugin and the
platform's parts compiled for Windows and macOS), amended as built
(Amendments). Amends ADR-020 (a second plugin makes requests, and HTTP
moves to `skwir-platform`) and ADR-040 (signed repositories for the
`.deb` and the `.rpm`).

## Context

- **Releases come often.** There were five in two weeks: 0.3.0 on
  2026-11-03, 0.3.1, 0.4.0, 0.5.0, and 0.5.1 on 2026-11-16.
- **Who updates a copy depends on where it came from.**
  - *A package manager*: the AUR's three bases (ADR-040, 7), Nix's
    package (ADR-124), one day a distribution's own. It tells the user
    and installs. Skwir saying so as well would be noise the user cannot
    act on, and a distribution patches such checks out.
  - *Nothing*: the AppImage and its tarball (ADR-052), the Windows
    installer and zip (ADR-074), the Mac app (ADR-075), and a `.deb` or
    `.rpm` downloaded from the release page (ADR-040). dpkg or rpm
    installs the last two in `/usr/bin` as a managed package would be,
    but no repository updates them.
  - Where a copy is installed does not tell which it is. `skwir-bin`
    installs the release's tarball as it is, so not even its files
    differ.
- **Tools that update a downloaded copy exist.**
  - AppImage's update information: a line in the runtime's `.upd_info`
    section (appimagetool's `-u`). AppImageUpdate, Gear Lever and
    AppImageLauncher read it and fetch the new image by zsync, only the
    blocks that changed.
  - winget, for the installer. The installer keeps its `UpgradeCode`
    and `MajorUpgrade` (`tools/package/windows/skwir.wxs`), so a new one
    replaces the copy in place. winget also offers to upgrade a copy
    installed by hand when it matches its entry in Installed apps (read,
    not tried).
  - apt and dnf, for a repository, and GNOME Software and Discover
    through PackageKit. With these, a new version reaches the user as
    any system update does.
- **Copr and OBS were weighed (2026-11-19) and not taken.**
  - OBS's builders have no network. Debian's and Ubuntu's Rust (1.85 in
    Debian 13) is older than `rust-version` (1.97), so a toolchain
    would have to be packaged there too, and a `debian/` packaging kept
    beside cargo-deb.
  - Copr could build Fedora's package from a spec, kept beside
    cargo-generate-rpm's metadata, with the network turned on. It would
    publish a package `try.sh` never tried (ADR-053).
- **An install can ask nothing.** An RPM's scripts run without a
  terminal. A `.deb` can ask through debconf, but GNOME Software installs
  without showing debconf's questions, so the default answers in the
  user's place. An opt-in at install time is either invisible or an
  opt-out. A repository added silently by the package was judged
  invasive (the user, 2026-11-19).
- **The website** is skwir.org, `skwir-site` on Cloudflare Pages (its
  `wrangler.jsonc`).
  - Its static files are at most 25 MiB each and are served free and
    without limit. A `.deb` was 49 MB and the `.rpm` 57 MB in 0.5.1.
  - Its `static/_redirects` takes placeholders. It already gives stable
    addresses on skwir.org for what lives with the code's host
    (`/source`, `/issues`), with 302 so that a move takes effect at
    once. About names those addresses, never the host.
- **The network.** ADR-020 made `map-tiles` the one plugin that makes
  requests, with HTTP private to it until a second plugin needs HTTP;
  then the client moves to `skwir-platform` as a utility. A new use of
  the network needs an ADR of its own (`AGENTS.md`, "Network").

## Decision

1. **A copy says how it came.** Each of Skwir's packaging scripts
   writes one word in `share/skwir/channel`, where data a package
   carries beside its programs lives (above `bin` on Linux, beside the
   program on Windows, `Resources/share` on a Mac; found as the fonts
   are). The words are `appimage`, `tarball`, `windows-installer`,
   `windows-zip`, `macos`, `deb` and `rpm`.
   - The AppImage's tree and its tarball each get their own word.
   - Without the file, or with a word that is not one of these, the
     copy never checks and never asks. That covers a build from source:
     cargo, the AUR's `skwir` and `skwir-git`, Nix's, a distribution's.
   - `skwir-bin`'s PKGBUILD removes the tarball's file. Another package
     manager that repackages a release removes it too, and
     `docs/RELEASING.md` says so for packagers.
2. **What the system updates is asked of the system.** A `deb` or `rpm`
   copy on a system that has Skwir's repository never checks. The test
   is whether the file `skwir-repository` installs is there
   (`/etc/apt/sources.list.d/skwir.sources`,
   `/etc/yum.repos.d/skwir.repo`), read on a worker when the plugin starts and at each day's check. A
   copy with a channel and no such repository is a *downloaded* copy.
3. **What a tool can update is handed to it.**
   1. *AppImage* (ADR-052). The update information is
      `zsync|https://skwir.org/download/skwir-x86_64.AppImage.zsync`,
      written into the runtime when the image is made. The `.zsync`
      file is made beside the image and uploaded with the release under
      that name, without a version. The site sends `/download/:file`
      to the latest release's file of that name (302), so a move of the
      code's host changes the site, never an image already installed.
      Whether each updater follows the redirect is to be tried.
   2. *winget* (ADR-074). A manifest for each release in
      `microsoft/winget-pkgs` names the release's installer. The user
      submits it, as with the AUR's bases (`docs/RELEASING.md`). The zip
      is not offered there.
   3. *apt and dnf* (ADR-040, amended). The repositories are signed and
      live at `skwir.org/apt` and `skwir.org/rpm`.
      - Their indexes are the site's static files: `dists/trixie` and
        `dists/noble` with a signed `InRelease` (apt-ftparchive), and
        `repodata` with a signed `repomd.xml` (createrepo_c).
        ~~The `.rpm` files are signed with rpmsign as well.~~ (Amended:
        they are not; their index is, below.)
      - The packages stay the release's. One `_redirects` line for each
        format sends `pool/:tag/:file` to the release's file (302). The
        SHA-256 in the signed indexes checks what arrives, wherever it
        comes from.
      - What is published is exactly what `release-build` made and
        `try.sh` tried, never built again (ADR-053).
      - Each distribution release a package is built for is a suite of
        its own, as it is a build root of its own today. A Fedora
        release that changes a library a package links gets its own
        folder.
   4. *The key.* One OpenPGP key signs both repositories (the new one
      carried beside it while it changes, below). It has a passphrase
      and is kept off the site's host. Its public part is on the site
      and in `skwir-repository`.
   5. *`skwir-repository`.* A package of its own for each format,
      architecture-independent and a few KiB, holding the sources or
      repo file (with `Signed-By` for apt) and the key.
      - The site's download page gives the command that installs it:
        `sudo dnf install https://skwir.org/rpm/skwir-repository.rpm`,
        or the `.deb` downloaded and then `sudo apt install
        ./skwir-repository.deb`. (Amended: one a family,
        `rpm/fedora/own/skwir-repository.rpm` and
        `apt/own/skwir-repository-debian.deb` or `-ubuntu.deb`, below.)
      - Skwir's own packages never install it, and Skwir never runs
        anything with administrator rights.
      - Removing it removes the repository. A new key reaches users as
        an update of it (amended: two updates, the new key beside the
        old one, which signs, then the new alone, below).
   6. *Tried.* ~~`try.sh` installs from the repository, served locally,
      in a fresh root, and upgrades there from the release before.~~
      (Amended: `repository-check.sh` tries the repositories in fresh
      roots of its own, below; it downloads `skwir` from them without
      installing it, and no upgrade of `skwir` from the release before
      is tried yet.)
   7. No Copr and no OBS (Context).
4. **The `updates` plugin** checks only for a downloaded copy. It
   declares `network` (ADR-044) and is turned off like any plugin, which
   means no request, no notice and no question.
5. **Asked once.** A downloaded copy asks at the first look
   (`Plugin::on_looked_at`) of the first day at least two days after the
   copy was first seen.
   - The question is a toast, "Tell you when a new version of Skwir is
     out? Skwir asks skwir.org once a day; Cloudflare, which serves it,
     sees your IP address", with Yes and No.
   - A `deb` or `rpm` copy's toast also links to the download page's
     repository section ("Update with your system…"), which opens in the
     browser. The link answers nothing. Once the repository is there,
     decision 2 applies.
   - The toast stays until it is answered. Dismissed, or still
     unanswered when the window closes, it counts as No.
   - The answer is the setting `updates.tell`, written either way, so a
     copy that wrote it never asks again. The setting is in Settings
     under Updates, with the same words. Off is its default.
6. **The check.**
   - *When.* With `updates.tell` on, on a downloaded copy, at the first
     look of each calendar day (`skwir_platform::calendar`, ADR-017).
     It runs on a worker from `on_looked_at`, with no timer and nothing
     on the startup path.
   - *Failure.* A check that fails (offline, 10 s without an answer, an
     answer not understood) says nothing. It is tried again at a later
     look, at most once an hour, and only an answer counts as the day's
     check.
   - *Metered connections.* On a metered connection the check is put
     off to a later look: the portal's NetworkMonitor `GetMetered` on
     Linux, on the one session connection; the connection's cost on
     Windows; the path's `isExpensive` on macOS. Where this is not known,
     the check runs.
   - *What is asked.* `GET https://skwir.org/updates/1.json` (1 is the
     format), with `If-None-Match`. The User-Agent is `skwir` alone. No
     version, system, language, identifier or cookie is sent.
   - *The file.* `{"latest": "0.6.2", "important_since": "0.6.1"}`:
     `latest` is the newest release, and `important_since` the newest
     release that fixes a loss of data or a hole in security (optional).
     Each is `MAJOR.MINOR.PATCH`, and anything else refuses the whole
     file.
   - *Nothing else is read from it.* No text and no address is taken
     from the file. The link is Skwir's own,
     `https://skwir.org/releases/v<version>`, which the site sends to
     the release's page (302).
   - *Publishing the file.* A release writes the file when its page is
     published, never before its files are up (`docs/RELEASING.md`),
     with the repositories' indexes, in one deploy of the site.
   - *What is remembered.* A namespace of the cache
     (`skwir_core::cache`) holds the day of the last check, the ETag and
     the last answer, the day the copy was first seen, and the versions
     told and when. Deleting the cache costs at most a notice said
     again.
   - *HTTP.* ADR-020's client moves to `skwir-platform` as a utility,
     used by `map-tiles` and `updates`, with the system's certificates
     and no new dependency.
   - *Commands.* `updates.check` asks at once (Check now in Settings
     and About), refused while `updates.tell` is off. `updates.latest`
     answers what was last read, from memory. A request is made only on
     the user's call; an agent's call is answered from memory
     (`docs/invariants/commands.md`).
7. **How it is said.**
   - A version newer than the copy's own is told once, in a toast:
     "Skwir 0.6.2 is out", with What's new (its page, in the browser)
     and Not now.
   - The next toast waits 14 days after the last, whatever comes out
     meanwhile, and then tells the newest version.
   - A copy older than `important_since` is told at the first check
     that finds it, the 14 days ignored, in words that say why ("Skwir
     0.6.2 fixes an important problem"). It is still told once per
     version.
   - Until the copy is updated, About says which version is out, with
     the same link, and More has a row beside About.
   - Never a dialog, a desktop notification or a badge.
   - The words are the plugin's messages (ADR-139), translated. The
     file's only part in them is a version.
   - Skwir never installs. Nothing is downloaded or run: the release's
     page and the tools of decision 3 install.
8. **What's new after an update**, on every channel, with no network.
   - At the first look of a copy whose version is greater than every
     version this user has run (kept in the cache), a toast says
     "Skwir was updated to 0.6.2", with What's new.
   - Nothing is said for a first start, after the cache was deleted, or
     for a version older than one already run. Two copies of different
     versions used in turn say it once.
   - `updates.whats_new` turns it off. It is on by default.

## Consequences

- When built, `map-tiles` and `updates` both make requests.
  `AGENTS.md`'s "Network" rule, ADR-020 and `commands.md` name both.
- A release gains steps: the `.zsync` file, ~~rpmsign,~~ the
  repositories' indexes, the site's `updates/1.json` and its redirects
  (one deploy, after publishing), and a winget manifest (the user's).
  ~~`release-verify` checks that the indexes name the files uploaded.~~
  (Amended: `just repositories` refuses a bundle whose packages are not
  those its manifest records, as `release-verify` refuses it; GitHub's
  copies are compared with the manifest when `release-upload` uploads
  them, not again when the indexes are made.)
- The repositories' key is a new secret. If it is lost, users install a
  new `skwir-repository` by hand.
- Cloudflare sees the IP address of each check and of each
  `apt update`/`dnf` refresh. GitHub serves the packages. The question,
  the setting and the download page say so.
- A copy released before this ADR is built never checks. The first
  release with it is the first that can tell.
- Each supported distribution release remains a build root, and now
  also a suite.
- Steps, each tried before the next: (1) the channel file and What's
  new; (2) HTTP moved to `skwir-platform`, the plugin, the question, the
  check and the toasts, tried against a local server with a throwaway
  authority (as `map-tiles` does); (3) the site's file and redirects;
  (4) AppImage's update information, tried with Gear Lever and
  AppImageUpdate; (5) winget; (6) the repositories, `skwir-repository`
  and ~~`try.sh`'s upgrade~~ (amended: `repository-check.sh`, below).

## Amendments

### 2026-11-19: as built

- **What is kept is in the state file**, `updates.kept` (ADR-056), as
  the drafts told are (`ui.drafts_told`), not the cache: the settings'
  store reads and writes it, and a worker saves it. Lost, it costs what
  decision 6 says.
- **The question** (5) is a notification with several buttons that stays
  until answered or closed (`App::ask_quietly`; a `Notification` holds
  `actions` and whether it `stays`). Only Yes, No or the setting answer
  it (`updates.answer`, a person's alone, `Effect::UserOnly`): closed, or
  replaced by another toast, it is asked again another day, on three days
  at most. Skwir asks nothing while the user has not seen it. A
  downloaded copy alone declares `updates.tell`.
- **The commands** (6): `updates.check` asks the site at once, an action
  an agent asks the user first for (`Effect::Acts`); the palette offers
  it, and no Check now button was added to Settings or About.
  `updates.latest` answers from memory, how the copy came included.
- **About and More** draw what a plugin keeps said about Skwir
  (`registry::News`): the window names no plugin.
- **Metered connections** (6) are read on Linux alone, from the portal.
  Windows' connection cost and macOS's `isExpensive` are not read yet:
  the check runs there.
- **The AppImage** (3.1): the update information names
  `skwir.org/latest/skwir-x86_64.AppImage.zsync` (`/download/` is the
  site's download page). `zsyncmake` makes the zsync file, which names
  the image at `skwir.org/files/v<version>/<name>`; appimagetool's own,
  relative to wherever a redirect led, is removed. The release expects
  the file (`tools/release/release.py`), and the AppImage's check reads
  the update information and the zsync file back.
- **The site** makes `/updates/1.json` from the release it describes
  (`release-manifest.json`), the important release named by hand in its
  `site.ts` (`importantSince`): it never names a release the site does
  not offer. Its redirects: `/releases/:tag`, `/latest/:file`,
  `/files/:tag/:file`, `/apt/pool/:tag/:file`,
  `/rpm/fedora/pool/:tag/:file`. Its download page shows the
  repositories once the first is published (`REPOSITORY_PUBLISHED`).
- **The repositories** (3.3 to 3.6): apt's suites `trixie` (Debian 13)
  and `noble` (Ubuntu 24.04), dnf's `fedora`; `skwir-repository` one
  package a family (`apt/own/skwir-repository-debian.deb`, `-ubuntu.deb`,
  `rpm/fedora/own/skwir-repository.rpm`), made by nfpm at the version
  `tools/package/repository/version` says, dated by that folder's last
  commit: the same file until its version is raised. Its apt source is a
  plain file: a conffile would stay once the package is removed (dpkg
  takes one only on purge), its key gone. dnf reads with
  `repo_gpgcheck=1` and `gpgcheck=0`: the index is signed and names each
  package's checksum, and a package signed after the release checked it
  would no longer be the release's. `just repositories <tag>` makes them
  (`tools/package/repository.py`), both before writing either into the
  site, from a bundle whose packages are those its manifest records.
  - The indexes are signed by one key, `skwir.asc`'s: Fedora 44's dnf
    refuses a signature beside another (tried). A new key (3.5) takes
    two versions of `skwir-repository`: the new key (`next.asc`)
    carried beside the old, which signs, then, after a grace period,
    the new alone, signing (`docs/RELEASING.md`). A key that expires
    within 180 days is refused: it is extended, and the extension
    brought by an update, while every system can still read what it
    signs.
  - `just repository-check` tries them in fresh roots with throwaway
    keys, against a stand-in for the site that sends each package to
    another host's name: the index a stranger's key signed refused, each
    upgrade of a change of key, and `skwir-repository` removed with its
    source and key, the system's own sources read after.
- **winget**: `just winget <tag>` writes the manifests
  (`tools/package/winget.py`, identifier `Skwir.Skwir`, schema 1.10.0),
  the installer matched by its `UpgradeCode`. winget's own validation was
  not run (no Windows here).
- **HTTP** is `skwir_platform::http`, behind the platform's `http`
  feature that only the two plugins enable, a client limited per use
  (`Http::limited`: the check waits 10 s and reads 16 KiB at most).
