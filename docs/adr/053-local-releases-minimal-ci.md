# ADR-053: Local Linux releases and minimal routine CI

Status: accepted, 2026-10-11 (the user chose local releases after hosted
CI and repeated package builds took too long).

## Context

Every push ran four full check jobs, native builds and Linux packaging;
tagging repeated the package builds. The Nix workstation already has
prepared distribution roots and build caches. ADR-040 and ADR-052 make
conventional Linux packages possible there without a container daemon.

## Decision

1. Routine GitHub CI runs one cached Rust job: workspace formatting,
   core/plugin API lint and nextest tests, and release-tool integration
   tests. UI, native workers, cross builds, dependencies, window tests and
   performance remain required full checks, run locally through
   `just release-check`. The former full workflow and package workflow
   remain available manually. A tag push validates metadata only.
2. `just release-build <tag>` requires clean source at the tagged commit,
   matching stable versions and dated changelog/AppStream metadata. It
   runs the full gate, builds and checks AppImage, Debian, Fedora and Arch
   packages in their existing roots, and stages one bundle atomically in
   `target/releases/<tag>`. Split package names come from package metadata;
   unrelated, missing or duplicate assets fail. Existing bundles are not
   replaced. The bundle includes notes, commit/check records and checksums.
3. `just release-upload <tag>` verifies the bundle and the remote tag's
   commit, then creates or updates a GitHub draft through `gh`. It refuses
   published/immutable releases and unexpected remote assets, replaces
   matching draft assets, and never publishes or moves tags. Publishing
   remains the maintainer's final review. Draft uploads may be retried.

## Consequences

- Green routine CI does not prove the complete application passes. The full
  local gate is mandatory before releases; manual workflows provide an
  independent fallback. macOS/Windows runtime findings still belong to 5b.
- Workstation availability and preserving the local bundle become release
  responsibilities. The manifest records checks but is not a signature or
  hosted provenance attestation. Distribution inputs retain the existing
  packagers' caching and update policy; this does not claim reproducible bytes.
- Uploading a draft is not a transaction. Keep one operator and leave it
  unpublished while uploading; a failed upload leaves a reviewable draft
  that a retry can complete. No new application network capability is added.
- AUR publication, native installers and signing remain separate work.

The workflow and commands are described in [Releasing](../RELEASING.md).
