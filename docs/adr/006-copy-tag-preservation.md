# ADR-006: Copy tag preservation

Status: accepted, 2026-09-19

## Decision

`core.preserve_tags_on_copy` defaults to true. Copy is a core operation shared
by the built-in command, clipboard, shelf and external drops; all construct
it through `Copy::from_settings`. The serialized operation records the choice,
so applying a plan or redoing it never consults a changed setting. Older plans
without the field use the default. The tags provider and copier share only the
`user.xdg.tags` storage key in core metadata.

Preservation copies the raw tag attribute for regular files and directories,
recursively, before restoring source permissions. Symlinks stay symlinks; their
targets are not followed. Other xattrs, comments and ACLs are outside this option.

Missing tags and a source filesystem without xattrs require no tag write. Other
read errors or destination tag-write failures fail the copy with an actionable
error. A copy is staged on the destination filesystem and published with a
no-replace rename only after content and metadata succeed. Failure cleanup may
relax permissions only inside the newly created staging tree. Untagged copying
remains available by disabling the preference. Move semantics are unchanged.

## Consequences

Copying tagged data to a filesystem without user xattrs requires an explicit
opt-out; there is no silent metadata loss or sidecar fallback. No filesystem
allowlist is maintained: actual xattr operations determine support. The staged
copy avoids presenting a partial result as an already-applied operation.
