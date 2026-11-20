# ADR-142: A new version is told where nothing else tells it

Status: accepted 2026-11-19, on the user's answers. They chose: ask
once; a static file on the website; AppImage's update information and
winget; apt and dnf repositories added by a command from the website,
never by a package or by Skwir; no Copr and no OBS; a check once a day;
a mark for an important release; What's new after an update, with a
setting that turns it off. Built 2026-11-19 on Linux (the plugin and the
platform's parts compiled for Windows and macOS), amended as built
(Amendments); amended 2026-11-20, on the user's choice: each `.rpm` is
signed, and dnf checks it. Amends ADR-020 (a second plugin makes requests, and HTTP
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
        The `.rpm` files are signed with rpmsign as well. (Amended as
        built: they were not, their index alone was; amended again
        2026-11-20: they are, by the key that signs the indexes.)
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
- A release gains steps: the `.zsync` file, rpmsign (struck as built,
  back 2026-11-20), the
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
  `actions`, each saying whether it answers, and whether it `stays`).
  Only Yes, No or the setting answer it (`updates.answer`, a person's
  alone, `Effect::UserOnly`); the link leaves it up
  (`NotifyAction::answers` false). A toast said meanwhile is drawn over
  it, and it comes back once that one goes (What's new said on the same
  look included): closed, it is asked again another day, on three days
  at most. Skwir asks nothing while the user has not seen it. A
  downloaded copy alone declares `updates.tell`.
- **The commands** (6): `updates.check` asks the site at once, an action
  an agent asks the user first for (`Effect::Acts`); the palette offers
  it, and no Check now button was added to Settings or About.
  `updates.latest` answers from memory, how the copy came included.
- **About and More** draw what a plugin keeps said about Skwir
  (`registry::News`): the window names no plugin.
- **Metered connections** (6) are read on Linux alone, from the portal,
  on a connection of its own given up on after 3 s
  (`network::metered`), not the one session connection: a portal that
  hangs would hold a call there for good (`AGENTS.md`). Windows'
  connection cost and macOS's `isExpensive` are not read yet: the check
  runs there. Only a check asked for now (`updates.check`) is made on a
  metered connection; it always answers, with the newer version (told
  before or within the 14 days), up to date, or why it could not tell.
  The question's Yes waits as the daily check does, and says the version
  it finds in place of its confirmation.
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
  `tools/package/repository/version` says, dated by that file's last
  commit (amended 2026-11-20: not the folder's, which holds its README,
  nor the shell's `SOURCE_DATE_EPOCH`, nixpkgs' 1980): the same file
  until its version is raised, and `just repositories` refuses a version
  the site holds made again with other bytes. Its apt source is a
  plain file: a conffile would stay once the package is removed (dpkg
  takes one only on purge), its key gone. ~~dnf reads with
  `repo_gpgcheck=1` and `gpgcheck=0`: the index is signed and names each
  package's checksum, and a package signed after the release checked it
  would no longer be the release's.~~ (Amended 2026-11-20, below: each
  `.rpm` is signed before the release checks it, and dnf reads with
  `gpgcheck=1` too.) `just repositories <tag>` makes them
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
  (`Http::limited`: the check waits 10 s and reads 16 KiB at most). The
  deadline is the request's whole: a second try, and the wait for
  another request's connection to the server, included.
  - It asks without `If-None-Match`, and no ETag is kept (decision 6
    amended): a tag the server chose, sent back each day, could tell one
    copy from another, as a cookie would, and the file is a few dozen
    bytes.
  - It follows no redirect. The site serves `/updates/1.json` as a file,
    never a redirect (`docs/RELEASING.md`); any other answer than the
    file is a failure.
  - It goes through no proxy: neither the `https_proxy` variables nor
    the desktop's, Windows' or macOS's proxy settings are read. On a
    network that lets requests out only through a proxy, the check fails
    (quietly, as any) and map tiles do not load (`docs/PORTING.md`).
    hyper-util's `client::proxy` (the system's settings matched, a
    `CONNECT` tunnel) would carry it, with one crate more (`ipnet`) and,
    for Windows' and macOS's settings, theirs.

### 2026-11-20: each `.rpm` signed

The review of 2026-11-20 found that the repository's index alone being
signed is not enough for the software centres the Context counts on.
PackageKit, under GNOME Software and Discover, first installs only what
a key it trusts signed (`ONLY_TRUSTED`), and libdnf fails an unsigned
package under it whatever the repository's `gpgcheck`. GNOME Software
then asks whether to install unsigned software, and declines by itself
in its background downloads; Discover asks too. Read in their sources,
not tried. dnf in a terminal took Skwir's updates, and apt is untouched:
it trusts a package by its signed index. The user chose signing.

- **What signs**: the repositories' key, the one that signs the indexes
  (`tools/package/repository/skwir.asc`), its secret part in the
  keyring of whoever makes the release. `tools/package/rpm-sign.sh`
  signs (nixpkgs' rpmsign, rpm 4.20, a header signature) and reads each
  file back with `skwir.asc` alone, what systems hold.
- **When**: in `release-build`, which first checks the key as `just
  repositories` does (`repository.py --check-key`: its secret part
  signing here, `skwir.asc` alone reading what it signs, 180 days left)
  and refuses to start otherwise, as it refuses without macOS's SDK; gpg
  asks for the passphrase then. `try.sh fedora`, given `SKWIR_RPM_KEY`
  (the key's file), signs what the
  Fedora root built, on this machine, never in the root. The fresh root
  reads every package back with Fedora's own rpm and `skwir.asc` alone
  before installing it. What is checked, given its digests and uploaded
  is the signed file, so it stays the release's (ADR-053). gpg asks for
  the passphrase again when Fedora's packages are built, past the hour
  of checks, unless gpg-agent still holds it. A package built elsewhere
  (CI's fallback) is unsigned, and the repositories refuse it.
- **The repositories**: `just repositories` refuses a release whose
  `.rpm` the key that signs the indexes did not sign, before signing
  anything. `skwir-repository.rpm` is signed too, its signature dated by
  its version file's last commit (`--faked-system-time`, never before the
  key's making): EdDSA signs a message the same way each time, so the
  package is still made again identical. Its repository file reads
  `gpgcheck=1`, and `skwir-repository` 2 brings it. A system with 1
  updates to it, and to the release that comes with it, under the old
  `gpgcheck=0` and, through PackageKit, with the key 1 installed in
  `/etc/pki/rpm-gpg`, where libdnf reads keys. 0.5.1's packages,
  published unsigned with `skwir-repository` 1, are never listed again.
- **A change of key** (3.5) is unchanged: the packages are signed by the
  key that signs the indexes, the key coming carried beside it.
- **What rpm keeps of a key** (the review's PK-4, tried on Fedora 44:
  rpm 6.0.2, dnf5 5.4.8): rpm keeps a key as dnf first imported it.
  Extended, the key reached `/etc/pki/rpm-gpg` through `skwir-repository`
  and never rpm: once its first expiry had passed, a package signed then
  was refused ("Problem occurred when opening the package"; dnf imports
  a key only when rpm has none). A key no longer carried stayed trusted by
  rpm, and by dnf's keys for the index, kept with its cache. A scriptlet
  cannot change rpm's keys (the transaction's lock: "can't create
  transaction lock"). So `skwir-repository` 2's `.rpm` carries
  `/usr/libexec/skwir-repository/keys`, which its `%posttrans` starts
  apart and which waits for the transaction to end: each key it carries
  that rpm holds is imported again (rpm 6 takes the newer copy), and each
  key `tools/package/repository/retired` names is let go of, by rpm and
  in dnf's keys for the index. dnf's and PackageKit's own copies of those
  are removed where they are found, untried. The update every Fedora
  system takes from the published 1 (unsigned, `gpgcheck=0`) to 2 is
  tried from 1's own file, then `skwir` installed under 2's
  `gpgcheck=1`, its key imported.
- **Removing `skwir-repository`** takes its repository file and key
  file away. A key dnf imported into rpm's own keyring (`gpg-pubkey`)
  stays there, as for any repository whose packages dnf checked: `rpm -e
  gpg-pubkey-<id>` removes it. A package's scriptlet cannot, while the
  transaction holds rpm's database, and the helper above goes with the
  package.
- **Checked**: `just repository-check`, its packages signed again by
  each step's throwaway key. Fedora 44's rpm reads `skwir` downloaded
  through the repository as signed by the key `skwir-repository`
  brought. An update a stranger signed, listed by an index the key
  signs, is refused ("Transaction failed: Signature verification
  failed."). A release another key signed is refused by `just
  repositories`, and each step of a change of key upgrades. The release
  tools' tests: `release-build` stops before the checks without the
  key's secret part, and hands Fedora's build the key. Not tried: a
  Fedora build signed by `try.sh` (a full build in the root; the next
  `release-build` is the first), and PackageKit itself (GNOME Software
  or Discover updating Skwir on Fedora).

### 2026-11-20: the review's fixes

- **Nothing waits on the window's thread.** Yes and No are kept on a
  worker, the setting written and saved there (`Settings::set`'s
  contract). A look's work is a job of its own, outside the plugin
  host's lock (`Plugin::on_looked_at` starts work), and reads the state
  file only where it may decide what another skwir on it may have
  decided first: What's new, once a run; the question, or the day's
  check, on a day one may come. A copy something else updates does
  nothing more once looked at. The plugin's lock of what is kept is
  held over memory alone, never a file.
- **An answer that cannot be kept.** Where the settings file cannot be
  written (Home Manager's), nothing is asked: `updates.tell` is said
  where the rest is declared. An answer the settings could not keep (a
  file made read-only since, a full drive) is taken back and said ("Skwir
  could not keep your answer", and why), and is not counted as answered:
  the question may come back another day.
- **Where and while it is asked.** The question is asked, and counted,
  only at a look of a window of the user's own (`App::looked_at_own`),
  never at a look of another program's dialog Skwir answers, where it
  would wait unseen and go with a skwir started for that dialog.
  Answered in Settings or in another skwir, or made moot (the copy no
  longer downloaded), it is taken back (`App::take_back_question`), and
  its buttons, pressed before the window took it down, take it down
  and nothing more: nothing written, nothing asked of the site. A check
  reads again how a `.deb` or an `.rpm` came before it asks the site.
- **What it says is its own** (`App::notify_from`): turned off, what
  the plugin said goes with it, queued or shown.
- **The question's link names the system** (decision 5): it opens
  `/download?package=<id>#repository`, the site showing that system's
  card and scrolling to its commands. `fedora` for an `.rpm`; for a
  `.deb`, one word for Debian's and Ubuntu's builds whose repository
  packages differ, os-release says which (the platform's `OsRelease`,
  the reader About's system name uses, read where how the copy came
  is): `ubuntu` where `ID` is ubuntu or `ID_LIKE` names it
  (Linux Mint, Pop!_OS, elementary OS, Zorin OS), else `debian` where
  `ID` is debian or `ID_LIKE` names it (LMDE, MX Linux). Neither said,
  `?package=` is left out and the site guesses from the browser, which
  rarely names the distribution. `#repository` alone is what older pages
  and bookmarks read, and the site still takes it.
- **Two copies and the clock** (`Kept`). Of two checks on the same day,
  the merge keeps the answer naming the newer version, whichever merges
  which. What no build reads any more (an earlier build's `etag`) is
  dropped as it is read, never carried on with what a later build
  keeps. No lock is held between processes: what one writes between
  another's read of the state file and that one's save is taken back
  from the file, not from its memory, which writes it again at its next
  save of the key; at worst a notice is said once more. A day kept one
  after today (a clock behind for a moment, a boot before it is set) is
  read as today where deciding and never written back, so nothing is
  asked or checked twice once the clock is right; only a day further
  ahead is taken back to today.
- **All it says unasked waits for a window of the user's own**, as the
  question does: What's new, and the day's check with the version it
  would tell, are each said once, so a look at another program's dialog
  Skwir answers starts nothing; the next look at a window of the user's
  own says them.
- **`updates.answer`** is a downloaded copy's, as `updates.check` is,
  and run only by the question's buttons (`Declared::asked_only`): the
  palette and the Keyboard page offer it nowhere, as they offer no
  other command only its own place runs.
