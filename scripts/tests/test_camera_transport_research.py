"""T608: validate research measurements' authentication, bounds and recovery policy."""
import importlib.util
from pathlib import Path
import sys
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1] / 'benchmarks'
SPEC = importlib.util.spec_from_file_location('camera_datagram', ROOT / 'camera_datagram.py')
WIRE = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = WIRE
SPEC.loader.exec_module(WIRE)


class DatagramResearchTests(unittest.TestCase):
    def test_t608_authenticated_fragments_reject_tamper_and_truncation(self):
        key = bytes(range(32))
        parts = list(WIRE.fragments(key, 0, b'x' * 3000))
        self.assertEqual(len(parts), 3)
        for part in parts:
            self.assertIsNotNone(WIRE.authenticated(key, part, 1))
            self.assertIsNone(WIRE.authenticated(key, part, 0))
            for offset in range(len(part)):
                bad = bytearray(part)
                bad[offset] ^= 1
                self.assertIsNone(WIRE.authenticated(key, bad, 1))
            for length in range(len(part)):
                self.assertIsNone(WIRE.authenticated(key, part[:length], 1))

    def test_t608_reorders_fragments_bounds_window_and_recovers_at_keyframe(self):
        packets = [b'a' * 1600, b'b' * 1500, b'c', b'd']
        key = bytes(range(32))
        receiver = WIRE.Receiver(packets, {0, 2}, key, 0)
        for part in reversed(list(WIRE.fragments(key, 0, packets[0]))):
            receiver.insert(part)
        with patch.object(WIRE.time, 'monotonic', return_value=.01):
            receiver.drain()
        receiver.insert(list(WIRE.fragments(key, 1, packets[1]))[0])
        receiver.insert(next(WIRE.fragments(key, 2, packets[2])))
        with patch.object(WIRE.time, 'monotonic', return_value=.19):
            receiver.drain()
        self.assertEqual([r['sequence'] for r in receiver.accepted], [0, 2])
        self.assertEqual(receiver.expired, 1)
        self.assertLessEqual(receiver.peak, sum(map(len, packets)))
        receiver.insert(next(WIRE.fragments(key, 0, packets[0])))
        self.assertNotIn(0, receiver.pending)
        with patch.object(WIRE.time, 'monotonic', return_value=1):
            receiver.drain()
        self.assertEqual(receiver.pending, {})

    def test_t608_incomplete_reference_requires_keyframe(self):
        packets = [b'a', b'b', b'c']
        key = bytes(range(32))
        receiver = WIRE.Receiver(packets, {0}, key, 0)
        receiver.insert(next(WIRE.fragments(key, 1, packets[1])))
        receiver.insert(next(WIRE.fragments(key, 2, packets[2])))
        with patch.object(WIRE.time, 'monotonic', return_value=.16):
            receiver.drain()
        self.assertEqual(receiver.accepted, [])
        self.assertEqual(len(receiver.complete), 2)

    def test_t608_future_frames_cannot_grow_reassembly(self):
        packets = [b'x'] * 100
        key = bytes(range(32))
        receiver = WIRE.Receiver(packets, {0}, key, 0)
        for sequence in range(100):
            receiver.insert(next(WIRE.fragments(key, sequence, packets[sequence])))
        self.assertEqual(len(receiver.pending), WIRE.WINDOW)
        self.assertEqual(receiver.peak, WIRE.WINDOW)


if __name__ == '__main__':
    unittest.main()
