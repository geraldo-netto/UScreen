"""T575/T578: corrupt pixels and incomplete ACKs must never become speed claims."""
import importlib.util
from pathlib import Path
import unittest

PATH = Path(__file__).resolve().parents[1] / 'benchmarks/summarize-gpu-capture.py'
SPEC = importlib.util.spec_from_file_location('gpu_report', PATH)
REPORT = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(REPORT)


class GpuReportTests(unittest.TestCase):
    def test_t578_corrupt_cross_gpu_pixels_fail_even_when_packets_and_acks_succeed(self):
        with self.assertRaisesRegex(ValueError, 'corrupt'):
            REPORT.barcode(bytes([143]) * (384 * 16))
        with self.assertRaisesRegex(ValueError, 'truncated'):
            REPORT.barcode(b'')
        pixels = bytearray(384 * 16)
        for bit in [0, 3, 23]:
            pixels[8 * 384 + bit * 16 + 8] = 255
        self.assertEqual(REPORT.barcode(pixels), 1 + 8 + (1 << 23))

    def test_t575_first_presentation_excludes_repeated_source_frames(self):
        result = dict(packets=[dict(ready_ns=30_000_000)] * 3,
                      acknowledgements=[dict(sequence=i + 1, acknowledged_ns=(40 + i) * 1_000_000) for i in range(3)])
        scene = {7: (20_000_000, 21_000_000), 8: (30_000_000, 31_000_000)}
        report = REPORT.analyze(result, [7, 7, 8], scene, 0)
        self.assertEqual(report['unique_updates'], 2)
        self.assertEqual(report['source_update_to_ack']['p50_ms'], 16)
        self.assertEqual(report['complete_acks'], 3)
        result['acknowledgements'].pop()
        with self.assertRaisesRegex(ValueError, 'ACK'):
            REPORT.analyze(result, [7, 7, 8], scene, 0)


if __name__ == '__main__':
    unittest.main()
