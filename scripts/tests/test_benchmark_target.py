"""T411: a benchmark window must fit the exact secondary monitor requested."""
import importlib.util
from pathlib import Path
import sys
import unittest
from unittest.mock import patch, MagicMock
from types import SimpleNamespace
import tempfile

SCRIPTS = Path(__file__).resolve().parents[1] / 'benchmarks'
sys.path.insert(0, str(SCRIPTS))
SPEC = importlib.util.spec_from_file_location('baseline', SCRIPTS / 'run-baseline.py')
BASELINE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(BASELINE)


class BenchmarkTargetTests(unittest.TestCase):
    def test_t441_partial_logger_startup_retires_previous_child(self):
        args = SimpleNamespace(output=Path('/unused-t441'), serial='fixture')
        first = ('first-collector', 'first-reader')
        with patch.object(BASELINE, 'filtered_logs', side_effect=[first, RuntimeError('second logger failed')]), \
             patch.object(BASELINE, 'retire_logs') as retire:
            with self.assertRaisesRegex(RuntimeError, 'second logger failed'):
                BASELINE.start_logs(args, {'android_pid': 123})
            retire.assert_called_once_with([first])

    def test_t441_startup_failure_retires_log_collectors_and_sampler(self):
        with tempfile.TemporaryDirectory() as directory:
            args = SimpleNamespace(output=Path(directory) / 'raw', geometry='1280x800+0+0', serial='fixture')
            sampler = MagicMock()
            logs = [('collector', 'reader')]
            with patch.object(BASELINE, 'arguments', return_value=args), \
                 patch.object(BASELINE, 'ensure_target', return_value='monitor'), \
                 patch.object(BASELINE, 'metadata', return_value={'plan': []}), \
                 patch.object(BASELINE, 'command', return_value=(0, '', '')), \
                 patch.object(BASELINE, 'start_logs', return_value=logs), \
                 patch.object(BASELINE, 'Sampler', return_value=sampler), \
                 patch.object(BASELINE, 'Workload', side_effect=RuntimeError('unavailable display')), \
                 patch.object(BASELINE, 'retire_logs') as retire:
                with self.assertRaisesRegex(RuntimeError, 'unavailable display'):
                    BASELINE.main()
                retire.assert_called_once_with(logs)
                sampler.file.close.assert_called_once()

    def test_t411_requires_complete_monitor_geometry(self):
        cases = [
            ('wrong height', '1280x800+3840+0', '1280/339x720/190+3840+0'),
            ('wrong width', '1280x800+3840+0', '11280/339x800/212+3840+0'),
            ('position prefix', '1280x800+3840+8', '1280/339x800/212+3840+80'),
        ]
        for label, requested, actual in cases:
            with self.subTest(label=label):
                monitors = f'Monitors: 1\n 1: +DVI-I-2-1 {actual} DVI-I-2-1\n'
                with patch.object(BASELINE, 'command', return_value=(0, monitors, '')):
                    with self.assertRaises(ValueError, msg='T411: never spill outside the selected monitor'):
                        BASELINE.ensure_target(requested)

    def test_t411_primary_and_failed_probes_are_rejected(self):
        primary = 'Monitors: 1\n 0: +*HDMI-A-1 1280/339x800/212+0+0 HDMI-A-1\n'
        for result in [(0, primary, ''), (1, '', 'probe failed')]:
            with self.subTest(result=result):
                with patch.object(BASELINE, 'command', return_value=result):
                    with self.assertRaises(ValueError):
                        BASELINE.ensure_target('1280x800+0+0')

    def test_t411_exact_secondary_monitor_is_accepted(self):
        monitors = ('Monitors: 2\n'
                    ' 0: +*HDMI-A-1 3840/600x2160/340+0+0 HDMI-A-1\n'
                    ' 1: +DVI-I-2-1 1280/339x800/212+3840+0 DVI-I-2-1\n')
        with patch.object(BASELINE, 'command', return_value=(0, monitors, '')):
            self.assertEqual(BASELINE.ensure_target('1280x800+3840+0'), monitors)


if __name__ == '__main__':
    unittest.main()
