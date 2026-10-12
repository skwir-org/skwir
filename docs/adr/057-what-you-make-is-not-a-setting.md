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
3. **Moved out of the settings.** One still in the settings file, where an
   earlier skwir kept it or a configuration declared it, is read until
   skwir keeps its own, and moves to the data file at the next save where
   the settings file can be written.
4. **Not declared.** `settings.check` says a key kept apart (the state or
   the data) is no setting, so Home Manager's build refuses one in
   `programs.skwir.settings`.

## Consequences

- With Home Manager's read-only settings, everything made in the window
  is kept: its state, its saved searches and tags. Only the dialog's
  changes are not.
- Saved searches and tag colours can no longer be declared in a
  configuration; they belong to the machine's user, as bookmarks do. A
  declared one in a settings file is taken once, then skwir's own wins.
- A third file to back up beside the settings: `data.toml`.
