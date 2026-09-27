"""T652: permanent evidence/isolation regressions for bounded fault campaigns."""
import argparse
import json
import os
from pathlib import Path
import sys
import tempfile
import time
import unittest
from unittest.mock import patch
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'mutation'))
import bounded
import isolation


def done(code=0):
    return dict(status='completed', code=code)


def row(**extra):
    return dict(id='T652', profile='unit', file='policy.py', before='return 1', after='return 2',
                expected_failure='test_contract', **extra)


class BoundedEvidenceTest(unittest.TestCase):
    def test_t652_build_test_timeout_and_expected_assertion_are_distinct(self):
        observed = dict(executed=1, failures=['test_contract'])
        self.assertEqual(bounded.classify(done(), done(1), observed, 'test_contract'), 'caught')
        self.assertEqual(bounded.classify(done(), done(1), observed, 'unrelated'), 'tool_error')
        self.assertEqual(bounded.classify(done(1), {}, observed, 'test_contract'), 'unviable')
        self.assertEqual(bounded.classify(done(), done(), dict(executed=1, failures=[]), ''), 'survived')
        for status in ['timeout', 'tool_error']:
            self.assertEqual(bounded.classify(dict(status=status), {}, observed, ''), status)
            self.assertEqual(bounded.classify(done(), dict(status=status), observed, ''), status)
        self.assertEqual(bounded.classify(done(), done(), dict(executed=0, failures=[]), ''), 'tool_error')
        self.assertEqual(bounded.classify(done(), done(), observed, ''), 'tool_error')

    def test_t652_ambiguous_noop_and_escaping_edits_are_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            file = root / 'policy.py'
            for text in ['return 1\nreturn 1', 'return 0']:
                file.write_text(text)
                with self.assertRaises(ValueError): bounded.edited(root, row())
            file.write_text('return 1')
            for changes in [dict(file='../policy.py'), dict(file=str(file)), dict(before=''), dict(after='return 1')]:
                with self.assertRaises(ValueError): bounded.edited(root, row() | changes)

    def test_t652_test_evidence_requires_executed_tests_and_known_failure(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp); log = root / 'log'
            log.write_text('test abc::test_contract ... FAILED\ntest result: FAILED. 2 passed; 1 failed;\n')
            self.assertEqual(bounded.test_evidence(root, dict(format='rust'), log),
                             dict(executed=3, failures=['abc::test_contract']))
            log.write_text('FAIL: test_contract (tests.Contract)\nRan 4 tests in 0.1s\nFAILED (failures=1)\n')
            self.assertEqual(bounded.test_evidence(root, dict(format='unittest'), log),
                             dict(executed=4, failures=['test_contract']))
            log.write_text('compiler failed before test execution')
            self.assertEqual(bounded.test_evidence(root, dict(format='rust'), log)['executed'], 0)
            self.assertEqual(bounded.test_evidence(root, dict(format='unittest'), log)['executed'], 0)

    def test_t652_interleaved_native_stderr_keeps_failed_test_identity(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp); log = root/'log'
            log.write_text('test crate::test_contract ... warning: native compiler\n\nFAILED\n'
                           'failures:\n    crate::test_contract\n\n'
                           'test result: FAILED. 4 passed; 1 failed; 0 ignored;\n')
            result = bounded.test_evidence(root, dict(format='rust'), log)
            self.assertEqual(result, dict(executed=5, failures=['crate::test_contract']))
            self.assertEqual(bounded.classify(done(), done(101), result, 'test_contract'), 'caught')

    def test_t652_junit_stale_empty_and_skipped_results_cannot_pass(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp); report = root / 'TEST-unit.xml'
            report.write_text('<testsuite><testcase name="skip"><skipped/></testcase>'
                              '<testcase name="test_contract"><failure/></testcase></testsuite>')
            self.assertEqual(bounded.junit(root, 'TEST-*.xml'), (1, ['test_contract']))
            selected = dict(junit='TEST-*.xml')
            bounded.retain_reports(root, root / 'retained', selected)
            bounded.clear_reports(root, selected)
            self.assertEqual(bounded.junit(root, 'TEST-*.xml'), (0, []))
            self.assertTrue((root/'retained/TEST-unit.xml').is_file())

    def test_t652_candidate_restores_source_on_success_and_exception(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp); source = root/'source'; source.mkdir()
            file = source/'policy.py'; file.write_bytes('# café\r\nreturn 1\r\n'.encode())
            original = file.read_bytes()
            observed = (done(), done(1), dict(executed=1, failures=['test_contract']))
            with patch.object(bounded, 'phases', return_value=observed):
                result = bounded.candidate(row(), source, root, {}, {}, None)
            self.assertEqual(result['outcome'], 'caught')
            self.assertEqual(file.read_bytes(), original)
            with patch.object(bounded, 'phases', side_effect=RuntimeError('interrupted')):
                with self.assertRaises(RuntimeError): bounded.candidate(row(), source, root, {}, {}, None)
            self.assertEqual(file.read_bytes(), original)
            self.assertIn('+return 2', (root/'T652.diff').read_text(encoding='utf-8'))

    def test_t652_partial_failed_baseline_and_changed_sources_fail_closed(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp); file = root/'policy.py'; file.write_text('return 1\n')
            hashes = {'policy.py': bounded.fingerprint(file)}
            baseline = (done(), done(), dict(executed=1, failures=[]))
            selected = dict(candidate_count=1); caught = [dict(id='T652', outcome='caught')]
            self.assertTrue(bounded.report(selected, caught, baseline, root, hashes)['passes'])
            self.assertFalse(bounded.report(selected, [], baseline, root, hashes)['passes'])
            self.assertFalse(bounded.report(selected, caught, (done(1), {}, {}), root, hashes)['passes'])
            file.write_text('modified by test')
            self.assertFalse(bounded.report(selected, caught, baseline, root, hashes)['passes'])

    def test_t652_deadline_prevents_launch_and_argv_keeps_literal_paths(self):
        with tempfile.TemporaryDirectory() as tmp, patch.object(bounded, 'execute') as execute:
            output = Path(tmp)
            result = bounded.phase(['missing'], output, output, {}, 'log', 1, time.monotonic()-1)
            self.assertEqual(result['status'], 'timeout'); execute.assert_not_called()
            self.assertEqual(bounded.invocation(['{source}/literal & spaces', '{python}'], output, output),
                             [str(output)+'/literal & spaces', sys.executable])

    def test_t652_catalog_rejects_empty_duplicate_foreign_and_unsafe_ids(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp)/'catalog.json'
            base = dict(version=1, profiles=dict(unit=dict(platforms=[sys.platform])))
            for rows in [[], [row(), row()], [row() | dict(id='../escape')], [row() | dict(expected_failure='')]]:
                path.write_text(json.dumps(base | dict(mutations=rows)))
                with self.assertRaises(ValueError): bounded.catalog(path, 'unit', sys.platform)
            path.write_text(json.dumps(base | dict(mutations=[row()])))
            with self.assertRaises(ValueError): bounded.catalog(path, 'unit', 'foreign')
            self.assertEqual(len(bounded.catalog(path, 'unit', sys.platform)[1]), 1)

    def test_t652_windows_environment_removes_inherited_measurement_hooks(self):
        original = dict(PATH='preserved', HOME='preserved', **{key: 'external' for key in isolation.MEASUREMENT_ENV})
        with patch.object(isolation.sys, 'platform', 'win32'):
            clean = isolation.prepare(Path('unused'), {}, original)
        self.assertEqual(clean, dict(PATH='preserved', HOME='preserved'))
        self.assertIn('COVERAGE_PROCESS_START', original)

    @unittest.skipUnless(sys.platform == 'linux', 'native Linux namespaces')
    def test_t652_namespace_protects_external_file_and_allows_private_output(self):
        import subprocess
        with tempfile.TemporaryDirectory() as tmp, tempfile.TemporaryDirectory(dir='/var/tmp') as outside:
            root = Path(tmp); output = root/'owned'; output.mkdir()
            sentinel = Path(outside)/'external'; sentinel.write_text('unchanged')
            code = ('import os,pathlib,sys; assert not any(k in os.environ for k in '
                    '["COVERAGE_PROCESS_START","BLENT_PYTHON_CALLS","BASH_ENV"]); '
                    'pathlib.Path(sys.argv[1]).write_text("changed")')
            env = dict(os.environ, COVERAGE_PROCESS_START='/not-used', BLENT_PYTHON_CALLS='/not-used', BASH_ENV='/not-used')
            blocked = subprocess.run(isolation.command([sys.executable, '-c', code, str(sentinel)], output),
                                     capture_output=True, timeout=10, env=env)
            self.assertNotEqual(blocked.returncode, 0)
            self.assertIn(b'Errno 30', blocked.stderr)
            self.assertEqual(sentinel.read_text(), 'unchanged')
            subprocess.run(isolation.command([sys.executable, '-c', code, str(output/'writable')], output),
                           check=True, timeout=10, env=env)
            self.assertEqual((output/'writable').read_text(), 'changed')


if __name__ == '__main__':
    unittest.main()
