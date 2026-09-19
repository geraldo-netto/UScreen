"""T497: normal subprocesses must retain both line and invocation evidence."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch
import contextlib
import io

import script_check
from calls import read_calls
from model import fingerprint
from script_check import environment, python_report, snapshot


class CollectionTest(unittest.TestCase):
    def test_t497_installer_command_overrides_preserve_native_origins(self):
        from shell import merge
        with tempfile.TemporaryDirectory() as name:
            directory = Path(name)
            root = Path(__file__).resolve().parents[2]
            manifest = snapshot(root)
            (directory/'manifest.json').write_text(json.dumps(manifest))
            env = environment(directory, root)
            command = [sys.executable, '-m', 'unittest', 'discover',
                       '-s', str(root/'scripts/tests'), '-p', 'test_installer.py']
            output = subprocess.run(command, env=env, capture_output=True)
            self.assertEqual(output.returncode, 0, output.stderr.decode())
            data = {}
            merge(root, directory/'shell', manifest, data)
            self.assertTrue(any(count for count in data['scripts/install.sh'].values()))

    def test_t497_collector_cli_enforces_gaps_and_rejects_reused_evidence(self):
        with tempfile.TemporaryDirectory() as name:
            base = Path(name)
            root = base/'repo'
            subprocess.run(['git', 'init', '-q', root], check=True)
            (root/'packaging').mkdir()
            source = root/'packaging/main.py'
            source.write_text('def used():\n    return 7\ndef missed():\n    return 9\nassert used() == 7\n')
            commands = [[sys.executable, str(source)]]
            with patch.object(script_check, 'ROOT', root), patch.object(script_check, 'commands', return_value=commands), \
                    contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(script_check.run(base/'strict', False), 1)
                self.assertEqual(script_check.run(base/'reported', True), 0)
                self.assertFalse(json.loads((base/'reported/report.json').read_text())['passes'])
                with self.assertRaises(FileExistsError): script_check.run(base/'strict', True)
                source.write_text(source.read_text() + 'assert missed() == 9\n')
                with patch.object(sys, 'argv', ['collect', str(base/'complete')]):
                    self.assertEqual(script_check.main(), 0)
                self.assertTrue(json.loads((base/'complete/report.json').read_text())['passes'])
            manifest = snapshot(root)
            manifest['version'] = -1
            with self.assertRaises(ValueError): script_check.validate_snapshot(root, manifest)
            manifest = snapshot(root)
            (root/'packaging/new.py').write_text('def new(): return 1\n')
            with self.assertRaises(ValueError): script_check.validate_snapshot(root, manifest)

    def test_t497_collection_keeps_the_regular_suites_and_their_failures(self):
        root = Path(__file__).resolve().parents[2]
        commands = list(script_check.commands(root))
        self.assertEqual(commands[0][-2:], ['--test', 'tooling'])
        self.assertEqual(len(commands), 4)
        with tempfile.TemporaryDirectory() as name:
            base = Path(name)
            source = base/'repo'
            subprocess.run(['git', 'init', '-q', source], check=True)
            (source/'packaging').mkdir()
            (source/'packaging/main.py').write_text('def unused(): return 1\n')
            with patch.object(script_check, 'ROOT', source), patch.object(script_check, 'commands',
                    return_value=[[sys.executable, '-c', 'raise SystemExit(17)']]):
                with self.assertRaises(subprocess.CalledProcessError): script_check.run(base/'evidence', True)
            self.assertFalse((base/'evidence/report.json').exists())

    def test_t497_nested_shell_fixtures_keep_python_tracer_provenance(self):
        with tempfile.TemporaryDirectory() as name:
            directory = Path(name)
            root = Path(__file__).resolve().parents[2]
            (directory/'manifest.json').write_text(json.dumps(snapshot(root)))
            env = environment(directory, root)
            command = [sys.executable, '-m', 'unittest', 'discover',
                       '-s', str(root/'scripts/coverage'), '-p', 'test_shell.py']
            output = subprocess.run(command, env=env, capture_output=True)
            self.assertEqual(output.returncode, 0, output.stderr.decode())
            python_report(directory, root, env)
            report = json.loads((directory/'python.json').read_text())
            self.assertTrue(any(path.endswith('shell.py') for path in report['files']))

    def test_t497_subprocess_hook_handles_installed_coverage_startup_and_copies(self):
        with tempfile.TemporaryDirectory() as name:
            directory = Path(name)
            root = directory/'repo'
            root.mkdir()
            source = root/'a.py'
            source.write_text('def called():\n    return 7\ndef missed(): return 8\nassert called() == 7\n')
            duplicate = directory/'copy.py'
            duplicate.write_bytes(source.read_bytes())
            manifest = {'sources': {'a.py': fingerprint(source)}}
            (directory/'manifest.json').write_text(json.dumps(manifest))
            env = environment(directory, root)
            for executable in {sys.executable, '/usr/bin/python3'}:
                for target in [source, duplicate]:
                    process = subprocess.run([executable, target], env=env, capture_output=True)
                    self.assertEqual(process.returncode, 0, process.stderr.decode())
                    self.assertNotIn(b'Error processing', process.stderr)
            duplicate.unlink()
            python_report(directory, root, env)
            observed = read_calls(directory/'calls')
            self.assertIn(('a.py', 1, 'called'), observed)
            self.assertNotIn(('a.py', 3, 'missed'), observed)
            report = json.loads((directory/'python.json').read_text())
            record = next(value for key, value in report['files'].items() if key.endswith('a.py'))
            self.assertIn(2, record['executed_lines'])

    def test_t497_hook_rejects_missing_manifest_instead_of_silently_losing_data(self):
        with tempfile.TemporaryDirectory() as name:
            directory = Path(name)
            env = environment(directory, directory)
            child = subprocess.run([sys.executable, '-c', 'raise AssertionError("must not run")'],
                                   env=env, capture_output=True)
            self.assertEqual(child.returncode, 86)
            self.assertIn(b'UScreen coverage startup failed', child.stderr)
