# Skwir's apt and dnf repositories (ADR-142)

- `version`: the version of `skwir-repository`, the package that adds the
  repositories and their key. Raise it when what it holds changes (a key
  extended, a new key, another address, the text of its source): the
  repositories then offer it as an update. Until then it is made again
  the same, byte for byte, dated by this folder's last commit.
- `skwir.asc`: the repositories' public key, once made (`docs/RELEASING.md`,
  Repositories), which signs the indexes. Its secret part never enters the
  repository: it stays in the keyring of whoever publishes a release.
- `next.asc`, only while the key changes (`docs/RELEASING.md`, A new key):
  the key coming, which `skwir-repository` carries beside the one that
  signs until it takes that one's place.
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
