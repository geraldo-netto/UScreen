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
    def run_build(self, *flags):
        with tempfile.TemporaryDirectory(prefix='uscreen-make-') as tmp:
            root = Path(tmp)
            (root / 'Makefile').write_text((REPO / 'Makefile').read_text())
            cargo = root / 'cargo'
            cargo.write_text(CARGO)
            cargo.chmod(0o755)
            env = dict(os.environ)
            for name in ['MAKEFLAGS', 'MFLAGS', 'CARGO_MAKEFLAGS']:
                env.pop(name, None)
            return subprocess.run(['make', '-j2', *flags, 'build', f'CARGO={cargo}', 'CC=true'],
                                  cwd=root, env=env, capture_output=True, text=True)

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
