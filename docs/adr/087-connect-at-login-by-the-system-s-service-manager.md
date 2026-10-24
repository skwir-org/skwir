# ADR-087: Connect at login by the system's service manager

Status: accepted, 2026-10-24. Builds on ADR-077 (a remote folder is a
mount an installed tool makes) and ADR-086; keeps ADR-000's third
principle (no daemon).

## Context

A remote mounted by Skwir lasts until the session ends or it is let go
of. Users asked for a remote that is there when they log in, before
Skwir starts, as a drive is. Skwir has no daemon and keeps none: it would
need one to mount at login, or the system's own service manager to do it.
The user chose the latter for the night of 2026-10-24 ("it's fine since
it's not a skwir daemon, but systemd"), and asked that its tests stay out
of their own manager.

## Decision

1. **The system's service manager mounts, never Skwir.** Connect at
   Login (a remote's menu, `remote.connect_at_login`) writes an entry for
   the user's own manager: a systemd user unit on Linux
   (`$XDG_CONFIG_HOME/systemd/user/skwir-remote-….service`, wanted by
   `default.target`), a launch agent on macOS (`~/Library/LaunchAgents`),
   a Task Scheduler logon task on Windows. Turned off, the entry is
   removed and its manager told; nothing else is kept.
2. **It runs Skwir's own mount command, in the foreground, at Skwir's
   own mount point** (`login::AtLogin`: rclone's `mount` as the connector
   runs it, telling systemd once mounted; sshfs with `-f`): Skwir knows the
   mount as its own, and Eject lets go of it as of any other. What a run
   that died left (a mount whose program is gone, its sockets) is let go
   of before the next and after it ends.
3. **What would need an answer at login is refused**: nobody is there to
   give it. Only a remote of rclone's configuration that is not
   encrypted, and an SFTP server logged in to by key (tried once with
   ssh's `BatchMode`, as the unit's will), connect at login; a server
   needing a password does not. Key login is set up from Skwir for that
   (Set Up Key Login…, ADR-086).
4. **Connecting now too, or nothing.** Turned on, the entry connects now
   where nothing is mounted at its point; where its manager fails, the
   entry is taken back. Turned off, a mount it carries stays until Eject
   or the session's end.
5. **Tests never touch the user's manager.** A unit is read by
   `systemd-analyze verify` in folders of the test's own, its command run
   as the unit would run it against servers of the test's own, and its
   installing asked of a stand-in for `systemctl` given a runtime folder of
   the test's own. A private manager of the test's own is not run: systemd's
   user manager moves every process of the cgroup it starts in into its
   `init.scope` (tried once, from a terminal: it moved that terminal's
   processes), and a cgroup of its own is one the user's manager would
   have to delegate.

## Consequences

- A mount at login outlives Skwir and needs none of it, as a mount Skwir
  made does.
- No real systemd, launchd or Task Scheduler has run one of these
  entries here: Linux's checked as described, macOS's compiled and its
  agent's file tested, Windows' task's file tested (Wine's Task Scheduler
  does not start). On Windows rclone's remotes do not connect at login
  yet (a mount there answers on a loopback login Skwir keeps beside it).
- A remote set to connect at login and then removed from rclone's
  configuration leaves its entry failing until turned off.
