"""Offline release regressions; never contact a remote service."""
from pathlib import Path
import os
import shutil
import subprocess
import tempfile
import unittest

REPO = Path(__file__).resolve().parents[2]


class ReleaseTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix='uscreen-release-')
        self.addCleanup(self.tmp.cleanup)
        self.base = Path(self.tmp.name)
        self.root = self.base / 'project'
        self.root.mkdir()
        self.bin = self.base / 'bin'
        self.bin.mkdir()
        self.env = dict(os.environ, PATH=f'{self.bin}:{os.environ["PATH"]}',
                        GH_TOKEN='dummy-release-secret', RELEASE_DATE='2026-09-16',
                        USCREEN_TEST_ROOT=str(self.base))
        self.write('Makefile', 'VERSION = 1.2.3\n')
        for file in ['host/Cargo.toml', 'gui/Cargo.toml', 'common/Cargo.toml']:
            self.write(file, 'version = "1.2.3"\n')
        self.write('android/app/build.gradle.kts', 'versionName = "1.2.3"\n')
        self.write('packaging/arch/PKGBUILD', 'pkgver=1.2.3\n')
        self.write('CHANGELOG.md', '## 1.2.3 — 2026-09-16\n')
        self.write('notes.md', 'Release notes\n')
        self.write('.gitignore', 'dist/\n')
        self.write('scripts/update-release-metadata.sh', '#!/bin/sh\nexit 0\n', True)
        for name in ['scripts/build-release.sh', 'packaging/build-packages.sh']:
            self.write(name, '#!/bin/sh\ntouch "$USCREEN_TEST_ROOT/build-called"\nexit 42\n', True)
        self.write('scripts/publish-release.sh', (REPO / 'scripts/publish-release.sh').read_text(), True)
        self.git('init', '-q')
        self.git('config', 'user.email', 'test@example.invalid')
        self.git('config', 'user.name', 'Regression Test')
        self.git('add', '.')
        self.git('commit', '-qm', 'release')
        self.git('tag', '-a', 'v1.2.3', '-m', 'release')
        subprocess.run(['git', 'init', '--bare', '-q', str(self.base / 'origin')], check=True)
        self.git('remote', 'add', 'origin', str(self.base / 'origin'))
        self.git('push', '-q', 'origin', 'HEAD:main', 'refs/tags/v1.2.3')

    def write(self, name, contents, executable=False):
        path = self.root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(contents)
        if executable:
            path.chmod(0o755)
        return path

    def git(self, *args):
        return subprocess.run(['git', *args], cwd=self.root, check=True, capture_output=True, text=True).stdout.strip()

    def publish(self):
        return subprocess.run(['bash', 'scripts/publish-release.sh', 'notes.md'],
                              cwd=self.root, env=self.env, capture_output=True, text=True, timeout=10)

    def reject_before_build(self):
        result = self.publish()
        self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertFalse((self.base / 'build-called').exists(), result.stdout + result.stderr)

    def test_t100_newer_head_rejected(self):
        self.write('new-source', 'new')
        self.git('add', '.')
        self.git('commit', '-qm', 'newer')
        self.reject_before_build()

    def test_t100_divergent_remote_tag_rejected(self):
        self.git('tag', '-fa', 'v1.2.3', '-m', 'different annotation')
        self.reject_before_build()

    def test_t100_missing_remote_tag_rejected(self):
        self.git('push', '-q', 'origin', ':refs/tags/v1.2.3')
        self.reject_before_build()

    def test_t100_matching_annotated_refs_reach_build(self):
        result = self.publish()
        self.assertEqual(result.returncode, 42, result.stdout + result.stderr)
        self.assertTrue((self.base / 'build-called').exists())


if __name__ == '__main__':
    unittest.main()
