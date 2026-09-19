"""T497: packaging entry points and cleanup operate only on owned private fixtures."""
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


def cleanup_body(path):
    return re.search(r'(?ms)^cleanup\(\) \{.*?^\}\n', path.read_text()).group()


class ScriptLifecycleTests(unittest.TestCase):
    def test_t497_release_metadata_usage_never_rewrites_files(self):
        documents = [REPO/name for name in ['docs/index.html', 'docs/llms.txt', 'docs/sitemap.xml', 'CITATION.cff']]
        before = [path.read_bytes() for path in documents]
        for arguments in [[], ['--check'], ['1.2.3'], ['1.2.3', '2026-09-19', 'extra']]:
            result = subprocess.run(['bash', str(REPO/'scripts/update-release-metadata.sh'), *arguments], capture_output=True, text=True)
            self.assertEqual(result.returncode, 2)
            self.assertIn('usage:', result.stderr)
        self.assertEqual(before, [path.read_bytes() for path in documents])

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

    def test_t497_gui_cleanup_reaps_owned_children_and_tolerates_empty_state(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            script = root/'fixture.sh'
            script.write_text('set -eu\nGUI_SMOKE_ROOT="$1/owned"\nmkdir "$GUI_SMOKE_ROOT"\n' +
                cleanup_body(REPO/'scripts/ci/gui-smoke.sh') + '''
/bin/sleep 60 & GUI_PID=$!
/bin/sleep 60 & XVFB_PID=$!
trap cleanup EXIT
gui=$GUI_PID
server=$XVFB_PID
cleanup
! kill -0 "$gui" 2>/dev/null
! kill -0 "$server" 2>/dev/null
[[ ! -e $GUI_SMOKE_ROOT ]]
GUI_PID= XVFB_PID=
cleanup
trap - EXIT
''')
            result = subprocess.run(['bash', str(script), str(root)], capture_output=True, text=True, timeout=10)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertFalse((root/'owned').exists())

    def test_t497_appimage_cleanup_stops_its_image_and_reaps_all_owned_children(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            image = root/'fixture-image'
            executable(image, 'printf "%s\\n" "$*" >> "$0.calls"\nexit 17\n')
            script = root/'fixture.sh'
            script.write_text('set -eu\nROOT="$1/owned"\nIMAGE="$1/fixture-image"\nmkdir "$ROOT"\n' +
                cleanup_body(REPO/'scripts/ci/appimage-lifetime-smoke.sh') + '''
/bin/sleep 60 & GUI=$!
/bin/sleep 60 & DAEMON=$!
/bin/sleep 60 & SERVER=$!
trap cleanup EXIT
owned="$GUI $DAEMON $SERVER"
cleanup
for pid in $owned; do ! kill -0 "$pid" 2>/dev/null; done
[[ ! -e $ROOT ]]
GUI= DAEMON= SERVER=
cleanup
trap - EXIT
''')
            result = subprocess.run(['bash', str(script), str(root)], capture_output=True, text=True, timeout=10)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual((root/'fixture-image.calls').read_text(), '--appimage-extract-and-run stop\n' * 2)


if __name__ == '__main__':
    unittest.main()
