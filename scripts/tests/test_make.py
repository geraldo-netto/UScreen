"""T220: Make must share its jobserver with Cargo and respect dry runs."""
from pathlib import Path
import os
import subprocess
import tempfile
import unittest

REPO = Path(__file__).resolve().parents[2]
CARGO = '''#!/usr/bin/env python3
import os, re, stat
match = re.search(r'--jobserver-auth=([^ ]+)', os.environ['MAKEFLAGS'])
assert match, 'T220: parallel Make omitted jobserver authentication'
auth = match.group(1)
if auth.startswith('fifo:'):
    assert stat.S_ISFIFO(os.stat(auth[5:]).st_mode)
else:
    for descriptor in auth.split(','):
        os.fstat(int(descriptor))
print('T220: cargo executed with usable jobserver')
'''


class MakeTest(unittest.TestCase):
    def make_environment(self, tools=None):
        env = dict(os.environ)
        for name in ['MAKEFLAGS', 'MFLAGS', 'CARGO_MAKEFLAGS']:
            env.pop(name, None)
        if tools is not None:
            env['PATH'] = str(tools) + os.pathsep + env['PATH']
        return env

    def run_dist(self, portable, *flags, container=None, listed='uscreen-build'):
        with tempfile.TemporaryDirectory(prefix='uscreen-make-dist-') as tmp:
            root = Path(tmp)
            makefile = (REPO / 'Makefile').read_text()
            for directory in ['scripts', 'packaging', 'bin']:
                (root / directory).mkdir()
            for path in ['scripts/build-release.sh', 'packaging/build-packages.sh']:
                script = root / path
                script.write_text('#!/bin/sh\nprintf "executed\\n" >> release-marker\n')
                script.chmod(0o755)
            distrobox = root / 'bin/distrobox'
            distrobox.write_text('#!/bin/sh\nprintf " %s \\n" "$USCREEN_TEST_CONTAINER_LIST"\n')
            distrobox.chmod(0o755)
            # Replace only the local fixture target; production dist dispatch stays intact.
            prefix = makefile[:makefile.index('dist-local: build')]
            (root / 'Makefile').write_text(prefix + 'dist-local: build\n\t@:\n')
            cargo = root / 'cargo'
            cargo.write_text(CARGO)
            cargo.chmod(0o755)
            env = self.make_environment(root / 'bin')
            env.pop('USCREEN_BUILD_CONTAINER', None)
            env['USCREEN_TEST_CONTAINER_LIST'] = listed if portable else ''
            if container is not None:
                env['USCREEN_BUILD_CONTAINER'] = container
            result = subprocess.run(
                ['make', '-j2', *flags, 'dist', f'CARGO={cargo}', 'CC=true'],
                cwd=root, env=env, capture_output=True, text=True)
            marker = root / 'release-marker'
            return result, marker.read_text() if marker.exists() else ''

    def test_t304_dist_uses_the_requested_build_container(self):
        for requested, listed, portable in [
            ('custom-build', 'custom-build', True),
            ('custom-build', 'uscreen-build', False),
            ('ci.build', 'ci.build', True),
            ('ci.build', 'ciXbuild', False),
            ('ci-build', 'ci-build-backup', False),
            ('', 'uscreen-build', True),
            (None, 'uscreen-build', True),
        ]:
            with self.subTest(requested=requested, listed=listed):
                result, marker = self.run_dist(True, container=requested, listed=listed)
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                self.assertEqual(marker, 'executed\nexecuted\n' if portable else '')
                if not portable:
                    self.assertIn(f'no {requested} container', result.stdout)
                    self.assertIn('T220: cargo executed with usable jobserver', result.stdout)

    def test_t291_dist_inspection_never_executes_release_scripts(self):
        for flag, expected_status in [('-n', 0), ('-t', 0), ('-q', 1)]:
            for portable in [False, True]:
                with self.subTest(flag=flag, portable=portable):
                    result, marker = self.run_dist(portable, flag)
                    self.assertEqual(result.returncode, expected_status, result.stdout + result.stderr)
                    self.assertEqual(marker, '', 'T291: Make inspection executed a release build')
                    self.assertNotIn('T220: cargo executed', result.stdout)

    def test_t291_dist_still_dispatches_portable_and_parallel_local_builds(self):
        result, marker = self.run_dist(True)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(marker, 'executed\nexecuted\n')
        result, marker = self.run_dist(False)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(marker, '')
        self.assertIn('T220: cargo executed with usable jobserver', result.stdout)

    def run_build(self, *flags):
        with tempfile.TemporaryDirectory(prefix='uscreen-make-') as tmp:
            root = Path(tmp)
            (root / 'Makefile').write_text((REPO / 'Makefile').read_text())
            cargo = root / 'cargo'
            cargo.write_text(CARGO)
            cargo.chmod(0o755)
            return subprocess.run(['make', '-j2', *flags, 'build', f'CARGO={cargo}', 'CC=true'],
                                  cwd=root, env=self.make_environment(), capture_output=True, text=True)

    def test_t261_failed_make_install_preserves_existing_binaries(self):
        with tempfile.TemporaryDirectory(prefix='uscreen-make-install-') as tmp:
            root = Path(tmp)
            (root / 'Makefile').write_text((REPO / 'Makefile').read_text())
            (root / 'scripts').mkdir()
            (root / 'scripts/install.sh').write_text((REPO / 'scripts/install.sh').read_text())
            source = root / 'target/release'
            source.mkdir(parents=True)
            installed = root / 'installed bin'
            installed.mkdir()
            for name in ['uscreen', 'uscreen-gui', 'evdi_helper']:
                (installed / name).write_text('old-' + name)
            (source / 'uscreen').write_text('new daemon')
            # Missing GUI/helper simulates a damaged build artifact after build.
            result = subprocess.run(['make', '-o', 'build', 'install', f'BIN_DIR={installed}'],
                                    cwd=root, capture_output=True, text=True)
            self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
            for name in ['uscreen', 'uscreen-gui', 'evdi_helper']:
                self.assertEqual((installed / name).read_text(), 'old-' + name,
                                 'T261: failed make install removed working binaries')
            self.assertEqual(list(installed.glob('.uscreen-*')), [])

    def test_t220_parallel_build_passes_open_jobserver(self):
        result = self.run_build()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn('T220: cargo executed with usable jobserver', result.stdout)

    def test_t220_dry_run_does_not_execute_cargo(self):
        for flag, expected_status in [('-n', 0), ('-t', 0), ('-q', 1)]:
            with self.subTest(flag=flag):
                result = self.run_build(flag)
                self.assertEqual(result.returncode, expected_status, result.stdout + result.stderr)
                self.assertNotIn('T220: cargo executed', result.stdout)


if __name__ == '__main__':
    unittest.main()
