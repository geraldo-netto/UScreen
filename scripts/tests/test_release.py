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

    def enable_uploads(self):
        for name in ['scripts/build-release.sh', 'packaging/build-packages.sh']:
            self.write(name, '#!/bin/sh\nexit 0\n', True)
        for name in ['uscreen-1.2.3-linux-x86_64.tar.gz', 'uscreen_1.2.3_amd64.deb',
                     'uscreen-1.2.3-1.x86_64.rpm', 'uscreen-1.2.3-PKGBUILD.tar.gz',
                     'uscreen-1.2.3/uscreen.apk']:
            self.write('dist/' + name, 'asset ' + name)
        self.git('add', '.')
        self.git('commit', '-qm', 'mock builds')
        self.git('tag', '-fa', 'v1.2.3', '-m', 'mock release')
        self.git('push', '-q', '-f', 'origin', 'refs/tags/v1.2.3')
        # urllib is intercepted in every child interpreter. No real HTTP.
        site = self.base / 'sitecustomize.py'
        site.write_text((REPO / 'scripts/tests/release_api_stub.py').read_text())
        self.env['PYTHONPATH'] = str(self.base)
        real_python = shutil.which('python3')
        wrapper = self.bin / 'python3'
        wrapper.write_text(f'#!/bin/sh\nprintf "%s\\n" "$@" >> "$USCREEN_TEST_ROOT/argv"\nexec {real_python} "$@"\n')
        wrapper.chmod(0o755)
        curl = self.bin / 'curl'
        curl.write_text("""#!/bin/sh\nprintf '%s\\n' "$@" >> "$USCREEN_TEST_ROOT/argv"\nprintf '{"name":"asset","state":"uploaded"}\\n'\n""")
        curl.chmod(0o755)

    def test_t117_failed_uploads_never_publish(self):
        import json
        self.enable_uploads()
        for failure in ['http', 'api']:
            for index in range(6):
                with self.subTest(failure=failure, index=index):
                    (self.base / 'api-state').unlink(missing_ok=True)
                    self.env.update(USCREEN_TEST_FAILURE=failure, USCREEN_TEST_FAIL_INDEX=str(index))
                    result = self.publish()
                    self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
                    state = json.loads((self.base / 'api-state').read_text())
                    self.assertFalse(state['published'], 'incomplete release made public')
                    self.assertTrue(state['draft'])

    def test_t117_verifies_uploaded_set_and_digests(self):
        import json
        self.enable_uploads()
        for failure in ['digest', 'missing']:
            with self.subTest(failure=failure):
                (self.base / 'api-state').unlink(missing_ok=True)
                self.env['USCREEN_TEST_FAILURE'] = failure
                result = self.publish()
                self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
                self.assertFalse(json.loads((self.base / 'api-state').read_text())['published'])

    def test_t117_only_complete_verified_release_is_published(self):
        import json
        self.enable_uploads()
        result = self.publish()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        state = json.loads((self.base / 'api-state').read_text())
        self.assertTrue(state['published'])
        self.assertEqual(len(state['assets']), 6)
        requests = [json.loads(line) for line in (self.base / 'requests').read_text().splitlines()]
        self.assertEqual(requests[-1]['method'], 'PATCH')
        self.assertTrue(any(r['method'] == 'GET' for r in requests))

    def test_t118_credentials_absent_from_arguments_and_logs(self):
        self.enable_uploads()
        result = self.publish()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        arguments = (self.base / 'argv').read_text()
        self.assertNotIn(self.env['GH_TOKEN'], arguments + result.stdout + result.stderr)
        requests = (self.base / 'requests').read_text().splitlines()
        import json
        self.assertGreaterEqual(len(requests), 7)
        self.assertTrue(all(json.loads(line)['authorized'] for line in requests))

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
