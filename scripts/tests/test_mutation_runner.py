"""T652: mutation evidence must fail closed and keep source/process ownership."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'mutation'))
import evidence
import process
import run as mutation


def mutant():
    return dict(file='production.rs', span={'start': {'line': 1, 'column': 1},
                                          'end': {'line': 1, 'column': 2}}, replacement='false')


def outcome(summary='CaughtMutant', test=None):
    return dict(scenario={'Mutant': mutant()}, summary=summary, log_path='mutant.log',
                phase_results=[{'phase': 'Build', 'process_status': 'Success',
                                'argv': ['cargo', 'test', '--package=producer', '--no-run']},
                               {'phase': 'Test', 'process_status': test or {'Failure': 101},
                                'argv': ['cargo', 'test', '--package=producer']}])


def artifacts(directory, row=None):
    baseline = dict(scenario='Baseline', summary='Success', log_path='baseline.log',
                    phase_results=[{'phase': phase, 'process_status': 'Success',
                                    'argv': ['cargo', 'test', '--package=producer'] +
                                            (['--no-run'] if phase == 'Build' else [])}
                                   for phase in ['Build', 'Test']])
    mutation.write_json(directory / 'mutants.json', [mutant()])
    mutation.write_json(directory / 'outcomes.json', {'outcomes': [baseline, row or outcome()]})
    (directory / 'baseline.log').write_text('test result: ok. 2 passed; 0 failed\n')
    (directory / 'mutant.log').write_text('test result: FAILED. 1 passed; 1 failed\n')


class MutationEvidenceTest(unittest.TestCase):
    def test_t652_only_complete_baselined_caught_runs_pass(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            artifacts(root)
            report = evidence.summarize(root, dict(status='completed', code=0))
            self.assertTrue(report['passes'])
            self.assertEqual(report['outcomes'], {'caught': 1})
            self.assertIsNone(report['whole_project_mutation_score'])

    def test_t652_zero_tests_and_missing_baselines_fail(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            artifacts(root)
            (root / 'baseline.log').write_text('test result: ok. 0 passed; 0 failed\n')
            self.assertFalse(evidence.summarize(root, dict(status='completed', code=0))['passes'])
            data = json.loads((root / 'outcomes.json').read_text())
            data['outcomes'].pop(0)
            mutation.write_json(root / 'outcomes.json', data)
            self.assertFalse(evidence.summarize(root, dict(status='completed', code=0))['passes'])

    def test_t652_incomplete_duplicate_empty_and_unknown_results_fail(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            for candidates in [[], [mutant(), mutant()], [dict(mutant(), replacement='true')]]:
                artifacts(root)
                mutation.write_json(root / 'mutants.json', candidates)
                self.assertFalse(evidence.summarize(root, dict(status='completed', code=0))['passes'])
            artifacts(root, outcome('Unexpected'))
            self.assertEqual(evidence.summarize(root, dict(status='completed', code=0))['outcomes'],
                             {'tool_error': 1})

    def test_t652_timeouts_unviable_survivors_and_tool_errors_remain_distinct(self):
        cases = [('MissedMutant', 'Success', 'survived'),
                 ('Unviable', {'Failure': 101}, 'unviable'),
                 ('Timeout', 'Timeout', 'timeout')]
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            for summary, test, expected in cases:
                artifacts(root, outcome(summary, test))
                report = evidence.summarize(root, dict(status='completed', code=2))
                self.assertFalse(report['passes'])
                self.assertEqual(report['outcomes'], {expected: 1})
            artifacts(root)
            for status in [dict(status='timeout'), dict(status='tool_error'),
                           dict(status='completed', code=70)]:
                self.assertFalse(evidence.summarize(root, status)['passes'])

    def test_t652_caught_requires_real_failed_test_after_successful_build(self):
        for invalid in ['Timeout', 'Success', {'Signal': 9}, {'Failure': 0}]:
            self.assertEqual(evidence.classify(outcome(test=invalid)), 'tool_error')
        row = outcome()
        row['phase_results'][0]['process_status'] = {'Failure': 101}
        self.assertEqual(evidence.classify(row), 'tool_error')

    def test_t671_different_or_missing_baseline_commands_never_pass(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            for extra in ['--package=consumer', '--all-features', '--test=integration']:
                changed = outcome()
                changed['phase_results'][1]['argv'].append(extra)
                artifacts(root, changed)
                self.assertFalse(evidence.summarize(root, dict(status='completed', code=0))['passes'])
            changed = outcome()
            changed['phase_results'][0].pop('argv')
            artifacts(root, changed)
            self.assertFalse(evidence.summarize(root, dict(status='completed', code=0))['passes'])

    def test_t671_package_order_versions_and_duplicates_are_equivalent(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            changed = outcome()
            changed['phase_results'][1]['argv'] = [
                'cargo', 'test', '--package=producer@1.2.3', '--package=producer']
            artifacts(root, changed)
            self.assertTrue(evidence.summarize(root, dict(status='completed', code=0))['passes'])

    def test_t652_missing_artifacts_never_pass(self):
        with tempfile.TemporaryDirectory() as temp:
            report = mutation.collect(Path(temp), dict(status='completed', code=0))
            self.assertFalse(report['passes'])
            self.assertEqual(report['outcomes'], {'tool_error': 1})


class MutationIsolationTest(unittest.TestCase):
    @unittest.skipUnless(sys.platform == 'linux', 'native Linux process ownership')
    def test_t652_deadline_kills_descendants_with_separate_process_groups(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            script = ('import subprocess,sys,time,pathlib; '
                      'p=subprocess.Popen([sys.executable,"-c","import time; time.sleep(30)"],'
                      'start_new_session=True); pathlib.Path("child.pid").write_text(str(p.pid)); '
                      'time.sleep(30)')
            process.execute([sys.executable, '-c', script], root, dict(os.environ), root/'log', .5)
            pid = int((root/'child.pid').read_text())
            status = Path(f'/proc/{pid}/status')
            try:
                if status.exists():
                    self.assertIn('State:\tZ', status.read_text())
            finally:
                try:
                    os.kill(pid, 9)
                except ProcessLookupError:
                    pass

    def test_t652_snapshot_copies_working_bytes_without_ignored_builds(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp) / 'repo'
            root.mkdir()
            subprocess.run(['git', 'init', '-q', str(root)], check=True)
            (root / '.gitignore').write_text('target/\n')
            (root / 'source.rs').write_text('original')
            (root / 'target').mkdir()
            (root / 'target' / 'ignored').write_text('build cache')
            destination = Path(temp) / 'copy'
            hashes = mutation.snapshot(root, destination)
            self.assertEqual((destination / 'source.rs').read_text(), 'original')
            self.assertFalse((destination / 'target').exists())
            (destination / 'source.rs').write_text('mutation')
            self.assertTrue(mutation.unchanged(root, hashes))
            (root / 'source.rs').write_text('new source')
            self.assertFalse(mutation.unchanged(root, hashes))

    def test_t652_profiles_reject_foreign_platform(self):
        with self.assertRaises(ValueError):
            mutation.profile('windows-autostart', 'linux')
        with self.assertRaises(ValueError):
            mutation.profile('linux-autostart', 'win32')

    def test_t671_consumer_packages_are_explicit_baseline_arguments(self):
        selected = dict(packages=['producer'], test_packages=['producer', 'consumer'],
                        files=['source.rs'], cargo_args=['--lib'])
        args = argparse.Namespace(test_timeout=60, build_timeout=600, jobs=2, build_jobs=6)
        command = mutation.command(selected, args, Path('/tmp/evidence'))
        for package in selected['test_packages']:
            self.assertIn('--cargo-arg=--package=' + package, command)

    def test_t652_deadline_and_missing_executable_are_not_caught(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            command = [sys.executable, '-c', 'import time; time.sleep(30)']
            result = process.execute(command, root, dict(os.environ), root/'timeout.log', .1)
            self.assertEqual(result['status'], 'timeout')
            result = process.execute([str(root/'missing')], root, dict(os.environ), root/'error.log', 1)
            self.assertEqual(result['status'], 'tool_error')

    def test_t652_positive_bounded_inputs_and_no_in_place_command(self):
        for value in ['0', '-1', '-2147483648']:
            with self.assertRaises(argparse.ArgumentTypeError):
                mutation.positive(value)
        selected = mutation.profile('usb', sys.platform)
        args = argparse.Namespace(test_timeout=60, build_timeout=600, jobs=2, build_jobs=6)
        command = mutation.command(selected, args, Path('/tmp/evidence'))
        self.assertIn('--baseline=run', command)
        self.assertIn('--copy-target=false', command)
        self.assertNotIn('--in-place', command)
        self.assertIn('--cargo-arg=--locked', command)


if __name__ == '__main__':
    unittest.main()
