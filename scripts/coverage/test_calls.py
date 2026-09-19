"""T497: declaration-only hits cannot pass a function coverage gate."""
from pathlib import Path
import json
import os
import sys
import tempfile
import unittest
from unittest.mock import patch

from calls import Calls, invoked, read_calls, start
from model import Function, fingerprint


class InvocationTest(unittest.TestCase):
    def test_t497_profile_callbacks_map_copies_without_counting_return_events(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)/'repo'
            root.mkdir()
            source = root/'fixture.py'
            source.write_text('def captured():\n    return sys._getframe()\n')
            duplicate = root.parent/'copy.py'
            duplicate.write_bytes(source.read_bytes())
            calls = Calls(root, {'fixture.py': fingerprint(source)}, root/'calls')
            namespace = dict(sys=sys)
            exec(compile(duplicate.read_text(), str(duplicate), 'exec'), namespace)
            frame = namespace['captured']()
            calls.observe(frame, 'return', None)
            self.assertFalse(calls.observed)
            calls.observe(frame, 'call', None)
            self.assertEqual(calls.source(frame.f_code), 'fixture.py')
            self.assertEqual(calls.observed, {('fixture.py', 1, 'captured')})
            self.assertIsNone(calls.source(self.test_t497_profile_callbacks_map_copies_without_counting_return_events.__code__))

    def test_t497_start_installs_main_and_future_thread_profiles_with_exit_persistence(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root/'a.py').write_text('def used(): return 7\n')
            manifest = root/'manifest.json'
            manifest.write_text(json.dumps(dict(sources={'a.py': fingerprint(root/'a.py')})))
            env = dict(USCREEN_COVERAGE_ROOT=str(root), USCREEN_COVERAGE_MANIFEST=str(manifest),
                       USCREEN_PYTHON_CALLS=str(root/'calls'))
            with patch.dict(os.environ, env), patch('calls.sys.setprofile') as main, \
                    patch('calls.threading.setprofile') as thread, patch('calls.atexit.register') as shutdown:
                observer = start()
            main.assert_called_once_with(observer.observe)
            thread.assert_called_once_with(observer.observe)
            shutdown.assert_called_once_with(observer.save)
            observer.save()
            self.assertEqual(read_calls(root/'calls'), set())

    def test_t497_one_line_function_needs_an_actual_call(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root/'fixture.py'
            source.write_text('def used(): return 7\ndef missed(): return 9\nused()\n')
            calls = Calls(root, {'fixture.py': fingerprint(source)}, root/'calls')
            previous = sys.getprofile()
            try:
                sys.setprofile(calls.observe)
                exec(compile(source.read_text(), str(source), 'exec'), {})
            finally:
                sys.setprofile(previous)
            calls.save()
            observed = read_calls(root/'calls')
            self.assertTrue(invoked(Function('fixture.py', 'used', 1, 1, 'python'), observed))
            self.assertFalse(invoked(Function('fixture.py', 'missed', 2, 2, 'python'), observed))

    def test_t497_decorated_methods_use_the_code_entry_line(self):
        function = Function('a.py', 'method', 4, 7, 'python', entry=3, body=5)
        self.assertTrue(invoked(function, {('a.py', 3, 'method')}))
        self.assertFalse(invoked(function, {('a.py', 4, 'method')}))
