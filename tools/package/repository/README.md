# Skwir's apt and dnf repositories (ADR-142)

- `version`: the version of `skwir-repository`, the package that adds the
  repositories and their key. Raise it when what it holds changes (a key
  extended, a new key, another address, the text of its source, which
  `tools/package/repository.py` writes): the repositories then offer it as
  an update. Until then it is made again the same, byte for byte, dated
  by this file's last commit; `just repositories` refuses a version the
  site already holds made again with other bytes, and this file or a key
  uncommitted.
- `skwir.asc`: the repositories' public key, once made (`docs/RELEASING.md`,
  Repositories), which signs the indexes and each `.rpm`: Fedora's
  packages, which `release-build` signs, and `skwir-repository`'s. Its
  secret part never enters the repository: it stays in the keyring of
  whoever publishes a release.
- `next.asc`, only while the key changes (`docs/RELEASING.md`, A new key):
  the key coming, which `skwir-repository` carries beside the one that
  signs until it takes that one's place.
- `retired`, once a key has been changed (`docs/RELEASING.md`, A new key):
  the fingerprints of the keys `skwir-repository` carried before, one a
  line, which rpm and dnf let go of on Fedora.
- `keys.sh` and `posttrans.sh`: what `skwir-repository`'s `.rpm` carries
  as `/usr/libexec/skwir-repository/keys` and starts from its
  `%posttrans`, once the transaction is done: rpm keeps a key as dnf first
  imported it, so an extension and a key retired reach rpm (and dnf's keys
  for the index) only through it (`docs/RELEASING.md`, What rpm keeps of a
  key).
- `published`: `skwir-repository` 1 as the site published it on
  2026-11-20 (unsigned, its dnf repository `gpgcheck=0`), from which
  every system that added the repositories then updates:
  `repository-check.sh` starts its roots from it. Never made again.
- A key's expiry is its own (`gpg --show-keys skwir.asc`):
  `tools/package/repository.py` says it at each release and refuses either
  key within 180 days of it. Extend it before then, export it again and
  raise `version` (`docs/RELEASING.md`, Its expiry): an expired key is
  refused by every system, the update that would bring its extension
  included.

`tools/package/repository.py` makes the repositories' indexes into the
website's static folder (`just repositories <tag>`);
`tools/package/repository-check.sh` tries them in fresh Debian, Ubuntu and
Fedora roots with throwaway keys (`just repository-check`).
