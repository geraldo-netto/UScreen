"""T382: measurement units, process identity and window semantics stay explicit."""
import gzip
import json
from pathlib import Path
import sys
import tempfile
import unittest
import re

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'benchmarks'))
from observe import battery_values, process_stat, filtered_logs
from summarize import battery_summary, cpu_delta, load_lines, window_values


class BaselineMeasurementTests(unittest.TestCase):
    def test_t457_current_and_historical_labels_have_identical_boundaries(self):
        historical = [dict(message='Latency encode→display: p50 20ms  p95 90ms  max 90ms  (3 samples'),
                      dict(message='of which tablet decode+render p50 30ms  p95 30ms → wire ~0ms')]
        current = [dict(message='Latency packet-ready→render-ACK (host clock): p50 20ms  p95 90ms  max 90ms  (3 samples'),
                   dict(message='Latency tablet arrival→render-callback (tablet clock): p50 30ms  p95 30ms  (2 samples)')]
        self.assertEqual(window_values(current), window_values(historical))
        self.assertEqual(len(window_values(current)), 2)

    def test_t410_journal_byte_array_messages_are_retained(self):
        message = '\x1b[32mINFO\x1b[0m Latency encode→display: p50 5ms'
        entries = [{'__REALTIME_TIMESTAMP': '123456789', 'MESSAGE': message},
                   {'__REALTIME_TIMESTAMP': '124456789', 'MESSAGE': list(message.encode())}]
        code = 'import json; rows=' + repr(entries) + '; [print(json.dumps(r)) for r in rows]'
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / 'journal.jsonl'
            process, thread = filtered_logs([sys.executable, '-c', code], output,
                                             re.compile('Latency'), journal=True)
            self.assertEqual(process.wait(timeout=5), 0)
            thread.join(timeout=5)
            rows = [json.loads(line) for line in output.read_text().splitlines()]
            self.assertEqual(len(rows), 2, 'T410: text and binary MESSAGE forms must survive')
            self.assertEqual([row['utc'] for row in rows], [123.456789, 124.456789])
            self.assertEqual(rows[0]['message'], rows[1]['message'])
            self.assertIn('Latency encode→display', rows[1]['message'])

    def test_t382_native_stat_fields_and_cpu_units(self):
        raw = '42 (worker (name)) S 1 1 0 0 -1 0 0 0 0 0 200 50 0 0 20 0 3 0 900 1000000 12'
        before = process_stat(raw, 4096)
        self.assertEqual(before, dict(pid=42, ticks=250, start_ticks=900,
                                      rss_bytes=49152, threads=3))
        after = dict(before, ticks=450)
        self.assertEqual(cpu_delta(before, after, 2, 100), 100)
        self.assertIsNone(cpu_delta(before, dict(after, start_ticks=901), 2, 100))
        self.assertIsNone(cpu_delta(before, dict(after, pid=43), 2, 100))
        self.assertIsNone(cpu_delta(before, dict(after, ticks=249), 2, 100))
        self.assertIsNone(cpu_delta(before, after, 0, 100))

    def test_t382_charge_is_net_current_not_input_power(self):
        battery = battery_values('USB powered: true\nCharge counter: 8000000\n'
                                 'Max charging current: 500000\nMax charging voltage: 5000000\n'
                                 'level: 80\ntemperature: 310\nprivate field: omitted')
        self.assertNotIn('private field', battery)
        before = dict(utc=100, monotonic=100, battery=battery)
        after = dict(utc=160, monotonic=160, battery=dict(battery, **{'Charge counter': '7990000'}))
        result = battery_summary([before, after])
        self.assertEqual(result['net_battery_ma'], -600)
        self.assertEqual(result['charge_delta_uah'], -10000)
        self.assertEqual(result['temperature_c']['median'], 31)
        self.assertEqual(result['usb_reported_values'], [('true', '500000', '5000000')])

    def test_t382_window_percentiles_are_never_pooled(self):
        logs = [dict(message='Latency encode→display: p50 5ms  p95 6ms  max 7ms  (100 samples'),
                dict(message='Latency encode→display: p50 100ms  p95 101ms  max 102ms  (1 samples'),
                dict(message='Encoder: 300 access units in 5.0s, 0.1 MB/s (800 kbps)')]
        result = window_values(logs)
        window = result['packet_ready_to_ack_window']
        self.assertEqual(window['p50_ms']['median'], 52.5)
        self.assertEqual(window['p50_ms']['n'], 2)
        self.assertEqual(window['samples']['minimum'], 1)
        self.assertEqual(window['samples']['maximum'], 100)
        self.assertEqual(result['encoder_window']['fps']['median'], 60)

    def test_t382_compressed_raw_samples_round_trip(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            rows = [{'utc': 123, 'value': 4}, {'utc': 124, 'value': 7}]
            with gzip.open(root / 'samples.jsonl.gz', 'wt') as stream:
                stream.write(''.join(json.dumps(row) + '\n' for row in rows))
            self.assertEqual(load_lines(root, 'samples.jsonl'), rows)


if __name__ == '__main__':
    unittest.main()
