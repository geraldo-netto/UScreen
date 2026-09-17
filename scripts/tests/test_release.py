"""Offline release regressions; never contact a remote service."""
from pathlib import Path
import os
import re
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

    def test_t225_publisher_routes_every_request_to_fork(self):
        import json
        from urllib.parse import urlsplit
        self.enable_uploads()
        result = self.publish()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        requests = (self.base / 'requests').read_text().splitlines()
        self.assertGreaterEqual(len(requests), 9)
        for line in requests:
            url = urlsplit(json.loads(line)['url'])
            self.assertIn(url.netloc, ['api.github.com', 'uploads.github.com'])
            self.assertTrue(url.path.startswith('/repos/geraldo-netto/UScreen/releases'), url.geturl())
        self.assertIn('https://github.com/geraldo-netto/UScreen/releases/tag/v1.2.3', result.stdout)

    def test_t225_active_project_links_target_fork(self):
        files = [
            'README.md', 'CHANGELOG.md', 'CITATION.cff', 'host/src/update.rs',
            'gui/src/main.rs', 'scripts/uscreen.service', 'scripts/publish-release.sh',
            'packaging/arch/PKGBUILD', 'packaging/deb/control', 'packaging/rpm/uscreen.spec',
            'android/app/src/main/java/com/uscreen/UpdateCheck.kt',
            'android/app/src/main/java/com/uscreen/MainActivity.kt',
        ]
        files.extend(str(path.relative_to(REPO)) for path in (REPO / 'docs').iterdir() if path.is_file())
        # Numbered upstream reports and explicit provenance remain valid citations.
        historical = r'https://github\.com/majmichu1/UScreen/(?:issues|discussions)/\d+[^\s)"<>]*'
        for name in files:
            with self.subTest(file=name):
                text = re.sub(historical, '', (REPO / name).read_text())
                text = text.replace('[upstream project](https://github.com/majmichu1/UScreen)', '')
                self.assertNotIn('majmichu1/UScreen', text)
                self.assertNotIn('majmichu1.github.io/UScreen', text)
        for name in ['host/src/update.rs', 'gui/src/main.rs',
                     'android/app/src/main/java/com/uscreen/UpdateCheck.kt']:
            self.assertIn('https://api.github.com/repos/geraldo-netto/UScreen/releases/latest',
                          (REPO / name).read_text())
        self.assertIn('https://github.com/geraldo-netto/UScreen/archive/refs/tags/v$pkgver.tar.gz',
                      (REPO / 'packaging/arch/PKGBUILD').read_text())

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


class MetadataTest(unittest.TestCase):
    FILES = ['docs/index.html', 'docs/llms.txt', 'docs/sitemap.xml', 'CITATION.cff']

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix='uscreen-metadata-')
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        for name in self.FILES + ['scripts/update-release-metadata.sh']:
            target = self.root / name
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(REPO / name, target)

    def update(self, check=False):
        return subprocess.run(
            ['bash', 'scripts/update-release-metadata.sh'] + (['--check'] if check else [])
            + ['9.9.9', '2030-01-02'], cwd=self.root, capture_output=True, text=True, timeout=10)

    def test_t277_invalid_input_preserves_all_metadata(self):
        citation = self.root / 'CITATION.cff'
        citation.write_text(citation.read_text().replace('version:', 'missing-version-marker:'))
        original = {name: (self.root / name).read_bytes() for name in self.FILES}
        for missing_file in [False, True]:
            with self.subTest(missing_file=missing_file):
                for name, contents in original.items():
                    (self.root / name).write_bytes(contents)
                if missing_file:
                    citation.unlink()
                before = {name: (self.root / name).read_bytes() for name in self.FILES
                          if (self.root / name).exists()}
                result = self.update()
                self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
                for name, contents in before.items():
                    self.assertEqual((self.root / name).read_bytes(), contents,
                                     'T277: failed validation modified ' + name)

    def test_t277_check_is_read_only_and_valid_update_completes(self):
        before = {name: (self.root / name).read_bytes() for name in self.FILES}
        self.assertNotEqual(self.update(check=True).returncode, 0)
        for name, contents in before.items():
            self.assertEqual((self.root / name).read_bytes(), contents)
        result = self.update()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn('version: "9.9.9"', (self.root / 'CITATION.cff').read_text())
        self.assertIn('Download 9.9.9</a>', (self.root / 'docs/index.html').read_text())
        self.assertIn('<lastmod>2030-01-02</lastmod>', (self.root / 'docs/sitemap.xml').read_text())
        result = self.update(check=True)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)


if __name__ == '__main__':
    unittest.main()
