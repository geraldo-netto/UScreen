"""T497: the C gate consumes real counters and refuses incomplete or changed evidence."""
import contextlib
import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

import capture_check
import compiler


class CaptureGateTest(unittest.TestCase):
    def test_t497_native_gate_distinguishes_missing_function_coverage(self):
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            root = base/'repo'
            subprocess.run(['git', 'init', '-q', root], check=True)
            source = root/'host/evdi/fixture.c'
            source.parent.mkdir(parents=True)
            source.write_text('int used(void) { return 1; }\nint missed(void) { return 2; }\n')
            driver = root/'driver.c'
            driver.write_text('int used(void); int main(void) { return used() != 1; }\n')
            execute = subprocess.run

            def run(command, **options):
                if command[0] != 'cargo':
                    return execute(command, **options)
                self.assertEqual(command.count('--test'), 4)
                env = options['env']
                executable = base/'fixture'
                args = [str(source), str(driver), '-o', str(executable)]
                self.assertEqual(compiler.compile_with_profile(env['BLENT_REAL_CC'], args,
                    Path(env['BLENT_C_COVERAGE']), root), 0)
                return execute([executable], check=True)

            with patch.object(capture_check, 'ROOT', root), patch.object(subprocess, 'run', side_effect=run), \
                    contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(capture_check.run(base/'incomplete'), 1)
                rows = json.loads((base/'incomplete/report.json').read_text())['functions']
                self.assertEqual({row['name']: row['passes'] for row in rows}, {'used': True, 'missed': False})
                driver.write_text('int used(void); int missed(void); int main(void) { return used()+missed() != 3; }\n')
                with patch.object(sys, 'argv', ['capture', str(base/'complete')]):
                    self.assertEqual(capture_check.main(), 0)
                self.assertTrue(json.loads((base/'complete/report.json').read_text())['passes'])
                with self.assertRaises(FileExistsError): capture_check.run(base/'complete')

    def test_t497_capture_tooling_rejects_missing_compiler_and_empty_inventory(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            subprocess.run(['git', 'init', '-q', root], check=True)
            with patch.object(shutil, 'which', return_value=None):
                with self.assertRaisesRegex(ValueError, 'requires GCC'): capture_check.environment(root)
            with patch.object(capture_check, 'ROOT', root), patch.object(subprocess, 'run'), \
                    contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(capture_check.run(root/'empty'), 1)
            self.assertFalse(json.loads((root/'empty/report.json').read_text())['passes'])
