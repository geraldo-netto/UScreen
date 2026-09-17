"""T129: shipped documentation and license links resolve in each artifact."""
from pathlib import Path
import os
import re
import shutil
import subprocess
import tempfile
import unittest

REPO = Path(__file__).resolve().parents[2]
NOTICES = ['LICENSE', 'THIRD_PARTY_LICENSES.md', 'licenses/libevdi-LGPL-2.1.txt']


class NoticeTest(unittest.TestCase):
    def test_t129_tar_deb_rpm_and_arch_include_notices(self):
        with tempfile.TemporaryDirectory(prefix='uscreen-notices-') as tmp:
            root = Path(tmp)
            self.copy_sources(root)
            def write(name, body, executable=False):
                path = root / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(body)
                if executable:
                    path.chmod(0o755)
            for name in ['release/uscreen', 'release/uscreen-gui', 'evdi_helper', 'evdi-src/library/libevdi.so.1.15.0']:
                write('target-deb12/' + name, '#!/bin/sh\nexit 0\n', True)
            write('bin/distrobox', '#!/bin/bash\nif [ "$USCREEN_TEST_BUILD" = portable ]; then touch target-deb12/.build-ok; else shift 3; shift 2; bash -c "$@"; fi\n', True)
            write('bin/objdump', '#!/bin/sh\necho GLIBC_2.36\n', True)
            write('bin/readelf', '#!/bin/sh\necho "RUNPATH [$ORIGIN]"\n', True)
            write('bin/fakeroot', '#!/bin/sh\nexec "$@"\n', True)
            write('android/gradlew', '#!/bin/sh\nexit 0\n', True)
            write('android/app/build/outputs/apk/release/app-release.apk', 'apk')
            env = dict(os.environ, PATH=f'{root}/bin:{os.environ["PATH"]}', USCREEN_TEST_BUILD='portable')
            def run(*args):
                result = subprocess.run(args, cwd=root, env=env, capture_output=True, text=True)
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                return result.stdout
            run('bash', 'scripts/build-release.sh')
            extracted = root / 'unpacked'
            extracted.mkdir()
            run('tar', '-xf', 'dist/uscreen-1.2.3-linux-x86_64.tar.gz', '-C', str(extracted))
            docs = extracted / 'uscreen-1.2.3'
            self.verify_docs(docs)
            env['USCREEN_TEST_BUILD'] = 'packages'
            run('bash', 'packaging/build-packages.sh')
            deb = root / 'deb'
            run('dpkg-deb', '-x', 'dist/uscreen_1.2.3_amd64.deb', str(deb))
            self.verify_docs(deb / 'usr/share/doc/uscreen')
            rpm_files = run('rpm', '-qpl', 'dist/uscreen-1.2.3-1.x86_64.rpm').splitlines()
            for name in NOTICES:
                self.assertIn('/usr/share/doc/uscreen/' + name, rpm_files)
            for link in re.findall(r'\]\(([^)]+)\)', (docs / 'README.md').read_text()):
                if ':' not in link and not link.startswith('#'):
                    self.assertIn('/usr/share/doc/uscreen/' + link.split('#')[0], rpm_files)
            # The real RPM install recipe generated these files before packing.
            rpm_roots = list((root / 'dist/rpmbuild/BUILDROOT').glob('*/usr/share/doc/uscreen'))
            # RPM normally cleans BUILDROOT; the archive inventory remains authoritative.
            for folder in rpm_roots:
                self.verify_docs(folder)
            source = root / 'UScreen-1.2.3'
            shutil.copytree(docs, source)
            (source / 'target/release').mkdir(parents=True)
            (source / 'host/evdi').mkdir(parents=True)
            for name in ['uscreen', 'uscreen-gui']:
                shutil.copy(docs / 'bin' / name, source / 'target/release' / name)
            shutil.copy(docs / 'bin/evdi_helper', source / 'host/evdi/evdi_helper')
            (root / 'evdi-1.15.0/library').mkdir(parents=True)
            shutil.copy(docs / 'bin/libevdi.so.1.15.0', root / 'evdi-1.15.0/library/')
            run('bash', '-c', 'set -e; source packaging/arch/PKGBUILD; srcdir="$PWD"; pkgdir="$PWD/arch"; package')
            self.verify_docs(root / 'arch/usr/share/doc/uscreen')

    def copy_sources(self, root):
        for name in ['Makefile', 'README.md', 'LICENSE', 'THIRD_PARTY_LICENSES.md', 'SECURITY.md',
                     'CHANGELOG.md', 'CONTRIBUTING.md', 'scripts', 'packaging', 'docs', 'licenses']:
            source = REPO / name
            if not source.exists():
                continue
            if source.is_dir():
                shutil.copytree(source, root / name)
            else:
                shutil.copy(source, root / name)

    def verify_docs(self, folder):
        for name in NOTICES:
            self.assertTrue((folder / name).is_file(), f'missing {folder / name}')
        for link in re.findall(r'\]\(([^)]+)\)', (folder / 'README.md').read_text()):
            if ':' not in link and not link.startswith('#'):
                self.assertTrue((folder / link.split('#')[0]).exists(), f'broken README link: {link}')


if __name__ == '__main__':
    unittest.main()
