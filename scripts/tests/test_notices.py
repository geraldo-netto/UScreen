"""T129: shipped documentation and license links resolve in each artifact."""
from pathlib import Path
import os
import re
import shutil
import subprocess
import tempfile
import unittest
import release_signing_fixture

REPO = Path(__file__).resolve().parents[2]
NOTICES = ['LICENSE', 'THIRD_PARTY_LICENSES.md', 'licenses/libevdi-LGPL-2.1.txt']


class NoticeTest(unittest.TestCase):
    def test_t129_tar_appimage_rpm_and_arch_include_notices(self):
        with tempfile.TemporaryDirectory(prefix='uscreen-notices-') as tmp:
            root = Path(tmp)
            version = self.copy_sources(root)
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
            write('bin/readelf', '#!/bin/sh\ncat << EOF\n(RUNPATH) Library runpath: [\\$ORIGIN]\n(NEEDED) Shared library: [libevdi.so.1]\nEOF\n', True)
            write('bin/fakeroot', '#!/bin/sh\nexec "$@"\n', True)
            write('android/gradlew', '#!/bin/sh\nexit 0\n', True)
            write('android/app/build/outputs/apk/release/app-release.apk', 'apk')
            release_signing_fixture.install_tools(root / 'bin')
            env = dict(os.environ, PATH=f'{root}/bin:{os.environ["PATH"]}', USCREEN_TEST_BUILD='portable')
            def run(*args):
                result = subprocess.run(args, cwd=root, env=env, capture_output=True, text=True)
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                return result.stdout
            run('bash', 'scripts/build-release.sh')
            extracted = root / 'unpacked'
            extracted.mkdir()
            run('tar', '-xf', f'dist/uscreen-{version}-linux-x86_64.tar.gz', '-C', str(extracted))
            docs = extracted / f'uscreen-{version}'
            self.verify_docs(docs)
            self.assertEqual((docs / 'scripts/setup-evdi.sh').read_bytes(), (REPO / 'scripts/setup-evdi.sh').read_bytes(), 'T269: portable setup missing')
            env['USCREEN_TEST_BUILD'] = 'packages'
            import appimage_fixture
            appimage_fixture.install(root)
            run('bash', 'packaging/build-packages.sh')
            self.verify_docs(root / 'dist/appimage-docs-fixture')
            self.assertEqual((root / 'dist/appimage-docs-fixture/scripts/setup-evdi.sh').read_bytes(),
                             (REPO / 'scripts/setup-evdi.sh').read_bytes(), 'T269: AppImage setup missing')
            rpm_files = run('rpm', '-qpl', f'dist/uscreen-{version}-1.x86_64.rpm').splitlines()
            self.assertIn('/usr/share/uscreen/setup-evdi.sh', rpm_files, 'T269: RPM setup missing')
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
            source = root / f'UScreen-{version}'
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
            self.assertEqual((root / 'arch/usr/share/uscreen/setup-evdi.sh').read_bytes(), (REPO / 'scripts/setup-evdi.sh').read_bytes(), 'T269: Arch setup missing')

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

        # T302: expectations follow the copied source version, not this release.
        return re.search(r'^VERSION = (\S+)$', (root / 'Makefile').read_text(), re.M)[1]

    def verify_docs(self, folder):
        for name in NOTICES:
            self.assertTrue((folder / name).is_file(), f'missing {folder / name}')
        for link in re.findall(r'\]\(([^)]+)\)', (folder / 'README.md').read_text()):
            if ':' not in link and not link.startswith('#'):
                self.assertTrue((folder / link.split('#')[0]).exists(), f'broken README link: {link}')


if __name__ == '__main__':
    unittest.main()
