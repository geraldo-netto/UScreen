"""T497: coverage must survive the existing harness deleting its build directory."""
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
import os
import sys
from unittest.mock import patch

import compiler
import readers


class CompilerTest(unittest.TestCase):
    def test_t497_compiler_entry_preserves_status_and_rejects_recursion(self):
        env = dict(USCREEN_C_COVERAGE=str(self.root/'native'), USCREEN_REAL_CC='/bin/false')
        with patch.dict(os.environ, env), patch.object(sys, 'argv', ['collector', '--version']):
            self.assertEqual(compiler.main(), 1)
            with patch.dict(os.environ, USCREEN_REAL_CC=str(Path('collector').resolve())):
                with self.assertRaisesRegex(ValueError, 'recursively'): compiler.main()

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.report = self.root / 'report'

    def test_t497_real_counters_survive_harness_cleanup(self):
        source = self.root / 'source.c'
        source.write_text('int used(int x) { return x + 1; }\nint missed(void) { return 9; }\nint main(void) { return used(0) != 1; }\n')
        build = self.root / 'build'
        build.mkdir()
        binary = build / 'fixture'
        self.assertEqual(compiler.compile_with_profile('/usr/bin/cc', [str(source), '-o', str(binary)], self.report), 0)
        subprocess.run([binary], check=True)
        shutil.rmtree(build)
        paths = compiler.export_reports(self.report)
        self.assertEqual(len(paths), 1)
        data = {}
        readers.gcov_json(paths[0], self.root, data)
        self.assertGreater(data['source.c'][1], 0)
        self.assertEqual(data['source.c'][2], 0)

    def test_t497_noncompilations_and_failures_preserve_compiler_results(self):
        self.assertIsNone(compiler.compilation(['--version']))
        source = self.root / 'bad.c'
        source.write_text('invalid C syntax')
        self.assertIsNone(compiler.compilation([str(source), '-o']))
        with patch.object(compiler.subprocess, 'run') as run:
            run.return_value.returncode = 7
            self.assertEqual(compiler.compile_with_profile('/usr/bin/cc', ['--version'], self.report), 7)
            self.assertEqual(compiler.compile_with_profile('/usr/bin/cc', [str(source), '-o', str(self.root/'binary')], self.report), 7)
        with self.assertRaises(ValueError): compiler.capture_notes(self.root/'missing', self.root)

    def test_t497_ambiguous_profile_names_are_rejected(self):
        note = self.root / 'file.gcno'
        note.touch()
        for name in ['one-file.gcda', 'two-file.gcda']:
            (self.root / name).touch()
        with self.assertRaises(ValueError): compiler.pair_counter(note, self.root)
        self.assertEqual(compiler.export_reports(self.root), [])

    def test_t497_dependency_builds_never_receive_gcov_link_requirements(self):
        dependency = self.root/'dependency.c'
        dependency.write_text('int foreign(void) { return 0; }\n')
        owned = self.root/'host/evdi/source.c'
        owned.parent.mkdir(parents=True)
        owned.write_bytes(dependency.read_bytes())
        self.assertTrue(compiler.owned_compilation([str(owned)], self.root))
        self.assertFalse(compiler.owned_compilation([str(dependency)], self.root))
        with patch.object(compiler.subprocess, 'run') as run:
            run.return_value.returncode = 0
            compiler.compile_with_profile('cc', [str(dependency), '-o', 'foreign'], self.report, self.root)
            self.assertEqual(run.call_args.args[0], ['cc', str(dependency), '-o', 'foreign'])


if __name__ == '__main__':
    unittest.main()
