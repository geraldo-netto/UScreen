"""T470: interrupted sessions or failed observers cannot certify a baseline."""
import json
import importlib.util
from pathlib import Path
import sys
import tempfile
import threading
import re
import subprocess
from types import SimpleNamespace
import unittest
from unittest.mock import MagicMock, patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'benchmarks'))
from observe import Sampler
from summarize import summarize
from workload import Workload
from android_session import AndroidSessionMonitor, parse_snapshot, snapshot
from observe import filtered_logs


def write_rows(folder, name, rows):
    (folder / name).write_text(''.join(json.dumps(row) + '\n' for row in rows))


def evidence(folder):
    meta = dict(source_commit='fixture', host_ticks_per_second=100, android_ticks_per_second=100,
                android_pid=123, visibility_guard_version=1, observation_guard_version=1)
    (folder / 'metadata.json').write_text(json.dumps(meta))
    phases = [dict(event='phase', utc=0, measured=True, trial=1, kind='static', visibility_verified=True),
              dict(event='complete', utc=30, visibility_verified=True)]
    samples = [dict(utc=t, monotonic=t, host=[], android=dict(pid=123, start_ticks=900, ticks=t, rss_bytes=100),
                    collection_seconds=.01) for t in range(0, 31, 5)]
    health = [dict(utc=t, monotonic=t, pid=123, start_ticks=900, foreground=True) for t in range(31)]
    write_rows(folder, 'phases.jsonl', phases)
    write_rows(folder, 'samples.jsonl', samples)
    write_rows(folder, 'android-session.jsonl', health)
    write_rows(folder, 'host-windows.jsonl', [dict(utc=15, message='Encoder: 1 access units in 5.0s, 0.0 MB/s (1 kbps)')])
    (folder / 'android.log').write_text('15.0 123 123 D UScreen: Control statistics: fixture\n')
    (folder / 'observer.json').write_text(json.dumps(dict(complete=True, errors=[], log_receipts={'android.log': [15]})))
    return samples, health


class ObservationTests(unittest.TestCase):
    def test_t470_missing_or_stale_observations_are_quarantined(self):
        for damage in ['empty', 'stale', 'pid', 'exception', 'missing-log', 'foreground']:
            with self.subTest(damage=damage), tempfile.TemporaryDirectory() as directory:
                folder = Path(directory)
                samples, health = evidence(folder)
                if damage == 'empty':
                    write_rows(folder, 'samples.jsonl', [])
                elif damage == 'stale':
                    write_rows(folder, 'samples.jsonl', samples[:1])
                elif damage == 'pid':
                    samples[2]['android']['pid'] = 456
                    write_rows(folder, 'samples.jsonl', samples)
                elif damage == 'exception':
                    (folder / 'observer.json').write_text(json.dumps(dict(complete=False, errors=['collector failed'])))
                elif damage == 'missing-log':
                    (folder / 'android.log').unlink()
                else:
                    health[10]['foreground'] = False
                    write_rows(folder, 'android-session.jsonl', health)
                result = summarize(folder)
                self.assertFalse(result['complete'], f'T470: accepted {damage}')
                self.assertEqual(result['phases'], [])
                self.assertTrue(result['invalid_reasons'])

    def test_t470_healthy_sparse_stream_does_not_require_high_frame_rate(self):
        with tempfile.TemporaryDirectory() as directory:
            folder = Path(directory)
            evidence(folder)
            result = summarize(folder)
            self.assertTrue(result['complete'], result['invalid_reasons'])
            self.assertEqual(len(result['phases']), 1)

    def test_t470_android_log_clock_is_not_compared_to_host_phase_clock(self):
        with tempfile.TemporaryDirectory() as directory:
            folder = Path(directory)
            evidence(folder)
            (folder / 'android.log').write_text('         9000000000.0 123 123 D UScreen: fixture\n')
            result = summarize(folder)
            self.assertTrue(result['complete'], result['invalid_reasons'])

    def test_t470_plot_refuses_failed_observers_before_series(self):
        path = Path(__file__).resolve().parents[1] / 'benchmarks/plot-baseline.py'
        spec = importlib.util.spec_from_file_location('observation_plot', path)
        plot = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(plot)
        with tempfile.TemporaryDirectory() as directory:
            folder = Path(directory)
            evidence(folder)
            (folder / 'observer.json').write_text(json.dumps(dict(complete=False, errors=['collector failed'])))
            with patch.object(plot, 'series') as series:
                with self.assertRaisesRegex(ValueError, 'collector failed'):
                    plot.plot(folder, folder / 'plot.png')
                series.assert_not_called()

    def test_t470_sampler_exception_is_owned_and_reported(self):
        with tempfile.TemporaryDirectory() as directory:
            sampler = Sampler('fixture', Path(directory), {}, threading.Event(), [], android_page_size=4096)
            with patch.object(sampler, 'sample', side_effect=ValueError('broken observer')):
                sampler.run()
            self.assertIn('broken observer', sampler.failure)
            self.assertTrue(sampler.done.is_set())
            self.assertTrue(sampler.file.closed)

    def test_t470_live_sampler_staleness_and_missing_process_are_errors(self):
        with tempfile.TemporaryDirectory() as directory:
            sampler = Sampler('fixture', Path(directory), {}, threading.Event(), [], android_page_size=4096)
            try:
                sampler.last_sample -= 31
                self.assertIn('stale', sampler.problem())
                with patch('observe.service_processes', return_value=[]), patch.object(sampler, 'app_process', return_value={'error': 'ADB failed'}):
                    with self.assertRaisesRegex(ValueError, 'process sample unavailable'):
                        sampler.sample()
            finally:
                sampler.file.close()

    def test_t470_workload_rejects_observer_failure_at_phase_boundary(self):
        work = Workload.__new__(Workload)
        work.plan, work.index, work.ticks, work.state = [{}], 0, 3, {}
        work.root, work.guard, work.event = MagicMock(), MagicMock(), MagicMock()
        work.guard.problem.return_value = None
        work.observation_problem = lambda: 'collector failed'
        work.next_phase()
        self.assertEqual([call.args[0]['event'] for call in work.event.call_args_list], ['invalid'])

    def test_t470_missing_files_and_android_health_gaps_are_quarantined(self):
        for damage in ['samples.jsonl', 'host-windows.jsonl', 'observer.json', 'android-session.jsonl', 'gap']:
            with self.subTest(damage=damage), tempfile.TemporaryDirectory() as directory:
                folder = Path(directory)
                _, health = evidence(folder)
                if damage == 'gap':
                    write_rows(folder, 'android-session.jsonl', health[:4] + health[12:])
                else:
                    (folder / damage).unlink()
                result = summarize(folder)
                self.assertFalse(result['complete'], result)
                self.assertTrue(result['invalid_reasons'])

    def test_t470_legacy_evidence_remains_read_only_and_observation_unverified(self):
        with tempfile.TemporaryDirectory() as directory:
            folder = Path(directory)
            evidence(folder)
            path = folder / 'metadata.json'
            meta = json.loads(path.read_text())
            del meta['observation_guard_version']
            path.write_text(json.dumps(meta))
            (folder / 'observer.json').unlink()
            (folder / 'android-session.jsonl').unlink()
            before = {p.name: p.read_bytes() for p in folder.iterdir()}
            result = summarize(folder)
            self.assertTrue(result['complete'], result['invalid_reasons'])
            self.assertEqual(result['observation'], 'unverified')
            self.assertEqual(before, {p.name: p.read_bytes() for p in folder.iterdir()})

    def test_t470_log_reader_failure_is_owned(self):
        with tempfile.TemporaryDirectory() as directory:
            collector = filtered_logs([sys.executable, '-c', 'print("invalid json")'], Path(directory) / 'host.log', re.compile('.'), True)
            try:
                collector.thread.join(2)
                self.assertIn('collector failed', collector.problem())
            finally:
                collector.close()


STAT = '123 (app) S 1 1 0 0 -1 0 0 0 0 0 200 50 0 0 20 0 3 0 900 1000000 12'
FOREGROUND = 'USCREEN_STAT ' + STAT + '\nmCurrentFocus=Window{abc u0 io.github.geraldo_netto.uscreen/com.uscreen.MainActivity}\nmShowingLockscreen=false\n'


class AndroidFocusSectionTests(unittest.TestCase):
    def test_t488_query_reads_focus_from_android_display_dump(self):
        # Android 16 device evidence: `windows` enumerates app windows but
        # `displays` owns mCurrentFocus. A visible window alone is insufficient.
        windows = 'Window #10 Window{abc u0 io.github.geraldo_netto.uscreen/com.uscreen.MainActivity}\n'
        displays = 'mCurrentFocus=Window{abc u0 io.github.geraldo_netto.uscreen/com.uscreen.MainActivity}\n'

        def shell(arguments, **kwargs):
            query = arguments[-1]
            sections = {'dumpsys window windows': windows,
                        'dumpsys window displays': displays,
                        'dumpsys window policy': 'mShowingLockscreen=false\n'}
            output = 'USCREEN_STAT ' + STAT + '\n'
            output += ''.join(text for command, text in sections.items() if command in query)
            return SimpleNamespace(returncode=0, stdout=output)

        with patch('android_session.subprocess.run', side_effect=shell):
            result = snapshot('tablet')
        self.assertTrue(result['foreground'])


class TabletSessionTests(unittest.TestCase):
    def test_t470_fake_adb_accepts_foreground_and_uses_bounded_read_only_query(self):
        def run(args, **kwargs):
            self.assertEqual(args[:4], ['adb', '-s', 'fixture', 'shell'])
            self.assertEqual(kwargs['timeout'], 2)
            self.assertNotIn('am start', args[-1])
            return SimpleNamespace(returncode=0, stdout=FOREGROUND)
        with patch('android_session.subprocess.run', side_effect=run):
            self.assertEqual(snapshot('fixture'), dict(pid=123, start_ticks=900, foreground=True))

    def test_t470_background_unknown_focus_and_keyguard_are_rejected(self):
        cases = [FOREGROUND.replace('io.github.geraldo_netto.uscreen/', 'com.other/'),
                 FOREGROUND.replace('mCurrentFocus=', 'oldFocus='),
                 FOREGROUND.replace('mShowingLockscreen=false', 'mShowingLockscreen=true')]
        for text in cases:
            with self.subTest(text=text), self.assertRaises(ValueError):
                parse_snapshot(text)

    def test_t470_adb_failure_and_timeout_reject_measurement(self):
        with patch('android_session.subprocess.run', return_value=SimpleNamespace(returncode=1, stdout='')):
            with self.assertRaises(ValueError):
                snapshot('fixture')
        with patch('android_session.subprocess.run', side_effect=subprocess.TimeoutExpired('adb', 2)):
            with self.assertRaises(subprocess.TimeoutExpired):
                snapshot('fixture')

    def test_t470_monitor_latches_pid_reuse_background_and_query_failure(self):
        original = dict(pid=123, start_ticks=900, foreground=True)
        for changed in [dict(original, pid=456), dict(original, start_ticks=901),
                        dict(original, foreground=False), ValueError('ADB failed')]:
            with self.subTest(changed=changed), tempfile.TemporaryDirectory() as directory:
                values = iter([original, changed])
                def probe(_):
                    value = next(values)
                    if isinstance(value, Exception):
                        raise value
                    return value
                monitor = AndroidSessionMonitor('fixture', 123, Path(directory), probe=probe)
                try:
                    monitor.start()
                    monitor.thread.join(2)
                    self.assertIn('Android observation failed', monitor.problem())
                finally:
                    monitor.close()
                self.assertEqual(len((Path(directory) / 'android-session.jsonl').read_text().splitlines()), 1)


if __name__ == '__main__':
    unittest.main()
