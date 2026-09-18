"""T424: obscured or interrupted workloads never become valid performance data."""
import json
import importlib.util
import os
from pathlib import Path
import sys
import select
import subprocess
import tempfile
import unittest
import time
from unittest.mock import MagicMock, patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'benchmarks'))
from summarize import summarize
from workload import Workload
from visibility import VisibilityMonitor, lock_problem
from xvisibility import XVisibility
from Xlib import X, display


class VisibilityIntegrityTests(unittest.TestCase):
    def test_t424_plot_refuses_invalid_data_before_reading_series(self):
        path = Path(__file__).resolve().parents[1] / 'benchmarks' / 'plot-baseline.py'
        spec = importlib.util.spec_from_file_location('baseline_plot', path)
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        with tempfile.TemporaryDirectory() as directory:
            folder = Path(directory)
            (folder / 'metadata.json').write_text('{"visibility_guard_version":1}')
            (folder / 'phases.jsonl').write_text('{"event":"invalid","reason":"desktop locked"}\n')
            with patch.object(module, 'series', side_effect=AssertionError('invalid run reached plotting')):
                with self.assertRaisesRegex(ValueError, 'invalid'):
                    module.plot(folder, folder / 'timeline.png')

    def summary(self, invalid=False, guarded=True, complete=True):
        with tempfile.TemporaryDirectory() as directory:
            folder = Path(directory)
            meta = dict(source_commit='fixture', host_ticks_per_second=100,
                        android_ticks_per_second=100)
            if guarded:
                meta['visibility_guard_version'] = 1
            (folder / 'metadata.json').write_text(json.dumps(meta))
            phases = [dict(event='phase', utc=0, measured=True, trial=1, kind='motion',
                           visibility_verified=guarded)]
            if complete:
                phases.append(dict(event='complete', utc=20, visibility_verified=guarded))
            for name, rows in [('phases', phases), ('samples', []), ('host-windows', [])]:
                (folder / f'{name}.jsonl').write_text(''.join(json.dumps(r) + '\n' for r in rows))
            if invalid:
                (folder / 'invalid.json').write_text(json.dumps(dict(reason='desktop locked')))
            return summarize(folder)

    def test_t424_invalid_marker_quarantines_complete_run(self):
        result = self.summary(invalid=True)
        self.assertFalse(result['complete'], 'T424: completion cannot override invalid visibility')
        self.assertEqual(result['phases'], [])
        self.assertIsNone(result['whole_run_battery'])
        self.assertEqual(len(result['invalid_data']['phases']), 1)
        self.assertIn('desktop locked', result['invalid_reasons'])

    def test_t424_phase_boundary_rechecks_visibility_before_completion(self):
        work = Workload.__new__(Workload)
        work.plan, work.index, work.ticks, work.state = [{}], 0, 3, {}
        work.root = MagicMock()
        work.guard = MagicMock()
        work.guard.problem.return_value = 'workload occluded'
        work.event = MagicMock()
        work.next_phase()
        events = [call.args[0] for call in work.event.call_args_list]
        self.assertEqual([e['event'] for e in events], ['invalid'],
                         'T424: a covered workload must never report completion')
        work.root.destroy.assert_called_once()

    def test_t424_valid_legacy_and_interrupted_runs_are_distinguished(self):
        valid = self.summary()
        self.assertEqual(valid['visibility'], 'verified')
        self.assertEqual(len(valid['phases']), 1)
        self.assertEqual(self.summary(guarded=False)['visibility'], 'unverified')
        partial = self.summary(complete=False)
        self.assertFalse(partial['complete'])
        self.assertEqual(partial['visibility'], 'invalid')
        self.assertEqual(partial['phases'], [])

    def test_t424_lock_state_is_fail_closed(self):
        cases = [(0, 'no', None), (0, 'yes', 'desktop locked'),
                 (0, '', 'unknown'), (1, 'no', 'unknown')]
        for code, output, problem in cases:
            result = MagicMock(returncode=code, stdout=output)
            with patch.dict(os.environ, {'XDG_SESSION_ID': 'fixture'}), \
                 patch('visibility.subprocess.run', side_effect=lambda args, **_: result if args[0] == 'loginctl' else MagicMock(returncode=0, stdout='(false,)')):
                actual = lock_problem()
                if problem is None:
                    self.assertIsNone(actual)
                else:
                    self.assertIn(problem, actual)
        with patch.dict(os.environ, {}, clear=True):
            self.assertIn('unavailable', lock_problem())

    def test_t424_desktop_lock_overrides_incorrect_logind_hint(self):
        def run(args, **kwargs):
            return MagicMock(returncode=0, stdout='no' if args[0] == 'loginctl' else '(true,)')
        with patch.dict(os.environ, {'XDG_SESSION_ID': 'fixture'}), \
             patch('visibility.subprocess.run', side_effect=run):
            self.assertIn('locked', lock_problem() or '', 'T424: Cinnamon can disagree with logind')

    def test_t424_monitor_latches_failure_and_closes_probe(self):
        probe = MagicMock()
        monitor = VisibilityMonitor(0, '', probe=lambda: probe, lock=lambda: 'desktop locked')
        monitor.start()
        monitor.close()
        self.assertEqual(monitor.problem(), 'desktop locked')
        probe.problem.assert_not_called()
        probe.close.assert_called_once()
        monitor.failure = None
        monitor.last_checked = time.monotonic() - 3
        self.assertIn('stale', monitor.problem())


class XVisibilityTests(unittest.TestCase):
    """Use only a private Xvfb, never the user's desktop or EVDI."""
    def setUp(self):
        read_fd, write_fd = os.pipe()
        try:
            server = subprocess.Popen(['Xvfb', '-displayfd', str(write_fd), '-screen', '0', '1600x1000x24',
                                       '-nolisten', 'tcp', '-ac'], pass_fds=(write_fd,),
                                      stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            self.addCleanup(self.stop_server, server)
            self.assertTrue(select.select([read_fd], [], [], 5)[0], 'T424: private Xvfb startup timed out')
            name = ':' + os.read(read_fd, 100).decode().strip()
        finally:
            os.close(read_fd)
            os.close(write_fd)
        self.display = display.Display(name)
        self.display_name = name
        self.addCleanup(self.display.close)
        self.root = self.display.screen().root
        self.window = self.root.create_window(100, 100, 1280, 800, 0, X.CopyFromParent,
                                              override_redirect=True)
        self.window.map()
        self.window.set_input_focus(X.RevertToParent, X.CurrentTime)
        self.display.sync()
        self.probe = XVisibility(self.window.id, '1280x800+100+100', name)
        self.addCleanup(self.probe.close)

    @staticmethod
    def stop_server(server):
        server.terminate()
        server.wait(timeout=5)

    def test_t424_visible_window_and_focused_descendant(self):
        self.assertIsNone(self.probe.problem())
        child = self.window.create_window(1, 1, 50, 50, 0, X.CopyFromParent)
        child.map()
        child.set_input_focus(X.RevertToParent, X.CurrentTime)
        self.display.sync()
        self.assertIsNone(self.probe.problem())

    def test_t424_one_pixel_occlusion_is_rejected(self):
        cover = self.root.create_window(101, 101, 1, 1, 0, X.CopyFromParent,
                                        override_redirect=True)
        cover.map()
        self.display.sync()
        self.assertIn('occluded', self.probe.problem())
        cover.unmap()
        self.display.sync()
        self.assertIsNone(self.probe.problem())

    def test_t424_redirected_window_coverage(self):
        self.display.composite_query_version()
        self.root.composite_redirect_subwindows(1)
        self.display.sync()
        self.assertIsNone(self.probe.problem())
        cover = self.root.create_window(101, 101, 50, 50, 0, X.CopyFromParent,
                                        override_redirect=True)
        cover.map()
        self.display.sync()
        self.assertIn('occluded', self.probe.problem())

    def test_t424_real_tk_workload_uses_guard_before_phase_and_completion(self):
        events = self.run_tk('visible')
        self.assertEqual([e['event'] for e in events], ['phase', 'complete'])
        self.assertTrue(all(e['visibility_verified'] for e in events))

    def test_t424_real_tk_workload_stops_on_midphase_occlusion(self):
        events = self.run_tk('occluded')
        self.assertEqual([e['event'] for e in events], ['phase', 'invalid'])
        self.assertIn('occluded', events[-1]['reason'])

    def run_tk(self, mode):
        fixture = Path(__file__).with_name('benchmark_tk_fixture.py')
        result = subprocess.run([sys.executable, str(fixture), self.display_name, mode],
                                capture_output=True, text=True, timeout=10)
        self.assertEqual(result.returncode, 0, result.stderr)
        return json.loads(result.stdout)

    def test_t424_focus_loss_and_geometry_change_are_rejected(self):
        self.root.set_input_focus(X.RevertToParent, X.CurrentTime)
        self.display.sync()
        self.assertIn('focus', self.probe.problem())
        self.window.set_input_focus(X.RevertToParent, X.CurrentTime)
        self.window.configure(x=101)
        self.display.sync()
        self.assertIn('geometry', self.probe.problem())

    def test_t424_hidden_window_is_rejected(self):
        self.window.unmap()
        self.display.sync()
        self.assertIn('viewable', self.probe.problem())


if __name__ == '__main__':
    unittest.main()
