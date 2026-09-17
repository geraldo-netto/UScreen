"""T102: local distributions need an APK and a loadable bundled helper."""
from pathlib import Path
import os
import shutil
import subprocess
import tempfile
import unittest

import test_notices


class DistributionTest(unittest.TestCase):
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
                library = root / 'libs/libevdi.so.1.15.0'
                library.parent.mkdir()
                subprocess.run(['cc', '-shared', '-fPIC', '-Wl,-soname,libevdi.so.1', 'library.c', '-o', str(library)], cwd=root, check=True)
                (library.parent / 'libevdi.so').symlink_to(library.name)
                (library.parent / 'libevdi.so.1').symlink_to(library.name)
                if mode == 'success':
                    write('android/app/build/outputs/apk/release/app-release.apk', 'apk')
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
                    shutil.rmtree(library.parent)
                    env.pop('LIBRARY_PATH')
                    env.pop('LD_LIBRARY_PATH', None)
                    helper = artifact / f'uscreen-{version}/bin/evdi_helper'
                    loaded = subprocess.run([str(helper)], env=env, capture_output=True, text=True)
                    self.assertEqual(loaded.returncode, 0, loaded.stderr)
                    self.assertTrue((artifact / f'uscreen-{version}/uscreen.apk').is_file())
                    # T129: the local tarball carries the same notices and working links.
                    test_notices.NoticeTest().verify_docs(artifact / f'uscreen-{version}')


if __name__ == '__main__':
    unittest.main()
