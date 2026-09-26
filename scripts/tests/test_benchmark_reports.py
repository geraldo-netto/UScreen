"""T497: report CLIs retain units, trial groups, rejected samples and clock boundaries."""
import copy
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]/'benchmarks'


def load(name):
    spec = importlib.util.spec_from_file_location(name.replace('-', '_'), ROOT/(name+'.py'))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def write(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value))


def run(name, *args):
    result = subprocess.run([sys.executable, str(ROOT/(name+'.py')), *map(str, args)],
                            capture_output=True, text=True, timeout=10)
    if result.returncode:
        raise AssertionError(result.stdout + result.stderr)


def decoder_trial():
    thread = dict(tid=1, voluntary_ctxt_switches=10, nonvoluntary_ctxt_switches=5)
    before = dict(elapsed_ns=1_000_000_000, process_cpu_ms=0, threads=[thread],
                  runtime={'art.gc.'+name: '0' for name in ['bytes-allocated', 'gc-count', 'gc-time']})
    after = copy.deepcopy(before)
    after.update(elapsed_ns=3_000_000_000, process_cpu_ms=500)
    after['threads'][0].update(voluntary_ctxt_switches=15, nonvoluntary_ctxt_switches=8)
    after['runtime']['art.gc.bytes-allocated'] = '1024'
    dequeues = dict(input_calls=40, output_calls=80, input_ns=1000, output_ns=2000)
    return dict(completed=True, scene='motion', send_fps=60, variant='candidate', profile='unhinted', trial=0,
                before=before, after=after, sent=3,
                stats=dict(rendered=3, invalidations=0, duplicates=0, arrival_to_callback_us=[1000, 2000, 3000]),
                dequeues_before={key: 0 for key in dequeues}, dequeues_after=dequeues,
                trace=[[1, 1_000_000, 3_000_000, 5_000_000, 4_000_000, 6_000_000]])


def raw_trial():
    return dict(width=4, height=4, frames=2, sessions=1, mode='rows', size=24,
                transport='memfd', allocation='plain', streams=[dict(ns=1_000_000_000,
                cpu_ns=2_000_000, age_ns=[1_000_000, 3_000_000], reads=4, touch_ns=10,
                touch_minor_faults=2, touch_major_faults=0, huge_kb=0, advice_accepted=1)])


def profile_trial():
    frames = [dict(admitted_ns=index * 1_000_000_000) for index in range(4)]
    packets = [dict(sequence=index + 1, ready_ns=frame['admitted_ns'] + 2_000_000,
                    bytes=2, pts=index + 1) for index, frame in enumerate(frames)]
    acks = [dict(sequence=index + 1, acknowledged_ns=frame['admitted_ns'] + 12_000_000)
            for index, frame in enumerate(frames)]
    return dict(android=dict(completed=True), acknowledgements=acks,
                setups=[dict(setup_us=2000)], phases=[dict(frames=frames, packets=packets)])


def profile_decoder():
    choice = dict(name='decoder', stream=dict(codec='avc', profile=66, level=31, depth=8),
                  low_latency=False, operating_rate=None)
    return dict(completed=True, selection=dict(decoder_selection=choice),
                selection_receipt='7:decoder:avc:66:31:8:0:0', scene='text', send_fps=1,
                trial=0, sent=2, stats=dict(rendered=2), setup_us=1000,
                trace=[[1, 1_000_000, 6_000_000], [2, 2_000_000, 7_000_000]])


def rect_trial():
    value = decoder_trial()
    value.update(mode='rect', case='lz4', count=1, compressed_storage_bytes=100, input_mode='read')
    value['after']['elapsed_ns'] = 8_000_000_000
    value['trace'] = [[3_000_000_000, 3_000_000_000, 3_001_000_000, 3_002_000_000,
                       3_003_000_000, 20, 1, 3_010_000_000]]
    return value


def rect_observations(root, value):
    write(root/'result.json', value)
    (root/'memory-com.blent.rectbench.txt').write_text('TOTAL PSS: 2048 TOTAL RSS: 4096 TOTAL SWAP PSS: 0')
    samples = [dict(app=dict(pid=1, start_ticks=10, ticks=100 + index * 50, at_ns=index * 1_000_000_000))
               for index in range(7)]
    (root/'resources.jsonl').write_text('\n'.join(map(json.dumps, samples)))
    (root/'surface.jsonl').write_text(json.dumps(dict(frames=[[1000, 3_010_000_000, 0], [0, 2**63 - 1, 0]])))


class ReportTests(unittest.TestCase):
    def test_t497_profile_report_keeps_usb_and_decoder_boundaries_separate(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            write(root/'decoder/one/result.json', profile_decoder())
            write(root/'usb/trial-0/result.json', profile_trial())
            write(root/'usb/trial-0/request.json', dict(scene='text', encoder='h264_vaapi', rate=1))
            run('summarize-profile-selection', '--decoder', root/'decoder', '--usb', root/'usb', '--output', root/'summary.json')
            result = json.loads((root/'summary.json').read_text())
            self.assertEqual(result['decoder_summary'][0]['median_run_percentiles']['p95'], 5)
            self.assertEqual(result['usb_summary'][0]['median_run_percentiles']['p95'], 12)
            self.assertEqual(result['usb_phases'][0]['packet_ready_to_ack_ms']['p95'], 10)
            self.assertEqual(result['usb_phases'][0]['samples'], 3)

    def test_t497_profile_report_rejects_duplicates_interruptions_and_receipt_changes(self):
        with patch.object(sys, 'path', [str(ROOT), *sys.path]):
            module = load('summarize-profile-selection')
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            write(root/'trial-0/request.json', dict(scene='text', encoder='h264_vaapi', rate=1))
            for field, value in [('android', dict(completed=False)), ('acknowledgements', [])]:
                broken = profile_trial()
                broken[field] = value
                write(root/'trial-0/result.json', broken)
                with self.assertRaises(ValueError): module.usb(root)
            broken = profile_trial()
            broken['acknowledgements'].append(broken['acknowledgements'][0])
            write(root/'trial-0/result.json', broken)
            with self.assertRaisesRegex(ValueError, 'duplicate ACK'): module.usb(root)
            write(root/'trial-0/result.json', dict(profile_decoder(), selection_receipt='stale'))
            with self.assertRaisesRegex(ValueError, 'mismatched'): module.decoder(root)
        with self.assertRaisesRegex(ValueError, 'missing timing'): module.percentiles([])

    def test_t497_rect_report_preserves_swap_presentation_and_resource_windows(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            write(root/'metadata.json', dict(verification=False, clock_ticks=100))
            rect_observations(root/'one', rect_trial())
            video = rect_trial()
            video.update(mode='video', case='avc', trace=[[1, 3_000_000_000, 3_002_000_000]])
            rect_observations(root/'two', video)
            run('summarize-rect', root)
            result = json.loads((root/'summary.json').read_text())
            self.assertEqual(result['completed_trials'], 2)
            self.assertEqual(result['trials'][0]['metrics']['admission_to_swap_ms']['p50'], 3)
            self.assertEqual(result['trials'][0]['surface']['admission_to_surface_present_ms']['p50'], 10)
            self.assertEqual(result['trials'][0]['services']['app']['cpu_percent_one_core'], 50)
            self.assertEqual(result['cohorts'][0]['sampled_app_pss_mib']['p50'], 2)
            self.assertEqual(result['trials'][1]['surface']['joined'], 1)

    def test_t497_rect_report_missing_evidence_and_backwards_intervals_are_rejected(self):
        module = load('summarize-rect')
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            value = rect_trial()
            rect_observations(root, value)
            (root/'surface.jsonl').unlink()
            self.assertIsNone(module.surface_metrics(root, value))
            (root/'resources.jsonl').write_text('{}\n')
            self.assertEqual(module.service_cpu(root, 100), {})
            (root/'memory-com.blent.rectbench.txt').write_text('missing')
            with self.assertRaisesRegex(ValueError, 'missing memory'): module.memory(root)
            for change in [dict(completed=False), dict(verified=True)]:
                write(root/'result.json', dict(value, **change))
                with self.assertRaisesRegex(ValueError, 'incomplete or verification'): module.trial(root, {})
        for end, milliseconds in [(0, 10), (-1, 10), (1, -1)]:
            with self.assertRaisesRegex(ValueError, 'invalid CPU interval'):
                module.cpu(dict(elapsed_ns=0, process_cpu_ms=0), dict(elapsed_ns=end, process_cpu_ms=milliseconds))

    def test_t497_decoder_report_preserves_callback_units_and_incomplete_trials(self):
        module = load('summarize-decoder')
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            trial = decoder_trial()
            write(root/'one/result.json', trial)
            write(root/'two/result.json', dict(trial, trial=1))
            write(root/'bad/result.json', dict(completed=False, sent=2))
            with self.assertRaisesRegex(ValueError, 'incomplete'): module.summarize(root)
            run('summarize-decoder', root, '--output', root/'report', '--allow-incomplete')
            report = json.loads((root/'report/summary.json').read_text())
            self.assertEqual(len(report['trials']), 2)
            self.assertEqual(len(report['rejected']), 1)
            metrics = report['groups'][0]['metrics']
            self.assertEqual(metrics['cpu_percent_one_core']['median'], 25)
            self.assertEqual(metrics['arrival_to_callback_p50_ms']['median'], 2)
            self.assertEqual(metrics['surviving_thread_switches']['median'], 8)
            self.assertEqual(metrics['feed_to_ack_p50_ms']['median'], 5)
            self.assertIn('candidate/unhinted', (root/'report/table.md').read_text())
        self.assertIsNone(module.percentile([], .5))
        self.assertIsNone(module.distribution([None]))

    def test_t497_raw_and_ring_report_cannot_relabel_transport_fps_as_display_fps(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            raw = raw_trial()
            ring = copy.deepcopy(raw)
            for field in ['touch_ns', 'touch_minor_faults', 'touch_major_faults', 'huge_kb', 'advice_accepted']:
                del raw['streams'][0][field]
            write(root/'raw.json', dict(trials=[raw, dict(raw, mode='direct')]))
            write(root/'ring.json', dict(trials=[ring, ring]))
            run('summarize-raw-input', '--raw', root/'raw.json', '--ring', root/'ring.json', '--output', root/'report')
            result = json.loads((root/'report/summary.json').read_text())
            self.assertEqual(len(result['raw']), 2)
            self.assertEqual(result['ring'][0]['trials'], 2)
            self.assertEqual(result['raw'][0]['metrics']['fps']['median'], 2)
            self.assertEqual(result['raw'][0]['metrics']['cpu_ms_per_frame']['median'], 1)
            self.assertEqual(result['ring'][0]['allocation_name'], 'plain')
            self.assertEqual(result['ring'][0]['allocation']['touch_minor_faults']['median'], 2)
            self.assertIn('memfd / plain', (root/'report/tables.md').read_text())

    def test_t497_readiness_report_uses_worst_lane_and_keeps_idle_metrics_unknown(self):
        module = load('summarize-readiness')
        row = dict(variant='eventfd', size=24, sessions=1, frames=2, mode='paced', capacities=[4096],
                   cpu_ns=10_000_000, cancel_ns=3_000_000, voluntary_switches=6,
                   readers=[dict(ages_ns=[1_000_000, 2_000_000], reads=2, ended_ns=4_000_000)],
                   writers_write_poll=[[2, 1]])
        idle = copy.deepcopy(row)
        idle.update(mode='idle', cancel_ns=0)
        idle['readers'][0]['ages_ns'] = []
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            write(root/'trials.json', dict(trials=[row, row, idle]))
            run('summarize-readiness', root/'trials.json', root/'report.json')
            result = {item['mode']: item for item in json.loads((root/'report.json').read_text())}
            self.assertEqual(result['paced']['trials'], 2)
            self.assertEqual(result['paced']['receipt_p99_ms']['median'], 2)
            self.assertEqual(result['paced']['cancel_ms']['median'], 1)
            self.assertNotIn('receipt_p99_ms', result['idle'])
            self.assertNotIn('cancel_ms', result['idle'])
        self.assertEqual(module.p99(list(range(100))), 98)

    def test_t497_decoder_schema_mutations_fail_without_publishing_partial_reports(self):
        module = load('summarize-decoder')
        required = ['before', 'after', 'stats', 'dequeues_before', 'dequeues_after']
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for field in required:
                broken = decoder_trial()
                del broken[field]
                write(root/'trial/result.json', broken)
                with self.assertRaises(KeyError): module.summarize(root)
