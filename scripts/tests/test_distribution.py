"""T102: local distributions need an APK and a loadable bundled helper."""
from pathlib import Path
import os
import shutil
import subprocess
import tempfile
import unittest

import test_notices


def tree_contents(root):
    entries = {}
    for path in root.rglob('*'):
        mode = path.lstat().st_mode
        if path.is_symlink():
            content = ('link', os.readlink(path))
        elif path.is_file():
            content = ('file', path.read_bytes())
        else:
            content = ('directory', None)
        entries[str(path.relative_to(root))] = (mode, content)
    return entries


class DistributionTest(unittest.TestCase):
    def compare_shared_bundle(self, root, local, library):
        staged = root / 'shared bundle'
        subprocess.run(['bash', 'scripts/stage-linux-bundle.sh', 'target/release',
                        'host/evdi/evdi_helper', str(library), str(staged)], cwd=root, check=True)
        expected = tree_contents(local)
        # APK production belongs to the caller; CI Linux package fixtures omit it.
        expected.pop('uscreen.apk')
        self.assertEqual(tree_contents(staged), expected, 'T379: bundle layouts diverged')
        self.assertEqual(os.readlink(staged / 'bin/libevdi.so.1'), 'libevdi.so.1.15.0')
        self.assertFalse((staged / 'bin/libevdi.so.1.15.0').is_symlink())
        test_notices.NoticeTest().verify_docs(staged)

    def test_t102_apk_failure_and_bundled_helper_loading(self):
        for mode in ['failed-apk', 'missing-apk', 'success']:
            with self.subTest(mode=mode), tempfile.TemporaryDirectory(prefix='uscreen-dist-') as tmp:
                root = Path(tmp)
                version = test_notices.NoticeTest().copy_sources(root)
                def write(name, body, executable=False):
                    path = root / name
                    path.parent.mkdir(parents=True, exist_ok=True)
                    path.write_text(body)
                    if executable:
                        path.chmod(0o755)
                    return path
                write('target/release/uscreen', '#!/bin/sh\nexit 0\n', True)
                write('target/release/uscreen-gui', '#!/bin/sh\nexit 0\n', True)
                write('library.c', 'int uscreen_distribution_probe(void) { return 0; }\n')
                write('host/evdi/evdi_helper.c', 'extern int uscreen_distribution_probe(void); int main(void) { return uscreen_distribution_probe(); }\n')
                # T380: stub the other C units too; this fixture checks loader
                # paths and packaging. evdi_modules tests the real module link.
                for module in ['conversion', 'frame_exchange', 'fifo_writer', 'capture', 'writer']:
                    write(f'host/evdi/{module}.c', '/* distribution fixture */\n')
                library = root / "library's space/libevdi.so.1.15.0"
                library.parent.mkdir()
                subprocess.run(['cc', '-shared', '-fPIC', '-Wl,-soname,libevdi.so.1', 'library.c', '-o', str(library)], cwd=root, check=True)
                (library.parent / 'libevdi.so').symlink_to(library.name)
                (library.parent / 'libevdi.so.1').symlink_to(library.name)
                if mode == 'success':
                    write('android/app/build/outputs/apk/release/app-release.apk', 'apk')
                    library.rename(library.parent / 'actual.so')
                    library.symlink_to('actual.so')
                write('android/gradlew', '#!/bin/sh\nexit ' + ('42' if mode == 'failed-apk' else '0') + '\n', True)
                env = dict(os.environ, LIBRARY_PATH=str(library.parent))
                result = subprocess.run(['make', 'dist-local', 'CARGO=true', f'LIBEVDI={library}'], cwd=root, env=env, capture_output=True, text=True)
                if mode != 'success':
                    self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
                else:
                    self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                    # Extract and remove build-time library search paths entirely.
                    artifact = root / 'extracted'
                    artifact.mkdir()
                    subprocess.run(['tar', '-xf', f'dist/uscreen-{version}-linux-x86_64.tar.gz', '-C', str(artifact)], cwd=root, check=True)
                    self.compare_shared_bundle(root, artifact / f'uscreen-{version}', library)
                    shutil.rmtree(library.parent)
                    env.pop('LIBRARY_PATH')
                    env.pop('LD_LIBRARY_PATH', None)
                    helper = artifact / f'uscreen-{version}/bin/evdi_helper'
                    loaded = subprocess.run([str(helper)], env=env, capture_output=True, text=True)
                    self.assertEqual(loaded.returncode, 0, loaded.stderr)
                    self.assertTrue((artifact / f'uscreen-{version}/uscreen.apk').is_file())
                    # T129: the local tarball carries the same notices and working links.
                    test_notices.NoticeTest().verify_docs(artifact / f'uscreen-{version}')

    def test_t379_missing_bundle_inputs_cannot_produce_an_archive(self):
        inputs = ['target/release/uscreen', 'target/release/uscreen-gui',
                  'host/evdi/evdi_helper', 'library/libevdi.so.1.15.0']
        for missing in inputs:
            with self.subTest(missing=missing), tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp)
                version = test_notices.NoticeTest().copy_sources(root)
                for name in inputs:
                    path = root / name
                    path.parent.mkdir(parents=True, exist_ok=True)
                    path.write_text('fixture')
                    path.chmod(0o751)
                (root / missing).unlink()
                commands = [
                    ['bash', 'scripts/stage-linux-bundle.sh', 'target/release',
                     'host/evdi/evdi_helper', 'library/libevdi.so.1.15.0', 'staged'],
                    ['make', 'dist-local', 'CC=true', 'CARGO=true',
                     'LIBEVDI=library/libevdi.so.1.15.0'],
                ]
                for command in commands:
                    result = subprocess.run(command, cwd=root, capture_output=True, text=True)
                    self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
                self.assertFalse((root / f'dist/uscreen-{version}-linux-x86_64.tar.gz').exists())


if __name__ == '__main__':
    unittest.main()
