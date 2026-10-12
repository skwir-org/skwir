# ADR-056: The window's state is not a setting

Status: accepted, 2026-10-12. Replaces decision 5 of ADR-055. Its
consequence on saved searches and tags revised by ADR-057.

## Context

- ADR-055 made the settings file shared, merged into by Home Manager at
  each activation. It left one door shut: a read-only settings file, as
  Home Manager links a program's settings from the Nix store by default.
- skwir wrote to `settings.toml` constantly for what is no preference:
  the window's layout (`ui.session`), its tabs and folder at close
  (`core.last_tabs`, `core.last_location`), the shortcut hints it showed
  (`ui.hints_shown`). A read-only file would have stopped all of it.
- Declared settings reached the file unchecked: a choice misspelt, or a
  key no setting has, was found only by its absence in skwir.

## Decision

1. **State has a file of its own.** A setting declared
   `SettingSpec::state` is kept in `state.toml`, in skwir's state folder
   (`folders::state`: `$XDG_STATE_HOME/skwir` on Linux, the settings'
   folder on macOS, the local application data on Windows). Callers read
   and set it as any setting. A save writes each file only when its table
   changes, the state first; a state key still in the settings file (an
   earlier skwir's) moves out where that file can be written. The state
   file is skwir's own: one that is no TOML is started over.
2. **The settings file may be read-only.** A save writes through a link,
   to where it leads (a settings file kept with the user's other
   configuration stays a link). One that cannot be written (Home
   Manager's link into the store) says the settings are read-only; the
   change is undone, the value what the file says; the state is saved
   still. The settings dialog says, above every page, that a change is not
   kept (`Settings::read_only`, read when it opens and at each wake).
3. **Settings are checked by skwir.** `settings.check` checks a file, or
   a table, against what this skwir declares: a value as the dialog checks
   what is typed (`SettingSpec::check`: its kind, choices, ranges, the
   owner's check); a key no setting has, under an owner that declares
   settings, is a problem; an owner nothing declares (a plugin turned off,
   or from outside) is noted. `skwir cmd` without a window declares the
   window's own settings too, so it checks them.
4. **Home Manager.** `programs.skwir.mutableSettings` (default false, as
   Home Manager's modules): off, the settings are linked read-only from
   the store, a writable file left as declared giving way to the link
   (`mkImpureConfigCleanup`); on, merged as ADR-055 says. Either way the
   skwir installed checks them while the configuration builds
   (`checkSettings`, default true), with no window: a wrong value fails
   the build, before anything is switched.

## Consequences

- A read-only settings file costs the dialog's changes alone: the
  window's layout, tabs and hints are kept as before.
- Saved searches, pinned tags, tag fields and colours stay settings, a
  declarative configuration's to declare: with a read-only file, the
  command that changes them says it is not kept. (Revised by ADR-057:
  they are the user's data, kept in a file of their own.)
- A plugin from outside is not loaded while the configuration builds: its
  keys are noted, never checked.
