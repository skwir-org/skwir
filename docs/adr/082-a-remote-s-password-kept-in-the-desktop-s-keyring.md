# ADR-082: A remote's password kept in the desktop's keyring, when asked

Status: accepted, 2026-10-22, the user's choice ("implement the secrets
related points you recommended, as well as the optional reading the
passwords GVfs and Nautilus save"), after ADR-081's report named what was
left: a password-protected SMB, WebDAV or FTP server through rclone, on a
desktop with neither GVfs nor KIO. Amends ADR-077's seventh decision
("never stored"). Its first decision amended by ADR-088 (2026-10-25): an
encrypted volume's passphrase is kept the same way.

## Context

- ADR-077 kept no credential: ssh's, GVfs's and rclone's questions reach
  the window each time, and rclone opened only what needs no password of
  Skwir's (it cannot ask: it takes a password in its configuration, a
  connection string, or its environment, obscured).
- Skwir already keeps secrets in the desktop's keyring, never in its
  files (ADR-010: tile servers' keys): `skwir_platform::secrets`, the
  Secret Service through oo7 (GNOME Keyring, KWallet, KeePassXC; the
  portal's file keyring in a sandbox), the Keychain, the Credential
  Manager. A private keyring runs its tests (`tools/test/private-keyring.sh`).
- GVfs keeps the passwords the user told Nautilus to remember in the
  keyring under libsecret's network password schema
  (`org.gnome.keyring.NetworkPassword`): `protocol` (`sftp`, `smb`,
  `http` for WebDAV, `ftp`), `server`, `user`, `domain` (SMB), `port`
  where not the usual one, `object` and `authtype` (WebDAV's realm,
  `basic`). GVfs reads them itself; sshfs and rclone do not.

## Decision

1. **Asked with Remember.** The window's question for a server's password
   offers "Remember in the keyring", off until the user turns it on. Only
   a server's password: never a key's passphrase, a one-time code, an
   encrypted configuration's password, an archive's.
2. **Kept in the desktop's keyring alone**, once the connection it was
   given for succeeded: never in Skwir's files, logs or command lines.
   Keyed by the server (scheme, user, domain, host, port: `Address::server`),
   labelled for the keyring's own window ("Skwir: me@nas (SMB)").
3. **The keyring answers first.** Before the window is asked, Skwir's own
   password for the server, then (Linux) one GVfs or Nautilus kept for it,
   read only, the least specific first as GVfs takes it. One the server
   refuses is not given again in that connection: the window asks. When
   Skwir's own was refused and another taken without Remember, Skwir's is
   forgotten; GVfs's are never changed.
4. **rclone with a password.** SMB, WebDAV and FTP addresses: a password
   from the keyring is given at once; else rclone first tries without,
   and a refusal (an authentication error) asks the window, three times
   at most. rclone takes it obscured (`rclone obscure -`, on its input),
   in its environment alone (`RCLONE_SMB_PASS`, `RCLONE_WEBDAV_PASS`,
   `RCLONE_FTP_PASS`), which only the user's own programs read; never on a
   command line, which every user's can.
5. **Forget Password.** A remote whose password Skwir kept says so in its
   menu: Forget Password removes it from the keyring. Which servers have
   one is noted in the plugin's data (`remote.remembered`), the servers'
   keys alone.

## Consequences

- A password the user chose to remember is in their keyring, unlocked
  with their session: any program of theirs can read it there, as GVfs's.
- rclone's mount holds the obscured password in its environment for its
  life (rclone reverses it), as GVfs's daemon holds its own in memory.
- A keyring that does not answer (none installed, locked and refused) is
  a question asked as before: nothing is kept.
- KIO's passwords stay KDE's (ADR-081): its dialog, KWallet.
- A keyring that asks to be unlocked waits for its own prompt, which the
  user may leave open: each call is made on a thread of its own and the
  connection waits for its answer or its Cancel, whichever comes first
  (2026-10-23; before, an unanswered prompt held the connection with no
  way out).
- Tried (2026-10-23): GNOME Keyring locked, its prompt (GCR's) answered
  and left open; KeePassXC's Secret Service (2.7, a database exposing a
  group); the Credential Manager through Skwir's own calls under Wine.
  The Keychain is compiled only.
