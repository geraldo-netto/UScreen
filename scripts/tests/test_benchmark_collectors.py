"""T497: private collector fixtures retain evidence and fail on missing observations."""
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import Mock, patch

BENCH = Path(__file__).resolve().parents[1] / 'benchmarks'
sys.path.insert(0, str(BENCH))
import rect_surface as surface
import rect_trace as trace


def load(name):
    spec = importlib.util.spec_from_file_location(name, BENCH / (name + '.py'))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def trace_fixture():
    frames = [[index * 1000, index * 1_000_000_000 + 10_000_000, 0] for index in range(1, 13)]
    result = dict(completed=True, mode='video', before=dict(elapsed_ns=0), after=dict(elapsed_ns=12_000_000_000),
                  trace=[[index, index * 1_000_000_000] for index in range(1, 13)])
    records = [dict(layer="fixture's Surface", frames=frames)]
    events = [dict(frame_number=index + 7, name='PresentFenceSignaled', ts=frame[1] + 1000)
              for index, frame in enumerate(frames, 1)]
    events += [dict(frame_number=index + 7, name='Queue', ts=index * 1_000_000_000 + 1000)
               for index in range(1, 13)]
    return result, records, events


def battery_counters():
    samples = []
    for index in range(10):
        values = {'batt.current_ua': 360000, 'batt.charge_uah': 100000 + index * 500,
                  'batt.voltage_uv': 4000000, 'batt.capacity_pct': 50}
        samples.extend(dict(ts=index * 5_000_000_000 + 1000, name=name, value=value)
                       for name, value in values.items())
    return samples


class CollectorTests(unittest.TestCase):
    def test_t497_battery_report_keeps_sign_settling_window_and_repeated_trials(self):
        module = load('rect-power-report')
        result = dict(completed=True, scene='static', case='avc', send_fps=1,
                      before=dict(elapsed_ns=0, process_cpu_ms=0),
                      after=dict(elapsed_ns=45_000_000_000, process_cpu_ms=4500), stats=dict(rendered=45))
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for name in ['one', 'two']:
                replay = root/name/'replay/trial'
                replay.mkdir(parents=True)
                (replay/'result.json').write_text(json.dumps(result))
                (root/name/'power.pftrace').write_bytes(b'fixture')
            responses = [[], [dict(ts=1000, clock_value=0)], battery_counters()] * 2
            with patch.object(sys, 'argv', ['rect-power-report', str(root), '--processor', 'fixture']), \
                    patch.object(module, 'query', side_effect=responses):
                module.main()
            report = json.loads((root/'summary.json').read_text())
            self.assertEqual(report['completed_phases'], 2)
            self.assertEqual(report['groups'][0]['trials'], 2)
            self.assertEqual(report['groups'][0]['mean_current_ma'], 360)
            battery = report['trials'][0]['battery']
            self.assertEqual(battery['seconds'], 15)
            self.assertEqual(battery['integrated_net_charge_mah'], 1.5)
            self.assertEqual(battery['gauge_net_charge_mah'], 1.5)
            self.assertEqual(report['trials'][0]['cpu_percent_one_core'], 10)
            self.assertIn('No USB input power measurement', report['boundary'])

    def test_t497_battery_report_rejects_missing_replay_trace_loss_and_invalid_properties(self):
        module = load('rect-power-report')
        with patch.object(module, 'query', return_value=[dict(name='data_loss', value=1)]):
            with self.assertRaisesRegex(ValueError, 'data loss'): module.counters('fixture', 'trace')
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            with self.assertRaisesRegex(ValueError, 'expected one replay'): module.trial(root, 'fixture')
            replay = root/'replay/trial'
            replay.mkdir(parents=True)
            (replay/'result.json').write_text('{"completed":false}')
            with self.assertRaisesRegex(ValueError, 'incomplete/verification'): module.trial(root, 'fixture')
        values = {'batt.current_ua': 0, 'batt.charge_uah': 100, 'batt.voltage_uv': 4000000, 'batt.capacity_pct': 50}
        for field, value in [('batt.capacity_pct', -1), ('batt.capacity_pct', 101),
                             ('batt.voltage_uv', 0), ('batt.current_ua', float('inf'))]:
            with self.subTest(field=field, value=value), self.assertRaises(ValueError):
                module.validate_counters(dict(values, **{field: value}), values.keys())

    def test_t497_surface_collector_records_only_its_private_layer_and_stops(self):
        with tempfile.TemporaryDirectory() as directory:
            sampler = surface.PresentationSampler('fixture-device', 'fixture.app', Path(directory))
            calls = []

            def reply(command, **kwargs):
                calls.append(command)
                self.assertTrue(kwargs['check'])
                if command[-1].endswith('--list'):
                    return SimpleNamespace(stdout='SurfaceView[fixture.app/.Main](BLAST)#1')
                sampler.stop.set()
                return SimpleNamespace(stdout='16666666\n1000 2000 3000\n')

            with patch.object(surface.subprocess, 'run', side_effect=reply):
                sampler.thread.start()
                sampler.thread.join(timeout=2)
                sampler.close()
            self.assertIsNone(sampler.error)
            self.assertFalse(sampler.thread.is_alive())
            rows = [json.loads(line) for line in (Path(directory)/'surface.jsonl').read_text().splitlines()]
            self.assertEqual(rows[0]['frames'], [[1000, 2000, 3000]])
            self.assertEqual(rows[0]['period_ns'], 16666666)
            self.assertGreaterEqual(rows[0]['collection_ns'], 0)
            self.assertEqual(calls[0][:4], ['adb', '-s', 'fixture-device', 'shell'])
            self.assertIn("'SurfaceView[fixture.app/.Main](BLAST)#1'", calls[1][-1])

    def test_t497_surface_collector_keeps_parse_command_and_retirement_errors(self):
        with tempfile.TemporaryDirectory() as directory:
            sampler = surface.PresentationSampler('fixture', 'fixture.app', Path(directory))
            with patch.object(surface.subprocess, 'run', side_effect=subprocess.TimeoutExpired('fixture', 5)):
                sampler.thread.start()
                sampler.thread.join(timeout=2)
                sampler.close()
            self.assertIn('timed out', sampler.error)
            sampler.thread = Mock()
            sampler.thread.is_alive.return_value = True
            sampler.close()
            self.assertEqual(sampler.error, 'presentation collector did not stop')
            sampler.thread.join.assert_called_once_with(timeout=6)
        for value in ['', 'invalid', '1\n1 2', '1\n1 2 3 4', '1\na b c']:
            with self.subTest(value=value), self.assertRaises(ValueError): surface.parse(value)

    def test_t497_trace_diagnosis_preserves_identity_and_escapes_layer_names(self):
        result, records, events = trace_fixture()
        replies = [[], events, [dict(ts=1000, clock_value=0), dict(ts=2000, clock_value=1000)]]
        with patch.object(trace, 'query', side_effect=replies) as query:
            report = trace.diagnose(Path('fixture.trace'), 'fixture-processor', result, records)
        self.assertEqual(report['mapping'], dict(frame_offset=7, validated_pairs=12))
        self.assertEqual(report['eligible'], 9)
        self.assertEqual(report['classification']['presented_in_surface_history'], list(range(1, 10)))
        self.assertIn("layer_name='fixture''s Surface'", query.call_args_list[1].args[2])
        raw = '"frame_number","name","ts"\n8,"Queue",1000\n'
        with patch.object(trace.subprocess, 'run', return_value=SimpleNamespace(stdout=raw)) as command:
            self.assertEqual(trace.query('processor', 'trace', 'SELECT fixture'),
                             [dict(frame_number='8', name='Queue', ts='1000')])
        self.assertEqual(command.call_args.args[0], ['processor', 'query', 'trace', 'SELECT fixture'])
        self.assertEqual(command.call_args.kwargs['timeout'], 30)

    def test_t497_trace_cli_publishes_only_complete_hardware_video_evidence(self):
        module = load('rect-trace-report')
        result, records, events = trace_fixture()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root/'result.json').write_text(json.dumps(result))
            (root/'surface.jsonl').write_text('\n'.join(map(json.dumps, records)))
            arguments = ['rect-trace-report', '--processor', 'fixture', '--trace', str(root/'trace'),
                         '--trial', str(root), '--output', str(root/'report.json')]
            replies = [[], events, [dict(ts=1000, clock_value=0)]]
            with patch.object(sys, 'argv', arguments), patch.object(trace, 'query', side_effect=replies):
                module.main()
            report = json.loads((root/'report.json').read_text())
            self.assertEqual(report['queue_frames'], 12)
            for change in [dict(completed=False), dict(mode='rect')]:
                (root/'result.json').write_text(json.dumps(dict(result, **change)))
                with patch.object(sys, 'argv', arguments), self.assertRaises(ValueError): module.main()

    def test_t497_trace_invalid_identifiers_and_incomplete_windows_are_rejected(self):
        result, records, _events = trace_fixture()
        with self.assertRaisesRegex(ValueError, 'ambiguous replay layer'):
            trace.diagnose('trace', 'fixture', result, records + [dict(layer='different', frames=[])])
        with patch.object(trace, 'query', return_value=[dict(name='lost', value=1)]):
            with self.assertRaisesRegex(ValueError, 'trace errors'): trace.diagnose('trace', 'fixture', result, records)
        invalid = [dict(frames=[[-1, 10, 0], [1001, 10, 0], [1000, 2**63 - 1, 0]])]
        self.assertEqual(trace.surface_sequences(invalid), {})
        records[0]['frames'].append([1000, 99, 0])
        with self.assertRaisesRegex(ValueError, 'ambiguous SurfaceFlinger'): trace.surface_sequences(records)
        result['trace'] = []
        with self.assertRaisesRegex(ValueError, 'empty interior'): trace.coverage(result, {}, {}, 0, {}, 'layer')


if __name__ == '__main__':
    unittest.main()
