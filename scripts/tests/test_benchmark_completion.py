"""T485: successful measurements cannot take focus from the user's next app."""
import importlib.util
import json
from pathlib import Path
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

DIRECTORY = Path(__file__).resolve().parents[1] / 'benchmarks'
sys.path.insert(0, str(DIRECTORY))


def module(name):
    spec = importlib.util.spec_from_file_location(name, DIRECTORY / (name + '.py'))
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


class CompletionTests(unittest.TestCase):
    def assert_no_relaunch(self, capture):
        targets = [call.args for call in capture.call_args_list
                   if 'com.uscreen/.MainActivity' in call.args]
        self.assertEqual(targets, [], 'T485: completion relaunched UScreen over the next app')

    def test_t485_decoder_matrix_completion_does_not_choose_foreground_app(self):
        device = module('decoder-device')
        with tempfile.TemporaryDirectory() as directory:
            args = SimpleNamespace(output=Path(directory) / 'output', serial='fixture', seconds=1,
                                   warmup=0, trials=1, variants=[('candidate', 'legacy')])
            with patch.object(device, 'capture', return_value='fixture') as capture, \
                    patch.object(device, 'trial'), patch.object(device.subprocess, 'check_output', return_value='adb'):
                device.run(args)
            self.assert_no_relaunch(capture)

    def test_t485_explicit_plan_completion_does_not_choose_foreground_app(self):
        plan = module('decoder-plan')
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'plan.json').write_text(json.dumps([dict(scene='motion', fixture='clip.bin',
                profile='legacy', rate=60, trial=0, seconds=1, warmup=0)]))
            (root / 'provenance.json').write_text(json.dumps(dict(package='com.uscreen.decoderbench.candidate')))
            args = SimpleNamespace(plan=root / 'plan.json', provenance=root / 'provenance.json',
                                   output=root / 'output', serial='fixture')
            with patch.object(plan, 'verify_apk'), patch.object(plan.DEVICE, 'trial'), \
                    patch.object(plan.DEVICE, 'capture', return_value='fixture') as capture:
                plan.run(args)
            self.assert_no_relaunch(capture)

    def test_t485_usb_completion_does_not_choose_foreground_app(self):
        driver = module('profile-usb')
        result = dict(phases=[dict(frames=[0, 1])], acknowledgements=[dict(sequence=1), dict(sequence=2)])
        with tempfile.TemporaryDirectory() as directory:
            args = SimpleNamespace(output=Path(directory), serial='fixture')
            row = dict(scene='motion', rate=60, encoder='libx264', selection={})
            with patch.object(driver, 'foreground'), patch.object(driver, 'connect'), \
                    patch.object(driver, 'stream', return_value=result), patch.object(driver.socket, 'socket') as socket, \
                    patch.object(driver.DEVICE, 'capture', return_value='fixture') as capture, \
                    patch.object(driver.DEVICE, 'adb'), patch.object(driver.DEVICE, 'logs'):
                socket.return_value.__enter__.return_value.getsockname.return_value = ('127.0.0.1', 45678)
                driver.trial(args, row, dict(width=1280, height=800, fps=60), {}, 0)
            self.assert_no_relaunch(capture)


if __name__ == '__main__':
    unittest.main()
