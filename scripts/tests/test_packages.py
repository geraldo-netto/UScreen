"""T101: failed container/package builds cannot reuse older output."""
from pathlib import Path
import os
import shutil
import subprocess
import tempfile
import unittest

REPO = Path(__file__).resolve().parents[2]


class PackageTest(unittest.TestCase):
    def test_t101_container_and_rpmbuild_failures_reject_stale_assets(self):
        for mode in ['container', 'rpm', 'success']:
            with self.subTest(mode=mode), tempfile.TemporaryDirectory(prefix='uscreen-packages-') as tmp:
                root = Path(tmp)
                def write(name, body, executable=False):
                    path = root / name
                    path.parent.mkdir(parents=True, exist_ok=True)
                    path.write_text(body)
                    if executable:
                        path.chmod(0o755)
                for name in ['packaging/build-packages.sh', 'packaging/deb/control', 'packaging/deb/postinst',
                             'packaging/rpm/uscreen.spec', 'packaging/arch/PKGBUILD', 'packaging/arch/uscreen.install']:
                    write(name, (REPO / name).read_text(), name.endswith('.sh'))
                for name in ['README.md', 'LICENSE', 'THIRD_PARTY_LICENSES.md', 'licenses', 'SECURITY.md', 'CHANGELOG.md', 'CONTRIBUTING.md', 'docs']:
                    source = REPO / name
                    if source.is_dir(): shutil.copytree(source, root / name)
                    else: shutil.copy(source, root / name)
                write('scripts/copy-distribution-docs.sh', (REPO / 'scripts/copy-distribution-docs.sh').read_text(), True)
                write('packaging/distribution-docs.txt', (REPO / 'packaging/distribution-docs.txt').read_text())
                write('Makefile', 'VERSION = 1.2.3\n')
                for name in ['uscreen', 'uscreen-gui', 'evdi_helper', 'libevdi.so.1.15.0']:
                    write('dist/uscreen-1.2.3/bin/' + name, 'binary', True)
                for name in ['scripts/uscreen.desktop', 'packaging/icons/uscreen.svg', 'packaging/icons/uscreen-pen.svg',
                             'packaging/uscreen.service', 'packaging/uscreen-evdi.conf', 'packaging/uscreen-modules.conf',
                             'packaging/60-uscreen-uinput.rules']:
                    write(name, 'fixture')
                assets = ['uscreen_1.2.3_amd64.deb', 'uscreen-1.2.3-1.x86_64.rpm', 'uscreen-1.2.3-PKGBUILD.tar.gz']
                for name in assets + ['uscreen-1.2.3-linux-x86_64.tar.gz', '.packages-ok']:
                    write('dist/' + name, 'stale')
                write('bin/distrobox', '#!/bin/bash\nif [ "$USCREEN_TEST_MODE" != container ]; then bash -c "${@: -1}"; fi\nexit 0\n', True)
                write('bin/fakeroot', '#!/bin/sh\nexec "$@"\n', True)
                write('bin/dpkg-deb', '#!/bin/bash\nif [ "$1" = --info ]; then echo "Package: uscreen"; else echo new > "${@: -1}"; fi\n', True)
                write('bin/rpmbuild', '#!/bin/sh\nmkdir -p dist/rpmbuild/RPMS/x86_64\necho new > dist/rpmbuild/RPMS/x86_64/uscreen-1.2.3-1.x86_64.rpm\necho Wrote\n[ "$USCREEN_TEST_MODE" != rpm ]\n', True)
                env = dict(os.environ, PATH=f'{root}/bin:{os.environ["PATH"]}', USCREEN_TEST_MODE=mode)
                result = subprocess.run(['bash', 'packaging/build-packages.sh'], cwd=root, env=env, capture_output=True, text=True)
                if mode == 'success':
                    self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                    self.assertTrue(all((root / 'dist' / name).is_file() for name in assets))
                else:
                    self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
                    self.assertFalse((root / 'dist' / assets[2]).exists())
                    for name in assets[:2]:
                        path = root / 'dist' / name
                        self.assertFalse(path.exists() and path.read_text() == 'stale')


if __name__ == '__main__':
    unittest.main()
