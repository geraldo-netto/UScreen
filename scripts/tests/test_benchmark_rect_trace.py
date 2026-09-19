"""T419: missing presentation is not automatically a dropped frame."""
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'benchmarks'))
import rect_trace as trace


class RectangleTraceTests(unittest.TestCase):
    def test_t419_requires_stable_clock_conversion(self):
        self.assertEqual(trace.clock_offset([dict(ts=10000, clock_value=5000),
                                             dict(ts=20000, clock_value=15010)]), 4995)
        for values in [[], [dict(ts=10000, clock_value=0), dict(ts=20000, clock_value=0)]]:
            with self.assertRaises(ValueError):
                trace.clock_offset(values)

    def test_t419_identity_comes_from_matching_presentation_times(self):
        frames = {i + 7: {'PresentFenceSignaled': {i * 10000 + 1000}} for i in range(1, 20)}
        surface = {i: i * 10000 for i in range(1, 20)}
        self.assertEqual(trace.identity(frames, surface, 1000), dict(frame_offset=7, validated_pairs=19))
        frames[99] = frames.pop(26)
        with self.assertRaises(ValueError):
            trace.identity(frames, surface, 1000)

    def test_t419_missing_queue_is_unknown_not_a_dropped_frame(self):
        mapping = dict(frame_offset=3)
        frames = {4: {'Queue': {10}}, 5: {'Queue': {20}, 'Latch': {30}},
                  6: {'Queue': {40}, 'Latch': {50}, 'PresentFenceSignaled': {60}}}
        self.assertEqual(trace.classify(1, mapping, frames, {}), 'queued_not_latched')
        self.assertEqual(trace.classify(2, mapping, frames, {}), 'latched_without_present_fence')
        self.assertEqual(trace.classify(3, mapping, frames, {}), 'present_fence_without_history')
        self.assertEqual(trace.classify(4, mapping, frames, {}), 'unknown_missing_queue')

    def test_t419_partial_trace_cannot_explain_an_entire_measurement_window(self):
        result = dict(before=dict(elapsed_ns=0), after=dict(elapsed_ns=5_000_000_000),
                      trace=[[1, 2_000_000_000]])
        with self.assertRaises(ValueError):
            trace.coverage(result, {1: {'Queue': {2_000_000_000}}}, {}, 0, dict(frame_offset=0), 'layer')


if __name__ == '__main__':
    unittest.main()
