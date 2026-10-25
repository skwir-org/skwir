# ADR-088: A volume's passphrase kept in the keyring, or the system's own unlocking

Status: accepted, 2026-10-25, the user's choice ("Yes, seems good", then
"include them now" for macOS and Windows), after asking whether Skwir
could offer to remember an encrypted volume's passphrase as it does a
network password. Amends ADR-082's first decision ("only a server's
password") and the operations rule that the passphrase "is never stored".

## Context

- The places plugin lists a locked LUKS container from `lsblk` and
  unlocks it through udisks, the passphrase on its standard input; it was
  asked each time, by the shell's own prompt (`ui.unlock_prompt`).
- ADR-082 keeps a server's password in the desktop's keyring when the
  user asks, and reads the ones GVfs keeps. GVfs keeps a volume's
  passphrase too when Nautilus is told to remember it ("Forever", "Until
  you log out"), under its own schema `org.gnome.GVfs.Luks.Password`, by
  `gvfs-luks-uuid`, the container's UUID (`gvfsudisks2volume.c`, 1.60.3,
  read for this ADR); it clears its own once refused. KDE's device
  notifier keeps it in KWallet; a Mac's Finder offers the Keychain.
- A disk passphrase is not a server's password. The server still decides
  who gets in, and a stolen laptop opens nothing its session could not.
  A drive's encryption is for the drive lost: once its passphrase is in
  the keyring, the drive is as safe as the keyring, behind the login
  password and usually a far cheaper derivation than LUKS2's Argon2id; a
  thief with the laptop and the drive guesses the login password offline,
  and with auto-login the keyring may have none. The drive alone stays as
  safe as before.
- A drive that only claims a UUID learns nothing: cryptsetup derives the
  key on this machine, and a wrong passphrase only fails.
- Off Linux Skwir unlocked nothing: macOS's and Windows' places listed
  mounted volumes alone (`docs/PORTING.md`, section 6). Both systems ask
  for an encrypted drive's password themselves when it is plugged in, and
  remember it for every program.

## Decision

1. **Asked with Remember.** Opening a locked volume (`places.open_locked`,
   its place's `activate`) asks for its passphrase through the window
   (`App::ask_passphrase`), offering "Remember in the keyring", off. What
   it means is said under the switch once it is turned on ("Once
   remembered, anyone who can open your keyring can unlock this volume"),
   and to a screen reader as the switch's description. Offered only where
   the volume's UUID is known. A refused passphrase is asked again at once, saying so,
   three times at most.
2. **Kept in the desktop's keyring alone**, once it unlocked the volume:
   never in Skwir's files, logs or command lines. Keyed by the container's
   UUID (`places.passphrases`), labelled for the keyring's own window
   ("Skwir: Backup (encrypted volume)"). The plugin's data notes which
   volumes have one (`places.remembered`), their UUIDs alone.
3. **The keyring answers first**: Skwir's own, only where the data notes
   one (a volume never remembered never wakes a locked keyring), then
   (Linux) GVfs's, read only. Skwir's own refused and another serving
   without Remember, Skwir's is forgotten; GVfs's are never changed.
4. **The user's alone.** `places.open_locked` is `user_only`: an agent
   never opens a volume with a passphrase the keyring keeps. An agent's or
   a script's `places.unlock` takes a passphrase given, and keeps none.
5. **Never unasked.** A volume is unlocked only when the user opens it,
   never because it was plugged in.
6. **Forget Passphrase** on the volume's place, locked or mounted, where
   Skwir keeps one: it removes Skwir's own from the keyring.
7. **The shell's own passphrase prompt goes** (`ui.unlock_prompt`,
   `PromptKind::Passphrase`): the plugin asks through the window's secret
   question, which now carries its placeholder and a note.
8. **macOS: Skwir's own unlocking, the Keychain.** The volumes
   `diskutil apfs list -plist` says are `Locked` are listed (by
   `DeviceIdentifier`, `Name`, `APFSVolumeUUID`; one with a role, another
   macOS's system or data, offered hidden) and unlocked as on Linux: the
   passphrase a line on `diskutil apfs unlockVolume`'s standard input
   (`-stdinpassphrase`), which mounts it under `/Volumes`; "Passphrase
   incorrect" is a refusal; where it is mounted is `diskutil info
   -plist`'s `MountPoint`. Remember keeps it in the Keychain
   (`skwir_platform::secrets`). macOS asks itself when such a drive is
   plugged in, and keeps what its dialog remembers in the login keychain
   in a form nothing documents: not read. (Apple's developer support says
   `diskutil mount` of a locked volume authenticates through the keychain
   and asks the user if needed, which would be macOS's own dialog; not
   taken, as nothing here can try it.)
9. **Windows: the system's own dialog.** `manage-bde` and WMI's
   `Win32_EncryptableVolume` unlock only for an administrator; Explorer's
   Unlock Drive verb on a drive (`unlock-bde`, which runs `bdeunlock.exe`
   as the user) shows Windows' own dialog, which offers to unlock it
   automatically on this PC from then on, BitLocker's to keep for every
   program. A drive BitLocker locked is told by its file calls failing
   with `FVE_E_LOCKED_VOLUME` (or `FVE_E_VOLUME_NOT_BOUND`), asked only of
   a drive whose volume says nothing of itself, without an administrator
   or COM; it is listed as locked, and opening it runs that verb on its
   root through `ShellExecuteEx`, waits for the dialog, and opens the
   drive if it is unlocked then (`places::SYSTEM_ASKS`,
   `unlock_by_system`). Skwir asks nothing and keeps nothing there.

## Consequences

- A passphrase the user chose to remember is in their keyring, unlocked
  with their session: any program of theirs can read it there, as GVfs's.
- The note under the field says what remembering costs; the guide says
  it too.
- A stale passphrase kept costs one key derivation (seconds) before the
  question, the place saying "Unlocking…" meanwhile.
- A keyring that asks to be unlocked waits for its own prompt on a thread
  of its own (`secrets::heard`, moved from the remote plugin), until the
  plugin is disabled.
- KWallet's volume passphrases (KDE's device notifier) are not read;
  neither are crypttab's key files (GVfs unlocks those without asking).
- macOS and Windows are compiled only (Wine has no BitLocker; macOS is
  never run here), and say so where recorded. Not known from here: whether
  `diskutil apfs unlockVolume` works from Skwir's window without Full Disk
  Access (a launch agent was refused, "restricted by Sandbox"), whether
  `bdeunlock.exe` ends when the drive is unlocked or only when its dialog
  closes (Skwir waits for it either way), and whether Windows Home shows
  the dialog (it reads BitLocker drives; its table of editions names only
  turning BitLocker on).
- On Windows a locked drive's label is unknown until it is unlocked: its
  row is "Local Disk (E:)", as Windows calls a drive without a label.
