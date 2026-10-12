# ADR-055: The settings file is shared

Status: accepted, 2026-10-12. Decision 5 replaced by ADR-056.

## Context

- skwir keeps its preferences in `$XDG_CONFIG_HOME/skwir/settings.toml`,
  and writes there often: the settings dialog, and the window's layout
  and hints (`ui.session`, `ui.hints_shown`), state kept with the
  preferences.
- A save wrote the whole table skwir held, read once at start. Whatever
  changed the file meanwhile (another skwir, a hand edit) was written
  over at the next save, and never seen by a skwir already running.
- Home Manager declares a program's settings. A file it links from the
  Nix store is read-only, and skwir's next save would replace the link,
  after which Home Manager refuses to activate. Its newer modules offer
  `mutableSettings` instead: at each activation the declared values are
  merged into the writable file (`lib.hm.generators.mkImpureConfigMerger`:
  what is declared wins, the rest stays).

## Decision

1. **A save writes what this process changed, over the file as it
   stands.** `Settings` keeps the keys set, applied or renamed since its
   last save; a save reads the file, writes those keys over it (a key gone
   from the process's table is taken out), and takes in the rest. Never
   the whole table.
2. **What changes in the file is taken in as it changes.** The app
   watches the file's folder (`settings::follow`); a change is read
   on a worker (`Settings::reload`), under what this process changed and
   has not saved yet, and reaches its owners as a change made in the
   dialog does (`settings::changed`), a plugin turned on or off there
   included. A setting that needs a restart waits for one.
3. **A file that is no TOML is never written over.** The save fails and
   says why; nothing is taken in from it until it is fixed. (One found so
   at start is still set aside, as before.)
4. **Home Manager merges.** The flake's `homeManagerModules.default`
   (`programs.skwir`: `enable`, `package`, `settings`) merges `settings`
   into the file at each activation with Home Manager's own merger; a
   running skwir takes it in. It needs a Home Manager that has the
   merger, and says so otherwise. `checks.home-manager` runs the merge on
   a file as skwir writes it.
5. **No read-only settings file.** It would need the window's state out
   of `settings.toml` and the dialog to say a setting cannot change: not
   done. (Done by ADR-056.)

## Consequences

- Two skwir processes, or skwir and a hand edit, no longer undo each
  other: the last to change a key has it.
- A declared setting changed in the dialog lasts until the next
  activation, which sets the declared value again. Removing a
  declaration leaves the last value in the file.
- Moving the window's state to `$XDG_STATE_HOME/skwir/` would let the
  settings file be read-only, and Home Manager link it.
