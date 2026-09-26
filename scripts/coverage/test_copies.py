"""T497: copied fixtures count only when their complete source is identical."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import Mock, patch

import copies
from model import fingerprint


class CopyTest(unittest.TestCase):
    def test_t497_plugin_registration_keeps_native_python_reporting(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root/'a.py').write_text('def used(): return 1\n')
            manifest = root/'manifest.json'
            manifest.write_text(json.dumps(dict(sources={'a.py': fingerprint(root/'a.py')})))
            registry = Mock()
            with patch.dict(os.environ, BLENT_COVERAGE_ROOT=str(root), BLENT_COVERAGE_MANIFEST=str(manifest)):
                copies.coverage_init(registry, {})
            plugin = registry.add_file_tracer.call_args.args[0]
            self.assertEqual(plugin.file_reporter(str(root/'a.py')), 'python')
            self.assertEqual(plugin.file_tracer(str(root/'a.py')).source_filename(), str(root/'a.py'))

    def test_t497_identical_copies_keep_coverage_after_the_sandbox_is_deleted(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root/'repo'
            source.mkdir()
            original = source/'fixture.py'
            original.write_text('def used():\n    return 7\ndef unused():\n    return 8\nused()\n')
            duplicate = root/'copy.py'
            duplicate.write_bytes(original.read_bytes())
            manifest = root/'manifest.json'
            manifest.write_text(json.dumps({'sources': {'fixture.py': fingerprint(original)}}))
            config = root/'coverage.ini'
            config.write_text(f'[run]\nplugins = copies\nsource = {source}\ndata_file = {root}/.coverage\n')
            env = dict(os.environ, BLENT_COVERAGE_ROOT=str(source), BLENT_COVERAGE_MANIFEST=str(manifest),
                       PYTHONPATH=str(Path(__file__).parent.resolve()))
            env.pop('COVERAGE_PROCESS_START', None)
            env.pop('BLENT_PYTHON_CALLS', None)
            command = [sys.executable, '-m', 'coverage']
            subprocess.run([*command, 'run', '--rcfile='+str(config), str(duplicate)], env=env, check=True)
            duplicate.unlink()
            report = root/'report.json'
            subprocess.run([*command, 'json', '--rcfile='+str(config), '-o', str(report)], env=env, check=True, stdout=subprocess.DEVNULL)
            record = next(iter(json.loads(report.read_text())['files'].values()))
            self.assertIn(2, record['executed_lines'])
            self.assertIn(4, record['missing_lines'])

    def test_t497_changed_and_ambiguous_copies_are_never_credited(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root/'repo'
            source.mkdir()
            original = source/'a.py'
            original.write_text('print(1)\n')
            manifest = {'a.py': fingerprint(original)}
            plugin = copies.Copies(source, manifest)
            self.assertEqual(plugin.file_tracer(str(original)).source_filename(), str(original))
            duplicate = root/'copy.py'
            duplicate.write_text('print(2)\n')
            self.assertIsNone(plugin.file_tracer(str(duplicate)))
            self.assertIsNone(plugin.file_tracer(str(root/'absent.py')))
            duplicate.write_bytes(original.read_bytes())
            self.assertEqual(plugin.file_tracer(str(duplicate)).source_filename(), str(original))
            (source/'b.py').write_bytes(original.read_bytes())
            manifest['b.py'] = manifest['a.py']
            self.assertIsNone(copies.Copies(source, manifest).file_tracer(str(duplicate)))
