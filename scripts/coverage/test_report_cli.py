"""T497: the report command enforces application function coverage and immutable sources."""
import contextlib
import io
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

import report


class ReportCommandTest(unittest.TestCase):
    def command(self, root, *args):
        with patch.object(report, 'ROOT', root), patch.object(sys, 'argv', ['coverage', *map(str, args)]), \
                contextlib.redirect_stdout(io.StringIO()):
            return report.main()

    def fixture(self, root):
        subprocess.run(['git', 'init', '-q', str(root)], check=True)
        (root/'a.rs').write_text('fn first() {\n  work();\n}\nfn second() {\n  work();\n}\n')
        (root/'b.rs').write_text('fn other() {\n    work();\n}\n')
        manifest = root/'manifest.json'
        self.command(root, 'snapshot', manifest)
        (root/'rust.lcov').write_text('SF:a.rs\nDA:1,1\nDA:2,1\nDA:4,0\nDA:5,0\nend_of_record\nSF:b.rs\nDA:2,1\nend_of_record\n')
        return ['check', '--manifest', manifest, '--output', root/'report.json', '--lcov', root/'rust.lcov']

    def test_t497_cli_distinguishes_scope_reporting_missing_functions_and_success(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            args = self.fixture(root)
            self.assertEqual(self.command(root, *args), 1)
            self.assertEqual(self.command(root, *args, '--report-only'), 0)
            self.assertFalse(json.loads((root/'report.json').read_text())['passes'])
            self.assertEqual(self.command(root, *args, '--scope', 'b.rs'), 0)
            (root/'rust.lcov').write_text('SF:a.rs\nDA:1,1\nDA:2,1\nDA:4,1\nDA:5,1\nend_of_record\nSF:b.rs\nDA:2,1\nend_of_record\n')
            self.assertEqual(self.command(root, *args), 0)
            self.assertTrue(json.loads((root/'report.json').read_text())['passes'])
            (root/'a.rs').write_text('fn changed() {}\n')
            with self.assertRaises(ValueError): self.command(root, *args)

    def test_t497_cli_rejects_a_new_function_file_and_unsupported_manifest(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            args = self.fixture(root)
            (root/'new.rs').write_text('fn new() {}\n')
            with self.assertRaisesRegex(ValueError, 'inventory changed'): self.command(root, *args)
            (root/'new.rs').unlink()
            manifest = json.loads((root/'manifest.json').read_text())
            manifest['version'] = -1
            (root/'manifest.json').write_text(json.dumps(manifest))
            with self.assertRaisesRegex(ValueError, 'unsupported'): self.command(root, *args)
