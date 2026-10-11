"""Run the public release CLI against real Git repositories and fake just/gh."""

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
TRY_SCRIPT = SCRIPT.parent.parent / "package/try.sh"

# Fakes execute in subprocesses, so tests cover arguments, environment and files.
FAKE_JUST = r'''#!/usr/bin/env python3
import hashlib, json, os, pathlib, sys
args = sys.argv[1:]
with open('target/just-calls', 'a') as stream:
    stream.write(json.dumps(args) + '\n')
if args == ['release-check']:
    sys.exit(1 if os.getenv('FAIL_CHECK') else 0)
if len(args) == 1 and args[0] in ['lint-all', 'deny', 'test-all', 'smoke', 'poppler-check',
                                 'mupdf-check', 'bench-check', 'release-tools-check']:
    sys.exit(1 if os.getenv('FAIL_RECIPE') == args[0] else 0)
out = pathlib.Path(os.environ['SKWIR_PACKAGE_OUT'])
if args == ['package-appimage']:
    name = 'skwir-1.2.3-x86_64.AppImage'
    (out / name).write_bytes(b'synthetic image')
    sha = hashlib.sha256((out / name).read_bytes()).hexdigest()
    (out / (name + '.sha256')).write_text(f'{sha}  {name}\n')
elif args == ['package-try', 'debian']:
    for name in ['skwir', 'skwir-media']:
        (out / f'{name}_1.2.3-1_amd64.deb').write_bytes(b'synthetic deb')
elif args == ['package-try', 'fedora']:
    for name in ['skwir', 'skwir-media']:
        (out / f'{name}-1.2.3-1.x86_64.rpm').write_bytes(b'synthetic rpm')
elif args == ['package-try', 'arch']:
    for name in ['skwir', 'skwir-mermaid']:
        (out / f'{name}-1.2.3-1-x86_64.pkg.tar.zst').write_bytes(b'synthetic arch')
else:
    sys.exit('unexpected just command')
if os.getenv('EXTRA_ASSET'):
    (out / 'skwir-0.9.0-old.rpm').write_bytes(b'stale')
if os.getenv('MISSING_ASSET'):
    next(out.iterdir()).unlink()
if os.getenv('CHANGE_SOURCE'):
    pathlib.Path('CHANGELOG.md').write_text('changed during build')
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


class ReleaseTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="skwir-release-test-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name) / "source with ' spaces"
        self.root.mkdir()
        files = {
            ".gitignore": "/target/\n",
            "Cargo.toml": '[workspace.package]\nversion = "1.2.3"\n',
            "CHANGELOG.md": "# Changes\n\n## 1.2.3 (2026-10-11)\n\nSynthetic release.\n\n## 1.2.2 (2026-10-10)\nOld notes.\n",
            "crates/skwir/Cargo.toml": '''[package]
name = "skwir"
version.workspace = true
[package.metadata.deb.variants.media]
name = "skwir-media"
[package.metadata.generate-rpm.variants.media]
name = "skwir-media"
''',
            "tools/package/aur/PKGBUILD": "pkgname=(skwir skwir-mermaid)\npkgver=1.2.3\npkgrel=1\n",
            "tools/package/linux/org.skwir.skwir.metainfo.xml": '<component><releases><release version="1.2.3" date="2026-10-11"/></releases></component>',
        }
        for name, content in files.items():
            path = self.root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(content)
        script = self.root / "tools/release/release.py"
        script.parent.mkdir(parents=True)
        shutil.copyfile(SCRIPT, script)
        self.script = script
        self.bin = Path(self.temp.name) / "bin"
        self.bin.mkdir()
        for name, text in (("just", FAKE_JUST), ("gh", FAKE_GH)):
            program = self.bin / name
            program.write_text(text)
            program.chmod(0o755)
        self.env = {**os.environ, "PATH": f"{self.bin}:{os.environ['PATH']}",
                    "GIT_CONFIG_GLOBAL": os.devnull, "GIT_CONFIG_NOSYSTEM": "1"}
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

    def test_build_and_verify_isolated_bundle(self):
        stale = self.root / "target/package/linux/skwir-0.9.0-old.rpm"
        stale.parent.mkdir(parents=True)
        stale.write_bytes(b"old release")
        self.cli("build")
        self.cli("verify")
        self.assertEqual(len(list(self.bundle.iterdir())), 11)
        self.assertNotIn("Old notes", (self.bundle / "notes.md").read_text())
        self.assertTrue(stale.exists())
        self.assertEqual(self.calls("just"), [["release-check"], ["package-appimage"],
                                             ["package-try", "debian"], ["package-try", "fedora"],
                                             ["package-try", "arch"]])
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
        self.cli("metadata", ok=False, tag="v1.2.4")
        self.cli("metadata", ok=False, tag="v1.2.3;echo unsafe")

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
                         ["lint-all", "deny", "test-all", "smoke", "poppler-check",
                          "mupdf-check", "bench-check", "release-tools-check"]])

    def test_extra_missing_assets_and_source_changes_leave_no_bundle(self):
        for flag in ("EXTRA_ASSET", "MISSING_ASSET", "CHANGE_SOURCE"):
            with self.subTest(flag=flag):
                self.cli("build", ok=False, **{flag: "1"})
                self.assertFalse(self.bundle.exists())
                self.git("checkout", "--", "CHANGELOG.md")

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


if __name__ == "__main__":
    unittest.main()
