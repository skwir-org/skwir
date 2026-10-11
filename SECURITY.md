# Security

skwir handles your files, runs plugins from outside its code, and lets
agents act through it. Report these problems privately, not in a public
issue:

- a private folder (Settings › Privacy) reaching an agent or a plugin
  from outside skwir;
- a plugin getting what its manifest did not declare (another file, the
  network, a program), or escaping its isolation;
- an agent or a script changing files without the review its plan needs;
- a file whose preview or thumbnail runs code, or reads beyond the file.

Use the repository's security advisories ("Report a vulnerability" on
its Security tab). Include what is needed to reproduce the problem, and
whether you want to be credited in the release that fixes it.

Only the latest release is supported. The plugin protocol is
experimental: a fix may change it.
