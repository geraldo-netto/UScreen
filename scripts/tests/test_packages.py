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
                assets = self.create_fixture(root)
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

    def test_t235_checkout_paths_are_literal_container_arguments(self):
        for name in ["project with spaces", "project'quote", 'project$(touch INJECTED)`touch INJECTED`']:
            with self.subTest(name=name), tempfile.TemporaryDirectory(prefix='uscreen-quoting-') as tmp:
                root = Path(tmp) / name
                root.mkdir()
                self.create_fixture(root)
                # Execute the actual inner program and preserve its positional arguments.
                stub = root / 'bin/distrobox'
                stub.write_text('#!/bin/bash\nshift 3\n[ "$1" = bash ] && shift\n[ "$1" = -lc ] && shift\nexec bash -c "$@"\n')
                env = dict(os.environ, PATH=f'{root}/bin:{os.environ["PATH"]}', USCREEN_TEST_MODE='success')
                result = subprocess.run(['bash', 'packaging/build-packages.sh'], cwd=root,
                                        env=env, capture_output=True, text=True, timeout=20)
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                self.assertFalse((root / 'INJECTED').exists())

    def test_t235_portable_build_uses_literal_checkout_path(self):
        from test_notices import NoticeTest
        for name in ["project with spaces", "project'quote", 'project$(touch INJECTED)`touch INJECTED`']:
            with self.subTest(name=name), tempfile.TemporaryDirectory(prefix='uscreen-build-path-') as tmp:
                root = Path(tmp) / name
                root.mkdir()
                version = NoticeTest().copy_sources(root)
                self.portable_fixture(root)
                env = dict(os.environ, PATH=f'{root}/bin:{os.environ["PATH"]}', HOME=str(root / 'home'))
                result = subprocess.run(['bash', 'scripts/build-release.sh'], cwd=root,
                                        env=env, capture_output=True, text=True, timeout=20)
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                self.assertTrue((root / f'dist/uscreen-{version}-linux-x86_64.tar.gz').is_file())
                self.assertFalse((root / 'INJECTED').exists())

    def test_t236_release_rejects_new_glibc_in_bundled_evdi(self):
        from test_notices import NoticeTest
        with tempfile.TemporaryDirectory(prefix='uscreen-abi-') as tmp:
            root = Path(tmp)
            version = NoticeTest().copy_sources(root)
            self.portable_fixture(root)
            (root / 'bin/objdump').write_text('#!/bin/sh\ncase "$2" in *libevdi*) echo GLIBC_2.37;; *) echo GLIBC_2.36;; esac\n')
            env = dict(os.environ, PATH=f'{root}/bin:{os.environ["PATH"]}', HOME=str(root / 'home'))
            result = subprocess.run(['bash', 'scripts/build-release.sh'], cwd=root,
                                    env=env, capture_output=True, text=True)
            self.assertNotEqual(result.returncode, 0, 'T236: shipped library exceeds the documented ABI floor')
            self.assertFalse((root / f'dist/uscreen-{version}-linux-x86_64.tar.gz').exists())

    def portable_fixture(self, root):
        files = {
            'bin/distrobox': '#!/bin/bash\nshift 3; shift 2; exec bash -c "$@"\n',
            'bin/objdump': '#!/bin/sh\necho GLIBC_2.36\n',
            'bin/readelf': '#!/bin/sh\ncat << EOF\n(RUNPATH) Library runpath: [\\$ORIGIN]\n(NEEDED) Shared library: [libevdi.so.1]\nEOF\n',
            'android/gradlew': '#!/bin/sh\nexit 0\n',
            'android/app/build/outputs/apk/release/app-release.apk': 'apk',
        }
        for name in ['cargo', 'make', 'gcc']:
            files['bin/' + name] = '#!/bin/sh\nexit 0\n'
        for name in ['release/uscreen', 'release/uscreen-gui', 'evdi_helper', 'evdi-src/library/libevdi.so.1.15.0']:
            files['target-deb12/' + name] = '#!/bin/sh\nexit 0\n'
        for name, text in files.items():
            path = root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(text)
            path.chmod(0o755)

    def test_t235_make_preserves_release_notes_path(self):
        with tempfile.TemporaryDirectory(prefix='uscreen-notes-path-') as tmp:
            root = Path(tmp)
            shutil.copy(REPO / 'Makefile', root)
            (root / 'scripts').mkdir()
            publisher = root / 'scripts/publish-release.sh'
            publisher.write_text('#!/bin/sh\nprintf "%s\n" "$#" "$@" > arguments\n')
            publisher.chmod(0o755)
            notes = "notes with 'quotes' $(touch INJECTED) `touch INJECTED`.md"
            result = subprocess.run(['make', 'publish', 'NOTES=' + notes], cwd=root,
                                    capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            self.assertEqual((root / 'arguments').read_text().splitlines(), ['1', notes])
            self.assertFalse((root / 'INJECTED').exists())

    def create_fixture(self, root):
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
        for name in ['scripts/setup-evdi.sh', 'scripts/uscreen.desktop', 'packaging/icons/uscreen.svg', 'packaging/icons/uscreen-pen.svg',
                     'packaging/uscreen.service', 'packaging/uscreen-evdi.conf', 'packaging/uscreen-modules.conf',
                     'packaging/60-uscreen-uinput.rules']:
            write(name, 'fixture')
        assets = ['uscreen_1.2.3_amd64.deb', 'uscreen-1.2.3-1.x86_64.rpm', 'uscreen-1.2.3-PKGBUILD.tar.gz']
        for name in assets + ['uscreen-1.2.3-linux-x86_64.tar.gz', '.packages-ok']:
            write('dist/' + name, 'stale')
        write('bin/distrobox', '#!/bin/bash\nif [ "$USCREEN_TEST_MODE" != container ]; then shift 3; shift 2; bash -c "$@"; fi\nexit 0\n', True)
        write('bin/fakeroot', '#!/bin/sh\nexec "$@"\n', True)
        write('bin/dpkg-deb', '#!/bin/bash\nif [ "$1" = --info ]; then echo "Package: uscreen"; else echo new > "${@: -1}"; fi\n', True)
        write('bin/rpmbuild', '#!/bin/sh\nRB=${2#_topdir }\nmkdir -p "$RB/RPMS/x86_64"\necho new > "$RB/RPMS/x86_64/uscreen-1.2.3-1.x86_64.rpm"\necho Wrote\n[ "$USCREEN_TEST_MODE" != rpm ]\n', True)
        return assets


if __name__ == '__main__':
    unittest.main()
