# Skwir's apt and dnf repositories (ADR-142)

- `version`: the version of `skwir-repository`, the package that adds the
  repositories and their key. Raise it when what it holds changes (a new
  key, another address): the repositories then offer it as an update.
- `skwir.asc`: the repositories' public key, once made (`docs/RELEASING.md`,
  Repositories). Its secret part never enters the repository: it stays in
  the keyring of whoever publishes a release.

`tools/package/repository.py` makes the repositories' indexes into the
website's static folder (`just repositories <tag>`);
`tools/package/repository-check.sh` tries them in fresh Debian, Ubuntu and
Fedora roots with a throwaway key (`just repository-check`).
