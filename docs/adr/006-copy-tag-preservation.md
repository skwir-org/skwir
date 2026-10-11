# ADR-006: Copy tag preservation (now: user attributes)

Status: accepted, 2026-09-19. Extended 2026-09-30 to every `user.*`
attribute; amended by ADR-028 (attributes kept beside the file).

## Context

Tags live in an extended attribute (`user.xdg.tags`) that a plain copy
drops. Copies come from the command, the clipboard, the shelf and external
drops, and plans are serialized, applied later and redone: the choice to keep
tags must be made once and recorded. A copy that fails halfway must not look
applied.

## Decision

- **On by default.** `core.preserve_tags_on_copy` (now
  `core.preserve_attributes_on_copy`) defaults to true. Every copy path builds
  the operation through `Copy::from_settings`, which records the choice in the
  serialized operation: apply and redo never read a changed setting. Older
  plans without the field take the default. The tags provider and the copier
  share only the storage key.
- **What is copied.** The tag attribute of regular files and directories,
  recursively, before source permissions are restored. Symlinks stay
  symlinks, never followed.
- **Errors.** No tags, or a source filesystem without xattrs: nothing to
  write. Any other read error or a failed write at the destination fails the
  copy with an actionable error.
- **Staging.** The copy is staged on the destination filesystem and published
  by a no-replace rename only after content and metadata succeed. Cleanup may
  relax permissions only inside the new staging tree.
- **Cross-device moves** (copy, then delete) always keep tags, whatever the
  preference, through the same staged transfer: a move never loses metadata
  and never leaves a partial target while the source exists. (Amended
  2026-09-19: the first version left them untagged and unstaged.)
- Turning the preference off gives untagged copies.

## Consequences

- Copying tagged data to a filesystem without user xattrs needs an explicit
  opt-out: no silent metadata loss or sidecar fallback (until ADR-028, below).
- No filesystem allowlist: real xattr operations decide support.
- A partial copy never looks applied.

## Amendments

### 2026-09-30: every user attribute

The tags plugin also keeps a rating (`user.baloo.rating`) and a comment
(`user.xdg.comment`) and shows a download's origin (`user.xdg.origin.url`,
`user.xdg.referrer.url`); copies kept only the tags.

- A preserving copy keeps every `user.*` attribute, whoever wrote it, as
  `cp --preserve=xattr` does. Other namespaces (ACLs, security labels, file
  capabilities) are left to the destination.
- The rules above hold for all of them, cross-device moves included.
- The key is now `core.preserve_attributes_on_copy` ("Keep tags and
  attributes when copying"); `Settings::rename` carries the old choice. The
  operation's field is now `preserve_attributes`, and a plan serialized with
  `preserve_tags` reads as it, so older journals keep their meaning.

### 2026-09-30: attributes beside the file (ADR-028)

- Where the destination refuses attributes, a preserving copy keeps them in a
  Mac `._` file beside the copy (the tags plugin's companion rule, every
  `user.*` attribute named as a Mac names them) instead of failing.
- A copy from a `._` file onto a volume with attributes makes them attributes
  again.
- The copy still fails when nothing can keep them and the preference is on.
- Where the no-replace rename flag is refused (many FUSE and network
  filesystems), the copy checks the target, then renames.
