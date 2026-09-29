"""T424: obscured or interrupted workloads never become valid performance data."""
import json
import importlib.util
import io
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


def startup_diagnostics(server, error):
    code = server.poll()
    fields = [f'pid={server.pid}', f'exit={code}', f'stderr={error!r}']
    for name in ['status', 'wchan', 'syscall']:
        value = 'unavailable: process already exited'
        if code is None:
            try:
                with Path(f'/proc/{server.pid}/{name}').open() as source:
                    value = source.read(4096)
            except OSError as failure:
                value = f'unavailable: {failure}'
        fields.append(f'proc/{name}={value!r}')
    return '; '.join(fields)


def read_display_number(read_fd, timeout=5):
    """T722: Xorg writes the number and its newline in separate syscalls."""
    deadline = time.monotonic() + timeout
    record = b''
    while not record.endswith(b'\n'):
        if len(record) >= 100:
            raise ValueError('display publication is too long')
        remaining = deadline - time.monotonic()
        if remaining <= 0 or not select.select([read_fd], [], [], remaining)[0]:
            raise TimeoutError('display publication deadline expired')
        chunk = os.read(read_fd, 100 - len(record))
        if not chunk:
            raise ValueError('display publication closed before newline')
        record += chunk
    if not record[:-1].isdigit():
        raise ValueError('invalid display publication')
    return ':' + record[:-1].decode('ascii')


class VisibilityIntegrityTests(unittest.TestCase):
    def test_t722_connect_failure_retains_owned_server_evidence(self):
        fixture = XVisibilityTests('test_t424_visible_window_and_focused_descendant')
        fixture.server = MagicMock(pid=-1)
        fixture.server.poll.return_value = 23
        fixture.server_log = io.StringIO('T722 fixture server retired\n')
        try:
            with patch.object(fixture, 'start_private_server', return_value=':42'), \
                 patch.object(display, 'Display', side_effect=ConnectionError('fixture refused')):
                with self.assertRaisesRegex(AssertionError, r'T722.*:42.*pid=-1.*exit=23.*server retired'):
                    fixture.setUp()
        finally:
            fixture.doCleanups()

    def test_t722_display_publication_waits_for_newline(self):
        fixture = XVisibilityTests('test_t424_visible_window_and_focused_descendant')
        try:
            with patch('subprocess.Popen'), \
                 patch('select.select', return_value=([1], [], [])), \
                 patch('os.read', side_effect=[b'4', b'2', b'\n']) as read:
                self.assertEqual(fixture.start_private_server(), ':42',
                                 'T722: numeric prefix is not a completed publication')
                self.assertEqual(read.call_count, 3)
        finally:
            fixture.doCleanups()

    def test_t722_display_publication_rejects_incomplete_or_invalid_records(self):
        records = [[b''], [b'\n'], [b'42', b''], [b'no\n'], [b'1\n2\n'],
                   [b'\xff\n'], [b'9' * 100]]
        for chunks in records:
            with self.subTest(chunks=chunks):
                fixture = XVisibilityTests('test_t424_visible_window_and_focused_descendant')
                try:
                    with patch('subprocess.Popen'), \
                         patch('select.select', return_value=([1], [], [])), \
                         patch('os.read', side_effect=chunks):
                        with self.assertRaisesRegex(AssertionError, 'T643'):
                            fixture.start_private_server()
                finally:
                    fixture.doCleanups()

    def test_t722_publication_boundaries_and_fragment_partitions(self):
        for number in [b'0', b'9', b'10', b'42', b'59535', b'9' * 99]:
            record = number + b'\n'
            for split in range(1, len(record)):
                with patch('select.select', return_value=([1], [], [])), \
                     patch('os.read', side_effect=[record[:split], record[split:]]):
                    self.assertEqual(read_display_number(1), ':' + number.decode())

    def test_t722_publication_rejects_all_non_digit_prefix_bytes(self):
        invalid = [value for value in range(256) if not bytes([value]).isdigit()]
        for value in invalid:
            with patch('select.select', return_value=([1], [], [])), \
                 patch('os.read', return_value=bytes([value]) + b'\n'):
                with self.assertRaises(ValueError):
                    read_display_number(1)

    def test_t722_partial_publication_keeps_one_deadline(self):
        fixture = XVisibilityTests('test_t424_visible_window_and_focused_descendant')
        try:
            with patch('subprocess.Popen'), \
                 patch('time.monotonic', side_effect=[10, 10, 14, 15]), \
                 patch('select.select', return_value=([1], [], [])) as select_read, \
                 patch('os.read', side_effect=[b'4', b'2']):
                with self.assertRaisesRegex(AssertionError, 'T424.*timed out'):
                    fixture.start_private_server()
                self.assertEqual([call.args[3] for call in select_read.call_args_list], [5, 1])
        finally:
            fixture.doCleanups()

    def test_t644_readiness_timeout_retains_live_child_diagnostics(self):
        real_popen = subprocess.Popen
        def stalled_server(command, **kwargs):
            return real_popen([sys.executable, '-c', 'import time; time.sleep(60)'], **kwargs)
        fixture = XVisibilityTests('test_t424_visible_window_and_focused_descendant')
        try:
            with patch('subprocess.Popen', side_effect=stalled_server), patch('select.select', return_value=([], [], [])):
                with self.assertRaisesRegex(AssertionError, r'pid=\d+.*exit=None.*proc/status='):
                    fixture.setUp()
        finally:
            fixture.doCleanups()

    def test_t644_exited_child_retains_unavailable_proc_and_stderr(self):
        server = MagicMock(pid=-1)
        server.poll.return_value = 23
        with patch.object(Path, 'open', side_effect=AssertionError('T644: reaped PID must not be inspected')):
            result = startup_diagnostics(server, 'fixture\nstderr')
        self.assertIn('exit=23', result)
        self.assertIn("stderr='fixture\\nstderr'", result)
        for name in ['status', 'wchan', 'syscall']:
            self.assertRegex(result, rf"proc/{name}=['\"]unavailable:")

    def test_t643_early_xvfb_exit_preserves_startup_error(self):
        real_popen = subprocess.Popen
        def failing_server(command, **kwargs):
            return real_popen([sys.executable, '-c',
                               "import sys; sys.stderr.write('T643 fixture startup error\\n'); sys.exit(23)"], **kwargs)
        fixture = XVisibilityTests('test_t424_visible_window_and_focused_descendant')
        try:
            with patch('subprocess.Popen', side_effect=failing_server):
                with self.assertRaisesRegex(AssertionError, 'T643 fixture startup error'):
                    fixture.setUp()
        finally:
            fixture.doCleanups()

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
            samples = [dict(utc=t, monotonic=t, host=[], android=dict(pid=123, start_ticks=1, ticks=t),
                            collection_seconds=.01) for t in [0, 5, 10, 15]]
            (folder / 'android.log').write_text('15.0 123 123 D Blent: fixture\n')
            for name, rows in [('phases', phases), ('samples', samples), ('host-windows', [dict(utc=15, message='fixture')])]:
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
        work.observation_problem = lambda: None
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
    def start_private_server(self):
        log = tempfile.TemporaryFile(mode='w+')
        self.addCleanup(log.close)
        read_fd, write_fd = os.pipe()
        try:
            server = subprocess.Popen(['Xvfb', '-displayfd', str(write_fd), '-screen', '0', '1600x1000x24',
                                       '-nolisten', 'tcp', '-ac'], pass_fds=(write_fd,),
                                      stdout=subprocess.DEVNULL, stderr=log)
            self.server, self.server_log = server, log
            self.addCleanup(self.stop_server, server)
            os.close(write_fd)
            write_fd = None
            try:
                return read_display_number(read_fd)
            except TimeoutError:
                self.fail('T424: private Xvfb startup timed out: ' + self.server_diagnostics())
            except ValueError as error:
                self.fail(f'T643: Xvfb did not publish a private display ({error}): ' +
                          self.server_diagnostics())
        finally:
            os.close(read_fd)
            if write_fd is not None:
                os.close(write_fd)

    def setUp(self):
        name = self.start_private_server()
        try:
            self.display = display.Display(name)
        except Exception as error:
            self.fail(f'T722: private Xvfb connection failed on {name} ({error}): ' +
                      self.server_diagnostics())
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

    def server_diagnostics(self):
        self.server_log.seek(0)
        return startup_diagnostics(self.server, self.server_log.read(65536))

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
