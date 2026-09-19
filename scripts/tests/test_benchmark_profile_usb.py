"""T479: combined-clock USB evidence must retain framing and request identity."""
import importlib.util
import io
from pathlib import Path
import struct
import sys
from types import SimpleNamespace
import unittest
from unittest.mock import patch
import zlib

BENCH = Path(__file__).resolve().parents[1] / 'benchmarks'
sys.path.insert(0, str(BENCH))
import profile_usb_wire as WIRE
import profile_usb_pipeline as PIPELINE
SPEC = importlib.util.spec_from_file_location('profile_usb', BENCH / 'profile-usb.py')
DRIVER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(DRIVER)


class ProfileUsbTests(unittest.TestCase):
    def test_t492_sparse_usb_comparison_remains_bounded_by_duration_and_corpus(self):
        meta = dict(scenes=[dict(scene='text')], frames=240)
        row = dict(scene='text', encoder='h264_vaapi_baseline', rate=1, seconds=12)
        DRIVER.validate([row], meta)
        DRIVER.validate([dict(row, rate=5)], meta)
        for change in [dict(rate=0), dict(rate=2), dict(seconds=13),
                       dict(seconds=1), dict(rate=60)]:
            with self.subTest(change=change), self.assertRaises(ValueError):
                DRIVER.validate([dict(row, **change)], meta)

    def test_t479_tee_fragmentation_preserves_packet_boundaries(self):
        payload = b'\x00\x00\x01\x65\n\x00fixture'
        line = f'0, 1, 1, 1, {len(payload)}, 0x{zlib.adler32(payload, 0):08x}\n'.encode()
        parser = WIRE.TeePackets()
        packets = []
        for byte in b'#codec_id 0: h264\n' + line + payload:
            packets.extend(parser.feed(bytes([byte])))
        parser.finish()
        self.assertEqual(packets, [(payload, 1)])
        with self.assertRaises(ValueError):
            parser.feed(line + payload)

    def test_t479_oversized_corrupt_and_truncated_packets_are_rejected(self):
        for data in [b'a' * 513, b'0, 0, 0, 1, 8388609, 0x0\n', b'0, 0, 0, 1, 1, 0x0\nx']:
            with self.subTest(data=data[:20]), self.assertRaises(ValueError):
                WIRE.TeePackets().feed(data)
        parser = WIRE.TeePackets()
        parser.feed(b'0, 0, 0, 1, 10, 0x0\nxx')
        with self.assertRaises(ValueError):
            parser.finish()

    def test_t479_ready_receipt_and_ack_frame_identity_are_checked(self):
        acks = WIRE.Acknowledgements(None, 'expected')
        ready = b'\0' + struct.pack('!H', 8) + b'expected' + struct.pack('!Q', 12)
        acks.read(io.BytesIO(ready))
        self.assertEqual(acks.ready[0]['setup_us'], 12)
        with self.assertRaises(ValueError):
            acks.read(io.BytesIO(ready.replace(b'expected', b'retired!')))
        result = dict(phases=[dict(frames=[0, 1])], acknowledgements=[dict(sequence=1), dict(sequence=1)])
        with self.assertRaises(ValueError):
            DRIVER.acks_rows(result)
        result['acknowledgements'][1]['sequence'] = 2
        self.assertEqual(len(DRIVER.acks_rows(result)), 2)

    def test_t479_incomplete_clock_intervals_cannot_be_treated_as_acks(self):
        for raw in [b'', b'\1\0\0', b'\0\0\x08old']:
            with self.subTest(raw=raw), self.assertRaises(EOFError):
                WIRE.Acknowledgements(None, 'expected').read(io.BytesIO(raw))

    def test_t479_host_benchmark_keeps_exported_encoder_policy(self):
        profile = dict(encoder='h264_vaapi', options=[['-profile:v', 'constrained_baseline'], ['-qp', '18']])
        command = PIPELINE.command(profile, dict(width=1280, height=800, fps=60), '/test/render')
        self.assertEqual(command[command.index('-profile:v') + 1], 'constrained_baseline')
        self.assertEqual(command[command.index('-qp') + 1], '18')
        self.assertIn('[f=framecrc:flush_packets=1]pipe:1|[f=data:flush_packets=1]pipe:1', command)
        self.assertNotIn('nobuffer', command)

    def test_t479_external_foreground_prevents_benchmark_launch(self):
        with patch.object(DRIVER.DEVICE, 'capture', return_value='topResumedActivity=other.app/.Activity'):
            with self.assertRaises(RuntimeError):
                DRIVER.foreground('test')

    def test_t479_summary_joins_correlated_intervals_without_adding_percentiles(self):
        summary = PIPELINE.module('summarize-profile-selection')
        frames = [dict(admitted_ns=index * 1_000_000_000) for index in range(4)]
        packets = [dict(sequence=index + 1, ready_ns=frame['admitted_ns'] + 2_000_000)
                   for index, frame in enumerate(frames)]
        acks = {index + 1: dict(acknowledged_ns=frame['admitted_ns'] + 12_000_000)
                for index, frame in enumerate(frames)}
        result = summary.usb_phase(dict(frames=frames, packets=packets), acks, 0, 1)
        self.assertEqual(result['raw_write_to_ack_ms']['p95'], 12)
        self.assertEqual(result['packet_ready_to_ack_ms']['p95'], 10)
        acks[4]['acknowledged_ns'] = 0
        with self.assertRaises(ValueError):
            summary.usb_phase(dict(frames=frames, packets=packets), acks, 0, 1)


if __name__ == '__main__':
    unittest.main()
