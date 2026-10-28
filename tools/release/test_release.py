"""Run the public release CLI against real Git repositories and fake just/gh."""

import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import tempfile
import unittest

SCRIPT = Path(__file__).with_name("release.py")
AUR_SCRIPT = SCRIPT.with_name("aur.py")
TRY_SCRIPT = SCRIPT.parent.parent / "package/try.sh"
AUR_BASES = ("skwir", "skwir-bin", "skwir-git")

# Fakes execute in subprocesses, so tests cover arguments, environment and files.
FAKE_JUST = r'''#!/usr/bin/env python3
import hashlib, json, os, pathlib, sys
args = sys.argv[1:]
with open('target/just-calls', 'a') as stream:
    stream.write(json.dumps(args) + '\n')
if args == ['release-check']:
    sys.exit(1 if os.getenv('FAIL_CHECK') else 0)
if len(args) == 1 and args[0] in ['lint-all', 'deny', 'notices-check', 'test-all', 'smoke',
                                 'a11y-check', 'poppler-check', 'mupdf-check', 'djvu-check',
                                 'bench-check', 'release-tools-check']:
    sys.exit(1 if os.getenv('FAIL_RECIPE') == args[0] else 0)
out = pathlib.Path(os.environ['SKWIR_PACKAGE_OUT'])
if args == ['package-appimage']:
    for name in ['skwir-1.2.3-x86_64.AppImage', 'skwir-1.2.3-linux-x86_64.tar.gz']:
        (out / name).write_bytes(b'synthetic ' + name.encode())
        sha = hashlib.sha256((out / name).read_bytes()).hexdigest()
        (out / (name + '.sha256')).write_text(f'{sha}  {name}\n')
elif args == ['package-try', 'debian']:
    for name in ['skwir', 'skwir-media']:
        (out / f'{name}_1.2.3-1_amd64.deb').write_bytes(b'synthetic deb')
elif args == ['package-try', 'ubuntu']:
    for name in ['skwir', 'skwir-media']:
        (out / f'{name}_1.2.3-1ubuntu24.04_amd64.deb').write_bytes(b'synthetic ubuntu deb')
elif args == ['package-try', 'fedora']:
    for name in ['skwir', 'skwir-media']:
        (out / f'{name}-1.2.3-1.x86_64.rpm').write_bytes(b'synthetic rpm')
elif args == ['package-try', 'arch']:
    for name in ['skwir', 'skwir-mermaid']:
        (out / f'{name}-1.2.3-1-x86_64.pkg.tar.zst').write_bytes(b'synthetic arch')
elif args in (['package-windows'], ['package-macos']):
    names = (['skwir-1.2.3-windows-x86_64.msi', 'skwir-1.2.3-windows-x86_64.zip']
             if args == ['package-windows'] else
             ['skwir-1.2.3-macos-arm64.dmg', 'skwir-1.2.3-macos-arm64.zip'])
    for name in names:
        (out / name).write_bytes(b'synthetic ' + name.encode())
        sha = hashlib.sha256((out / name).read_bytes()).hexdigest()
        (out / (name + '.sha256')).write_text(f'{sha}  {name}\n')
else:
    sys.exit('unexpected just command')
if os.getenv('EXTRA_ASSET'):
    (out / 'skwir-0.9.0-old.rpm').write_bytes(b'stale')
if os.getenv('MISSING_ASSET'):
    next(out.iterdir()).unlink()
if os.getenv('CHANGE_SOURCE'):
    pathlib.Path('CHANGELOG.md').write_text('changed during build')
if os.getenv('WRONG_COMPANION') and args == ['package-appimage']:
    (out / 'skwir-1.2.3-linux-x86_64.tar.gz.sha256').write_text(
        f"{'0' * 64}  skwir-1.2.3-linux-x86_64.tar.gz\n")
'''

FAKE_GH = r'''#!/usr/bin/env python3
import hashlib, json, os, pathlib, subprocess, sys
assert os.environ.get('GH_HOST') == 'github.com'
args = sys.argv[1:]
with open('target/gh-calls', 'a') as stream:
    stream.write(json.dumps(args) + '\n')
calls = [json.loads(line) for line in pathlib.Path('target/gh-calls').read_text().splitlines()]
created = any(c[:2] == ['release', 'create'] for c in calls)
uploads = [c for c in calls if c[:2] == ['release', 'upload']]
if args[:2] == ['auth', 'status']:
    sys.exit(0)
if args[0] == 'api':
    endpoint = args[-1]
    commit = subprocess.check_output(['git', 'rev-parse', 'HEAD'], text=True).strip()
    if os.getenv('WRONG_REMOTE'):
        commit = '0' * 40
    if '/git/ref/' in endpoint:
        obj = {'type': 'tag', 'sha': 'a' * 40} if os.getenv('ANNOTATED') else {'type': 'commit', 'sha': commit}
        result = {'object': obj}
    elif '/git/tags/' in endpoint:
        result = {'object': {'type': 'commit', 'sha': commit}}
    elif '/assets?' in endpoint:
        if uploads:
            result = [[{'name': pathlib.Path(p).name, 'size': pathlib.Path(p).stat().st_size,
                        'state': 'uploaded',
                        'digest': 'sha256:' + hashlib.sha256(pathlib.Path(p).read_bytes()).hexdigest()}
                       for p in uploads[-1][6:]]]
            if os.getenv('WRONG_DIGEST'):
                result[0][0]['digest'] = 'sha256:' + '0' * 64
        else:
            result = [[{'name': 'unknown.zip'}]] if os.getenv('REMOTE_EXTRA') else [[]]
    elif '/releases?' in endpoint:
        result = [[{'id': 7, 'tag_name': 'v1.2.3', 'draft': not bool(os.getenv('PUBLISHED')),
                    'immutable': bool(os.getenv('IMMUTABLE'))}]] if os.getenv('EXISTING') or created else [[]]
    else:
        sys.exit('unexpected API request: ' + endpoint)
    print(json.dumps(result))
elif args[:2] in (['release', 'create'], ['release', 'upload'], ['release', 'edit']):
    if os.getenv('FAIL_UPLOAD') and args[1] == 'upload':
        sys.exit(1)
    sys.exit(0)
else:
    sys.exit('unexpected gh command')
'''


# Relative links and wrapped lines stay so in the repository and the bundle; the
# release page, which breaks at each line's end, gets the tag's files and a line
# for each paragraph and list item.
NOTES = ("Synthetic release: [help](docs/HELP.md#on-a-mac), ![icon](docs/icon.svg),\n"
         "[the docs](docs/), [site](https://skwir.org/), [above](#changes) and `[code](docs/none.md)`.\n"
         "\n- A list item\n  wrapped,\n  - one in it\n    wrapped.\n"
         "\n### Known\nlimits written\nhere.\n"
         "\n```\nkept\nas written\n```\nBroken here  \nand there.\n"
         "\n[ref]: <docs/HELP.md>\n")
POSTED = ("Synthetic release: [help](https://github.com/example/skwir/blob/v1.2.3/docs/HELP.md#on-a-mac), "
          "![icon](https://github.com/example/skwir/raw/v1.2.3/docs/icon.svg), "
          "[the docs](https://github.com/example/skwir/tree/v1.2.3/docs), [site](https://skwir.org/), "
          "[above](#changes) and `[code](docs/none.md)`.\n"
          "\n- A list item wrapped,\n  - one in it wrapped.\n"
          "\n### Known\nlimits written here.\n"
          "\n```\nkept\nas written\n```\nBroken here  \nand there.\n"
          "\n[ref]: https://github.com/example/skwir/blob/v1.2.3/docs/HELP.md\n")


class ReleaseTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="skwir-release-test-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name) / "source with ' spaces"
        self.root.mkdir()
        files = {
            ".gitignore": "/target/\n",
            "Cargo.toml": '[workspace.package]\nversion = "1.2.3"\n',
            "CHANGELOG.md": "# Changes\n\n## 1.2.3 (2026-10-11)\n\n" + NOTES + "\n## 1.2.2 (2026-10-10)\nOld notes.\n",
            "docs/HELP.md": "# Help\n",
            "docs/icon.svg": "<svg/>\n",
            "crates/skwir/Cargo.toml": '''[package]
name = "skwir"
version.workspace = true
[package.metadata.deb.variants.media]
name = "skwir-media"
[package.metadata.generate-rpm.variants.media]
name = "skwir-media"
''',
            "tools/package/aur/PKGBUILD": "pkgbase=skwir\npkgname=(skwir skwir-mermaid)\npkgver=1.2.3\npkgrel=1\n"
                "pkgdesc='Synthetic package'\narch=('x86_64' 'aarch64')\nlicense=('Apache-2.0')\n"
                "source=('skwir-1.2.3.tar.gz::https://github.com/example/skwir/archive/v1.2.3.tar.gz')\n"
                "sha256sums=('SKIP')\npackage_skwir() { :; }\npackage_skwir-mermaid() { :; }\n",
            "tools/package/aur/skwir-bin/PKGBUILD": "pkgname=skwir-bin\npkgver=1.2.3\npkgrel=1\n"
                "pkgdesc='Synthetic package'\narch=('x86_64')\nlicense=('Apache-2.0')\n"
                "source=('skwir-1.2.3-linux-x86_64.tar.gz::https://github.com/example/skwir/releases/download/"
                "v1.2.3/skwir-1.2.3-linux-x86_64.tar.gz')\nsha256sums=('SKIP')\npackage() { :; }\n",
            "tools/package/aur/skwir-git/PKGBUILD": "pkgbase=skwir-git\npkgname=(skwir-git skwir-mermaid-git)\n"
                "pkgver=1.2.2.r4.g0000000\npkgrel=2\npkgdesc='Synthetic package'\narch=('x86_64' 'aarch64')\n"
                "license=('Apache-2.0')\nsource=('skwir::git+https://github.com/example/skwir.git')\n"
                "sha256sums=('SKIP')\npackage_skwir-git() { :; }\npackage_skwir-mermaid-git() { :; }\n",
            "tools/package/linux/org.skwir.skwir.metainfo.xml": '<component><releases><release version="1.2.3" date="2026-10-11"/></releases></component>',
        }
        for name, content in files.items():
            path = self.root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(content)
        script = self.root / "tools/release/release.py"
        script.parent.mkdir(parents=True)
        shutil.copyfile(SCRIPT, script)
        shutil.copyfile(AUR_SCRIPT, script.with_name("aur.py"))
        self.script = script
        self.bin = Path(self.temp.name) / "bin"
        self.bin.mkdir()
        for name, text in (("just", FAKE_JUST), ("gh", FAKE_GH)):
            program = self.bin / name
            program.write_text(text)
            program.chmod(0o755)
        # A stand-in for the macOS SDK `just macos-sdk` downloads, where the
        # release looks for it: never this machine's own.
        data = Path(self.temp.name) / "data"
        (data / "skwir-build/macos-sdk").mkdir(parents=True)
        (data / "skwir-build/macos-sdk/env.sh").write_text("# xmac's, in a real SDK\n")
        self.env = {**os.environ, "PATH": f"{self.bin}:{os.environ['PATH']}",
                    "GIT_CONFIG_GLOBAL": os.devnull, "GIT_CONFIG_NOSYSTEM": "1",
                    "XDG_DATA_HOME": str(data)}
        self.git("init", "-q")
        self.git("config", "user.name", "Release Test")
        self.git("config", "user.email", "test@example.invalid")
        self.git("add", ".")
        self.git("commit", "-qm", "Synthetic release")
        self.git("tag", "-a", "v1.2.3", "-m", "Synthetic tag")
        self.git("remote", "add", "origin", "git@github.com:example/skwir.git")
        self.bundle = self.root / "target/releases/v1.2.3"

    def git(self, *args):
        return subprocess.run(["git", *args], cwd=self.root, env=self.env,
                              check=True, capture_output=True, text=True).stdout.strip()

    def cli(self, command, ok=True, tag="v1.2.3", **env):
        result = subprocess.run([sys.executable, str(self.script), command] + ([tag] if tag else []),
                                cwd=self.root, env={**self.env, **env},
                                capture_output=True, text=True)
        self.assertEqual(result.returncode == 0, ok, result.stdout + result.stderr)
        return result.stdout + result.stderr

    def calls(self, program):
        path = self.root / f"target/{program}-calls"
        return [json.loads(line) for line in path.read_text().splitlines()] if path.exists() else []

    def assert_no_remote_changes(self):
        self.assertFalse([c for c in self.calls("gh") if c[0] == "release"])

    def aur_setup(self):
        self.cli("build")
        self.aur = self.root / "target/aur-publish/skwir"
        remotes = Path(self.temp.name) / "aur"
        self.aur_remotes = {base: remotes / f"{base}.git" for base in AUR_BASES}
        self.aur_remote = self.aur_remotes["skwir"]
        for remote in self.aur_remotes.values():
            subprocess.run(["git", "init", "--bare", "--initial-branch=master", str(remote)],
                           env=self.env, check=True, capture_output=True)
        programs = {
            "makepkg": r'''#!/usr/bin/env python3
import os, pathlib, re, shlex, sys
assert sys.argv[1:] == ['--printsrcinfo']
if os.getenv('FAIL_MAKEPKG'):
    sys.exit(1)
text = pathlib.Path('PKGBUILD').read_text()
def values(key):
    return shlex.split(re.search(r'^' + key + r'=(.+)$', text, re.M)[1].strip('()'))
print('pkgbase = ' + re.search(r'^pkg(?:base|name)=([^(\n]+)$', text, re.M)[1])
for key in ('pkgver', 'pkgrel', 'source', 'sha256sums'):
    for value in values(key):
        print(f'\t{key} = {value}')
for name in values('pkgname'):
    print(f'pkgname = {name}')
''',
            "curl": r'''#!/usr/bin/env python3
import os, pathlib, sys
if os.getenv('FAIL_DOWNLOAD'):
    sys.exit(1)
url = sys.argv[-1]
out = pathlib.Path(sys.argv[sys.argv.index('--output') + 1])
if url == 'https://github.com/example/skwir/archive/v1.2.3.tar.gz':
    out.write_bytes(b'synthetic source archive')
elif url == 'https://github.com/example/skwir/releases/download/v1.2.3/skwir-1.2.3-linux-x86_64.tar.gz':
    out.write_bytes(b'not the release' if os.getenv('WRONG_ASSET')
                    else b'synthetic skwir-1.2.3-linux-x86_64.tar.gz')
else:
    sys.exit('unexpected download: ' + url)
''',
            "aur-ssh": r'''#!/usr/bin/env python3
import os, shlex, subprocess, sys
service, path = shlex.split(sys.argv[-1])
base = path.removeprefix('/').removesuffix('.git')
assert base in ('skwir', 'skwir-bin', 'skwir-git'), path
assert service in ('git-upload-pack', 'git-receive-pack')
if service == 'git-receive-pack' and os.getenv('FAIL_PUSH') in ('1', base):
    sys.exit(1)
sys.exit(subprocess.call([service, os.path.join(os.environ['AUR_TEST_REMOTES'], base + '.git')]))
''',
        }
        for name, content in programs.items():
            path = self.bin / name
            path.write_text(content)
            path.chmod(0o755)
        self.env.update(GIT_SSH_COMMAND=str(self.bin / "aur-ssh"), GIT_SSH_VARIANT="ssh",
                        AUR_TEST_REMOTES=str(remotes))

    def aur_cli(self, command, ok=True, tag="v1.2.3", **env):
        result = subprocess.run([sys.executable, str(self.script.with_name("aur.py")), command, tag],
                                cwd=self.root, env={**self.env, **env}, capture_output=True, text=True)
        self.assertEqual(result.returncode == 0, ok, result.stdout + result.stderr)
        return result.stdout + result.stderr

    def aur_git(self, *args, base="skwir"):
        return subprocess.run(["git", "-C", str(self.aur.with_name(base)), *args], env=self.env,
                              check=True, capture_output=True, text=True).stdout.strip()

    def aur_prepare(self):
        self.aur_cli("prepare")
        for base in AUR_BASES:
            self.aur_git("config", "user.name", "Release Test", base=base)
            self.aur_git("config", "user.email", "test@example.invalid", base=base)

    def aur_remote_head(self, base="skwir"):
        result = subprocess.run(["git", "--git-dir", str(self.aur_remotes[base]), "rev-parse", "--verify", "HEAD"],
                                env=self.env, capture_output=True, text=True)
        return result.stdout.strip() if result.returncode == 0 else None

    def test_aur_prepare_and_publish_split_package_with_retry(self):
        self.aur_setup()
        self.aur_prepare()
        self.assertIsNone(self.aur_remote_head())
        pkgbuild = (self.aur / "PKGBUILD").read_text()
        info = (self.aur / ".SRCINFO").read_text()
        checksum = hashlib.sha256(b"synthetic source archive").hexdigest()
        self.assertIn(f"sha256sums=('{checksum}')", pkgbuild)
        self.assertIn(f"sha256sums = {checksum}", info)
        self.assertIn("pkgname = skwir-mermaid", info)
        self.assertIn("SKIP", (self.root / "tools/package/aur/PKGBUILD").read_text())
        # skwir-bin: the tarball GitHub serves, the bundle's own.
        asset = hashlib.sha256((self.bundle / "skwir-1.2.3-linux-x86_64.tar.gz").read_bytes()).hexdigest()
        self.assertIn(f"sha256sums=('{asset}')", (self.aur.with_name("skwir-bin") / "PKGBUILD").read_text())
        # skwir-git: named after the release's commit; its Git source unsummed.
        info = (self.aur.with_name("skwir-git") / ".SRCINFO").read_text()
        self.assertIn(f"pkgver = 1.2.3.r0.g{self.git('rev-parse', 'HEAD')[:7]}\n\tpkgrel = 1\n", info)
        self.assertIn("sha256sums = SKIP", info)
        self.aur_cli("publish", ok=False, FAIL_PUSH="1")
        self.assertIsNone(self.aur_remote_head())
        commits = {base: self.aur_git("rev-parse", "HEAD", base=base) for base in AUR_BASES}
        self.aur_cli("publish")
        self.assertEqual({base: self.aur_remote_head(base) for base in AUR_BASES}, commits)
        self.assertEqual(self.aur_git("ls-tree", "--name-only", "HEAD").splitlines(),
                         [".SRCINFO", "PKGBUILD"])
        self.aur_cli("publish")
        self.assertEqual(self.aur_remote_head(), commits["skwir"])
        self.aur_cli("prepare")
        self.aur_cli("publish")
        self.assertEqual({base: self.aur_remote_head(base) for base in AUR_BASES}, commits)

    def test_aur_publish_retries_the_packages_left(self):
        self.aur_setup()
        self.aur_prepare()
        self.aur_cli("publish", ok=False, FAIL_PUSH="skwir-git")
        self.assertIsNotNone(self.aur_remote_head("skwir-bin"))
        self.assertIsNone(self.aur_remote_head("skwir-git"))
        self.aur_cli("publish")
        self.assertEqual(self.aur_remote_head("skwir-git"), self.aur_git("rev-parse", "HEAD", base="skwir-git"))

    def test_aur_bin_waits_for_the_published_tarball(self):
        self.aur_setup()
        self.assertIn("not the release bundle's", self.aur_cli("prepare", ok=False, WRONG_ASSET="1"))
        # No package is prepared while one cannot be.
        for base in AUR_BASES:
            self.assertFalse((self.aur.with_name(base) / "PKGBUILD").exists())
            self.assertFalse((self.aur.parent / f"{base}.json").exists())
        self.aur_prepare()

    @unittest.skipUnless(shutil.which("makepkg"), "makepkg is available in the Linux Nix shell")
    def test_aur_with_real_makepkg(self):
        real_makepkg = shutil.which("makepkg")
        self.aur_setup()
        (self.bin / "makepkg").unlink()
        (self.bin / "makepkg").symlink_to(real_makepkg)
        self.aur_prepare()
        info = (self.aur / ".SRCINFO").read_text()
        self.assertNotIn("SKIP", info)
        self.assertIn("pkgname = skwir-mermaid", info)
        self.aur_cli("publish")
        self.assertEqual(self.aur_remote_head(), self.aur_git("rev-parse", "HEAD"))

    def test_aur_rejects_missing_bundle_remote_tag_and_failed_tools(self):
        self.aur_setup()
        for env in ({"WRONG_REMOTE": "1"}, {"FAIL_DOWNLOAD": "1"}, {"FAIL_MAKEPKG": "1"}):
            with self.subTest(env=env):
                self.aur_cli("prepare", ok=False, **env)
                self.assertIsNone(self.aur_remote_head())
                self.assertFalse((self.aur / "PKGBUILD").exists())
        (self.bundle / "notes.md").write_text("changed")
        self.assertIn("asset changed", self.aur_cli("prepare", ok=False))
        self.assertIsNone(self.aur_remote_head())

    def test_aur_refuses_changed_files_wrong_release_and_unrelated_changes(self):
        self.aur_setup()
        self.aur_prepare()
        info = self.aur / ".SRCINFO"
        original = info.read_text()
        info.write_text("changed")
        self.assertIn("files changed", self.aur_cli("publish", ok=False))
        info.write_text(original)
        unrelated = self.aur / "unrelated file"
        unrelated.touch()
        self.assertIn("unrelated changes", self.aur_cli("publish", ok=False))
        unrelated.unlink()
        state = self.root / "target/aur-publish/skwir.json"
        prepared = json.loads(state.read_text())
        prepared["tag"] = "v9.9.9"
        state.write_text(json.dumps(prepared))
        self.assertIn("another release", self.aur_cli("publish", ok=False))
        self.assertIsNone(self.aur_remote_head())

    def test_aur_refuses_dirty_preparation_and_redirected_origin(self):
        self.aur_setup()
        self.aur_prepare()
        self.assertIn("checkout has changes", self.aur_cli("prepare", ok=False))
        self.aur_git("remote", "set-url", "--push", "origin", "ssh://example.invalid/wrong.git")
        self.assertIn("origin disagrees", self.aur_cli("publish", ok=False))
        self.assertIsNone(self.aur_remote_head())

    def test_aur_remote_change_rejects_push_without_force(self):
        self.aur_setup()
        self.aur_prepare()
        other = Path(self.temp.name) / "other maintainer"
        subprocess.run(["git", "clone", str(self.aur_remote), str(other)],
                       env=self.env, check=True, capture_output=True)
        for key, value in (("user.name", "Other Test"), ("user.email", "other@example.invalid")):
            subprocess.run(["git", "-C", str(other), "config", key, value], env=self.env, check=True)
        (other / "PKGBUILD").write_text("another maintainer's package")
        for args in (("add", "."), ("commit", "-qm", "Other package update"), ("push", "origin", "master")):
            subprocess.run(["git", "-C", str(other), *args], env=self.env, check=True, capture_output=True)
        remote = self.aur_remote_head()
        self.aur_cli("publish", ok=False)
        self.assertEqual(self.aur_remote_head(), remote)

    def test_build_and_verify_isolated_bundle(self):
        stale = self.root / "target/package/linux/skwir-0.9.0-old.rpm"
        stale.parent.mkdir(parents=True)
        stale.write_bytes(b"old release")
        self.cli("build")
        self.cli("verify")
        # Linux's 15, then Windows' and macOS's builds to try with their sums.
        self.assertEqual(len(list(self.bundle.iterdir())), 23)
        for name in ["skwir-1.2.3-windows-x86_64.msi", "skwir-1.2.3-macos-arm64.dmg"]:
            self.assertTrue((self.bundle / name).is_file() and (self.bundle / f"{name}.sha256").is_file())
        self.assertNotIn("Old notes", (self.bundle / "notes.md").read_text())
        self.assertTrue(stale.exists())
        self.assertEqual(self.calls("just"), [["release-check"], ["package-appimage"],
                                             ["package-try", "debian"], ["package-try", "ubuntu"],
                                             ["package-try", "fedora"], ["package-try", "arch"],
                                             ["package-windows"], ["package-macos"]])
        self.cli("build", ok=False)
        self.cli("verify")

    def test_dirty_source_and_wrong_tag_stop_before_checks(self):
        (self.root / "untracked").write_text("synthetic")
        self.assertIn("clean", self.cli("build", ok=False))
        (self.root / "untracked").unlink()
        self.git("commit", "--allow-empty", "-qm", "Different source")
        self.assertIn("Check out", self.cli("build", ok=False))
        self.assertEqual(self.calls("just"), [])

    def test_unreleased_and_disagreeing_metadata(self):
        changelog = self.root / "CHANGELOG.md"
        original = changelog.read_text()
        changelog.write_text(original.replace("2026-10-11", "unreleased"))
        self.assertIn("dated section", self.cli("metadata", ok=False))
        changelog.write_text(original.replace("2026-10-11", "2026-10-12"))
        self.assertIn("AppStream", self.cli("metadata", ok=False))
        changelog.write_text(original.replace("# Changes\n\n", "# Changes\n\n## Unreleased\n\n- More.\n\n"))
        self.assertIn("“## Unreleased” above 1.2.3", self.cli("metadata", ok=False))
        for target in ("docs/gone.md", "../outside.md"):
            changelog.write_text(original.replace("docs/HELP.md#on-a-mac", target))
            self.assertIn(f"not in the source: {target}", self.cli("metadata", ok=False))
        self.cli("metadata", ok=False, tag="v1.2.4")
        self.cli("metadata", ok=False, tag="v1.2.3;echo unsafe")

    def test_documents_name_the_release_or_no_version(self):
        readme, guide = self.root / "README.md", self.root / "docs/guide.md"
        readme.write_text("Releases are for Linux.\n\n```sh\nsudo apt install ./skwir_1.2.3-1_amd64.deb\n"
                          "./skwir-1.2.3-x86_64.AppImage\n```\n\nSkwir 1.2 under the GPL version 3.\n")
        guide.write_text("This guide is for Skwir on Linux, with Rust 1.96.\n")
        self.cli("metadata")
        for document, text, named in (
            (readme, "sudo dnf install ./skwir-media-1.2.2-1.x86_64.rpm\n", "1.2.2"),
            (readme, "tar -xzf skwir-1.2.2-linux-x86_64.tar.gz\n", "1.2.2"),
            (guide, "This guide is for Skwir 1.1 on Linux.\n", "1.1"),
            (guide, "Version 1.2.4 is for Linux.\n", "1.2.4"),
        ):
            original = document.read_text()
            document.write_text(text)
            self.assertIn(f"{document.relative_to(self.root)} names {named}, not 1.2.3",
                          self.cli("metadata", ok=False))
            document.write_text(original)
        self.cli("metadata")

    def test_no_macos_sdk_stops_before_the_checks(self):
        (Path(self.env["XDG_DATA_HOME"]) / "skwir-build/macos-sdk/env.sh").unlink()
        self.assertIn("just macos-sdk", self.cli("build", ok=False))
        self.assertEqual(self.calls("just"), [])
        self.assertFalse(self.bundle.exists())

    def test_failed_checks_do_not_package(self):
        self.cli("build", ok=False, FAIL_CHECK="1")
        self.assertEqual(self.calls("just"), [["release-check"]])
        self.assertFalse(self.bundle.exists())

    def test_full_gate_stops_at_the_first_failure(self):
        (self.root / "target").mkdir()
        self.cli("check", ok=False, tag=None, FAIL_RECIPE="deny")
        self.assertEqual(self.calls("just"), [["lint-all"], ["deny"]])
        (self.root / "target/just-calls").unlink()
        self.cli("check", tag=None)
        self.assertEqual(self.calls("just"), [[r] for r in
                         ["lint-all", "deny", "notices-check", "test-all", "smoke",
                          "a11y-check", "poppler-check", "mupdf-check", "djvu-check", "bench-check",
                          "release-tools-check"]])

    def test_extra_missing_assets_and_source_changes_leave_no_bundle(self):
        for flag in ("EXTRA_ASSET", "MISSING_ASSET", "CHANGE_SOURCE"):
            with self.subTest(flag=flag):
                self.cli("build", ok=False, **{flag: "1"})
                self.assertFalse(self.bundle.exists())
                self.git("checkout", "--", "CHANGELOG.md")

    def test_a_wrong_companion_checksum_leaves_no_bundle(self):
        self.assertIn("companion checksum", self.cli("build", ok=False, WRONG_COMPANION="1"))
        self.assertFalse(self.bundle.exists())

    def test_modified_assets_refuse_upload_before_gh(self):
        self.cli("build")
        image = self.bundle / "skwir-1.2.3-x86_64.AppImage"
        image.write_bytes(b"tampered")
        self.assertIn("asset changed", self.cli("upload", ok=False))
        self.assertEqual(self.calls("gh"), [])

    def test_checksum_and_manifest_corruption(self):
        self.cli("build")
        manifest = self.bundle / "manifest.json"
        original = manifest.read_text()
        data = json.loads(original)
        data["checks"] = []
        manifest.write_text(json.dumps(data))
        self.cli("verify", ok=False)
        manifest.write_text(original)
        (self.bundle / "SHA256SUMS").write_text("wrong checksum list\n")
        self.cli("upload", ok=False)
        self.assertEqual(self.calls("gh"), [])

    def test_remote_mismatch_published_immutable_and_unknown_assets_refused(self):
        self.cli("build")
        for flags in ({"WRONG_REMOTE": "1"}, {"EXISTING": "1", "PUBLISHED": "1"},
                      {"EXISTING": "1", "IMMUTABLE": "1"}, {"EXISTING": "1", "REMOTE_EXTRA": "1"}):
            with self.subTest(flags=flags):
                self.cli("upload", ok=False, **flags)
                self.assert_no_remote_changes()

    def test_create_draft_with_exact_assets_and_annotated_remote_tag(self):
        self.cli("build")
        self.cli("upload", ANNOTATED="1", GH_HOST="example.invalid")
        mutations = [c for c in self.calls("gh") if c[0] == "release"]
        self.assertEqual([c[1] for c in mutations], ["create", "upload", "edit"])
        self.assertIn("--draft", mutations[0])
        self.assertIn("--verify-tag", mutations[0])
        self.assertIn("--draft=true", mutations[-1])
        self.assertEqual({Path(p).name for p in mutations[1][6:]},
                         {p.name for p in self.bundle.iterdir()})
        self.assertEqual((self.bundle / "notes.md").read_text(), NOTES)
        self.assertEqual(mutations[0][-1], "--notes=" + POSTED)
        self.assertEqual(mutations[-1][-1], "--notes=" + POSTED)

    def test_the_page_shows_a_section_above_its_details_and_links_to_them(self):
        changelog = self.root / "CHANGELOG.md"
        changelog.write_text("# Changes\n\n## 1.2.3 (2026-10-11)\n\nSummary,\nwrapped.\n\n- One highlight.\n"
                             "\n```\n### All changes\n```\n\n### All changes\n\n- Every change, in full.\n"
                             "\n## 1.2.2 (2026-10-10)\nOld notes.\n")
        self.git("commit", "-qam", "A release page")
        self.git("tag", "-fa", "v1.2.3", "-m", "Synthetic tag")
        self.cli("build")
        self.assertIn("- Every change, in full.\n", (self.bundle / "notes.md").read_text())
        self.cli("upload")
        mutations = [c for c in self.calls("gh") if c[0] == "release"]
        self.assertEqual(mutations[-1][-1], "--notes=Summary, wrapped.\n\n- One highlight.\n"
                         "\n```\n### All changes\n```\n\n**Full details**: [CHANGELOG.md]"
                         "(https://github.com/example/skwir/blob/v1.2.3/CHANGELOG.md#123-2026-10-11)\n")
        # A page or details left empty stops the release.
        for section in ("### All changes\n\n- Every change.\n", "Summary.\n\n### All changes\n"):
            changelog.write_text("# Changes\n\n## 1.2.3 (2026-10-11)\n\n" + section)
            self.assertIn("All changes", self.cli("metadata", ok=False))

    def test_update_draft_and_retry_failed_upload(self):
        self.cli("build")
        self.cli("upload", ok=False, EXISTING="1", FAIL_UPLOAD="1")
        self.assertFalse([c for c in self.calls("gh") if c[:2] == ["release", "edit"]])
        self.cli("upload", EXISTING="1")
        self.assertFalse([c for c in self.calls("gh") if c[:2] == ["release", "create"]])
        self.assertTrue([c for c in self.calls("gh") if c[:2] == ["release", "edit"]])

    def test_remote_digest_mismatch_fails_before_notes_update(self):
        self.cli("build")
        self.assertIn("SHA256 digests disagree", self.cli("upload", ok=False, WRONG_DIGEST="1"))
        self.assertFalse([c for c in self.calls("gh") if c[:2] == ["release", "edit"]])

    def test_distribution_outputs_are_fresh_and_only_copied_after_checks(self):
        # Exercise the real package root orchestrator, stubbing only namespace
        # entry. No network, package manager or privileged operation is used.
        package_script = self.root / "tools/package/try.sh"
        shutil.copyfile(TRY_SCRIPT, package_script)
        test_home = Path(self.temp.name) / "home"
        roots = test_home / ".cache/skwir-tests/roots"
        builder = roots / "debian-build"
        builder.mkdir(parents=True)
        (builder / ".skwir-prepared").touch()
        stale = builder / "build/target/debian/skwir_0.9.0-1_amd64.deb"
        stale.parent.mkdir(parents=True)
        stale.write_bytes(b"old version")
        rootfs = Path(self.temp.name) / "rootfs"
        for directory in ("root", "etc/apt/apt.conf.d"):
            (rootfs / directory).mkdir(parents=True)
        with tarfile.open(roots / "debian.rootfs.tar.xz", "w:xz") as archive:
            archive.add(rootfs, arcname=".")
        fake = self.bin / "bwrap"
        fake.write_text(r'''#!/usr/bin/env python3
import os, pathlib, sys
args = sys.argv[1:]
root = pathlib.Path(args[args.index('--bind') + 1])
if 'cd /src && tools/package/deb.sh' in args:
    made = root / 'build/target/debian'
    assert not made.exists(), 'stale package outputs reached the build'
    made.mkdir(parents=True)
    for name in ['skwir', 'skwir-media']:
        (made / f'{name}_1.2.3-1_amd64.deb').write_bytes(b'synthetic package')
if '/tools/package/linux/prepare.sh' in args:
    with (root / 'prepared').open('a') as log:
        log.write('prepared\n')
if '/tools/package/linux/check.sh' in args and os.getenv('FAIL_PACKAGE_CHECK'):
    sys.exit(1)
''')
        fake.chmod(0o755)
        output = Path(self.temp.name) / "verified packages"
        for fail in (True, False):
            result = subprocess.run(["sh", str(package_script), "debian"], cwd=self.root,
                                    env={**self.env, "HOME": str(test_home),
                                         "SKWIR_PACKAGE_OUT": str(output),
                                         "FAIL_PACKAGE_CHECK": "1" if fail else ""},
                                    capture_output=True, text=True)
            self.assertEqual(result.returncode == 0, not fail, result.stdout + result.stderr)
            self.assertFalse(stale.exists())
            self.assertEqual({p.name for p in output.iterdir()}, set() if fail else
                             {"skwir_1.2.3-1_amd64.deb", "skwir-media_1.2.3-1_amd64.deb"})
        # The kept root was not unpacked again, and each try brought it up
        # to date: a library added to the lists since reaches it.
        self.assertEqual((builder / "prepared").read_text(), "prepared\n" * 2)


if __name__ == "__main__":
    unittest.main()
