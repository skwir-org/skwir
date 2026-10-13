# ADR-057: What you make in skwir is not a setting

Status: accepted, 2026-10-12. Revises ADR-056's consequence on saved
searches and tags.

## Context

- ADR-056 moved the window's state out of `settings.toml`, so the
  settings file may be read-only (Home Manager links it from the store).
  It kept saved searches, pinned tags, tags' colours and exclusive tag
  categories as settings, "a declarative configuration's to declare":
  with a read-only file, the command that changes them said the change
  was not kept.
- Those four are made by using skwir (Save Search, Pin, a tag's colour),
  never in the Settings dialog, which does not show them. A read-only
  settings file made them read-only too: a configuration choice cost the
  user what they make day to day.
- Other file managers keep such things with the user's data: GTK's
  bookmarks file, Dolphin's places (`user-places.xbel`).

## Decision

1. **They are the user's data.** A setting declared `SettingSpec::data`
   (`query.saved`, `tags.pinned`, `tags.colors`, `tags.exclusive`) is kept
   in `data.toml`, in skwir's data folder (`folders::data`:
   `$XDG_DATA_HOME/skwir` on Linux, the settings' own folder on macOS and
   Windows). Callers read and set them as before. A read-only settings
   file no longer stops them being saved.
2. **Kept as the settings are.** The file is the user's: one that is no
   TOML is set aside beside itself (`data.toml.unreadable-…`), said, never
   written over; a save writes the keys this process changed over the
   file as it stands; a change from outside (another skwir, `skwir cmd`)
   is taken in as it happens (`settings::follow` watches its folder too).
3. **Read under the data file.** One still in the settings file (written
   by hand) is read until skwir keeps its own, which wins whole: the data
   file's value replaces the settings file's, never merged with it key by
   key, so a tag's colour taken away does not come back from a stale copy.
   The settings file is not written for it (it may be read-only, and is
   shared). (2026-10-13: it no longer moves to the data file at the next
   save, which only carried an earlier skwir's; no one used skwir yet. The
   same day, a table merged key by key with that copy at each start and
   save was found and made whole.)
4. **Not declared.** `settings.check` says a key kept apart (the state or
   the data) is no setting. (2026-10-13: in a note, read only until skwir
   keeps its own, no longer a problem: one a settings file still held was
   flagged at every check, though harmless once skwir kept its own.)

## Consequences

- With Home Manager's read-only settings, everything made in the window
  is kept: its state, its saved searches and tags. Only the dialog's
  changes are not.
- Saved searches and tag colours can no longer be declared in a
  configuration; they belong to the machine's user, as bookmarks do. One
  written in a settings file by hand is read until skwir keeps its own,
  which wins.
- A third file to back up beside the settings: `data.toml`.
