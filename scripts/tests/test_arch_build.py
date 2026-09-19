"""T497: Arch source builds preserve the target and replaceable-library contract."""
import os
from pathlib import Path
import re
import subprocess
import tempfile
import unittest

REPO = Path(__file__).resolve().parents[2]


def executable(path, body):
    path.write_text('#!/bin/bash\n' + body)
    path.chmod(0o700)


class ArchBuildTest(unittest.TestCase):
    def test_t497_arch_build_uses_one_target_directory_and_replaceable_evdi_library(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            package = REPO/'packaging/arch/PKGBUILD'
            version = re.search(r'^pkgver=(.+)$', package.read_text(), re.M)[1]
            source = root/('UScreen-' + version)
            (source/'host/evdi').mkdir(parents=True)
            (root/'evdi-1.15.0/library').mkdir(parents=True)
            bin = root/'bin'
            bin.mkdir()
            for name in ['cargo', 'make', 'gcc']:
                executable(bin/name, 'printf "%s|%s|%s\\n" "${0##*/}" "$PWD" "$*" >> "$T497_COMMANDS"\n')
            environment = dict(os.environ, PATH=str(bin) + os.pathsep + os.environ['PATH'], T497_COMMANDS=str(root/'commands'))
            result = subprocess.run(['bash', '-c', 'set -e; source "$1"; srcdir="$2"; build', 't497', str(package), str(root)],
                                    cwd=root, env=environment, capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            commands = (root/'commands').read_text().splitlines()
            self.assertEqual(len(commands), 4)
            self.assertTrue(all(f'|{source}|' in command for command in commands))
            self.assertIn('--target-dir target --manifest-path host/Cargo.toml', commands[0])
            self.assertIn('--target-dir target --manifest-path gui/Cargo.toml', commands[1])
            self.assertIn('evdi-1.15.0/library', commands[2])
            self.assertIn('-levdi -lpthread -Wl,-rpath,$ORIGIN', commands[3])
            self.assertIn('host/evdi/frame_exchange.c host/evdi/fifo_writer.c', commands[3])


if __name__ == '__main__':
    unittest.main()
