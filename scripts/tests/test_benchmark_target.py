"""T411: a benchmark window must fit the exact secondary monitor requested."""
import importlib.util
from pathlib import Path
import sys
import unittest
from unittest.mock import patch

SCRIPTS = Path(__file__).resolve().parents[1] / 'benchmarks'
sys.path.insert(0, str(SCRIPTS))
SPEC = importlib.util.spec_from_file_location('baseline', SCRIPTS / 'run-baseline.py')
BASELINE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(BASELINE)


class BenchmarkTargetTests(unittest.TestCase):
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
