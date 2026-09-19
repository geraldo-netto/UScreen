"""T492: measured join-point gaps must survive sparse replay summarization."""
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'benchmarks'))
import profile_usb_pipeline as pipeline
REPORT = pipeline.module('summarize-idle-cadence')


class IdleCadenceTests(unittest.TestCase):
    def example(self):
        phase = dict(frames=[{}, {}, {}], packets=[
            dict(sequence=i + 1, bytes=10, pts=i * 1_000_000, ready_ns=i * 1_000_000_000)
            for i in range(3)])
        probed = [dict(pos=str(i * 10), size='10', flags='K_' if i != 1 else '__')
                  for i in range(3)]
        return phase, probed

    def test_t492_two_second_join_gap_is_not_confused_with_one_second_input(self):
        phase, probed = self.example()
        result = REPORT.keyframes(phase, probed)
        self.assertEqual(result['max_inter_keyframe_ms'], 2000)
        self.assertEqual([row['sequence'] for row in result['keys']], [1, 3])

    def test_t492_probe_positions_and_packet_counts_must_match(self):
        phase, probed = self.example()
        with self.assertRaises(ValueError):
            REPORT.keyframes(phase, probed[:-1])
        probed[1]['pos'] = '11'
        with self.assertRaises(ValueError):
            REPORT.keyframes(phase, probed)

    def test_t492_missing_initial_keyframe_or_backward_clock_invalidates_evidence(self):
        phase, probed = self.example()
        probed[0]['flags'] = '__'
        with self.assertRaises(ValueError):
            REPORT.keyframes(phase, probed)
        probed[0]['flags'] = 'K_'
        phase['packets'][2]['ready_ns'] = -1
        with self.assertRaises(ValueError):
            REPORT.keyframes(phase, probed)


if __name__ == '__main__':
    unittest.main()
