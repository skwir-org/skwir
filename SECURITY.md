# Security

skwir handles your files, runs plugins from outside its code, and lets
agents act through it, so some problems are not for a public issue:

- a private folder (Settings › Privacy) reaching an agent or a plugin
  from outside skwir;
- a plugin getting what its manifest did not declare (another file, the
  network, a program), or leaving its isolation;
- an agent or a script changing files without the review its plan needs;
- a file whose preview or thumbnail runs code, or reads beyond itself.

Report one privately, through the repository's security advisories
("Report a vulnerability" on its Security tab), with what is needed to
see it happen, and whether you want to be named in the release that
fixes it.

Only the latest release is supported. The plugin protocol is
experimental: a fix may change it.
