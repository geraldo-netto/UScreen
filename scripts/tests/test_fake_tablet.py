"""T103: TCP boundaries must not change the fake tablet's protocol."""
import importlib.util
from pathlib import Path
import struct
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('fake_tablet', Path(__file__).parents[1] / 'fake-tablet.py')
tablet = importlib.util.module_from_spec(spec)
spec.loader.exec_module(tablet)


class FragmentedSocket:
    def __init__(self, chunks):
        self.chunks = list(chunks)
        self.sent = bytearray()

    def recv(self, size):
        if not self.chunks:
            return b''
        chunk = self.chunks.pop(0)
        if len(chunk) > size:
            self.chunks.insert(0, chunk[size:])
        return chunk[:size]

    def send(self, data):
        self.sent.extend(data[:1])
        return min(1, len(data))

    def sendall(self, data):
        while data:
            data = data[self.send(data):]


class PartialIoTest(unittest.TestCase):
    def test_t327_video_duration_ignores_wall_clock_corrections(self):
        packets = [b'\x01' + struct.pack('>I', seq) + b'frame' for seq in range(1, 4)]
        chunks = [struct.pack('>I', len(packet)) + packet for packet in packets]
        for wall_times in [[1000, 1000, 2000], [1000, 1000, 0, 0, 1003]]:
            with self.subTest(wall_times=wall_times), \
                    patch.object(tablet.time, 'time', side_effect=wall_times), \
                    patch.object(tablet.time, 'monotonic', side_effect=[10, 10, 11, 12]), \
                    patch.object(tablet, 'ws_send') as send:
                control = object()
                result = tablet.receive_video(FragmentedSocket(chunks), control, 2)
                self.assertEqual(result, (False, 2, 2),
                                 'T327: wall-clock correction changed the elapsed video window')
                self.assertEqual([call.args[1]['seq'] for call in send.call_args_list], [1, 2])

    def test_t103_upgrade_preserves_overread_and_short_writes(self):
        sock = FragmentedSocket([b'HTTP/1.1 101 Switching Protocols\r\n', b'\r\n\x81\x05hello'])
        with patch.object(tablet.socket, 'create_connection', return_value=sock):
            ws = tablet.ws_connect(8891)
        self.assertTrue(sock.sent.endswith(b'\r\n\r\n'))
        self.assertEqual(tablet.ws_recv_text(ws), 'hello')

    def test_t103_fragmented_headers_and_payloads(self):
        for payload in [b'hi', b'a' * 126, b'b' * 65536]:
            size = len(payload)
            header = bytes([0x81, size]) if size < 126 else (
                b'\x81\x7e' + struct.pack('>H', size) if size < 65536 else
                b'\x81\x7f' + struct.pack('>Q', size))
            sock = FragmentedSocket([bytes([b]) for b in header] + [payload[:3], payload[3:]])
            self.assertEqual(tablet.ws_recv_text(sock), payload.decode())

    def test_t103_auth_writes_whole_frame(self):
        sock = FragmentedSocket([])
        tablet.ws_send(sock, {'type': 'auth', 'token': 'test'})
        self.assertEqual(len(sock.sent), 6 + (sock.sent[1] & 0x7f))

    def test_t103_truncated_payload_is_not_valid_text(self):
        self.assertIsNone(tablet.ws_recv_text(FragmentedSocket([b'\x81\x05abc'])))

    def test_t207_video_packets_keep_config_and_ack_sequence(self):
        packets = [b'\x00codec', b'\x01' + struct.pack('>I', 42) + b'frame']
        chunks = [struct.pack('>I', len(packet)) + packet for packet in packets]
        with patch.object(tablet, 'ws_send') as send:
            control = object()
            result = tablet.receive_video(FragmentedSocket(chunks), control, 5)
        self.assertEqual(result, (True, 1, 42))
        send.assert_called_once_with(control, {'type': 'rendered', 'seq': 42, 'decode_us': 1000})
        self.assertEqual(tablet.receive_video(FragmentedSocket([]), control, 5), (False, 0, None))


if __name__ == '__main__':
    unittest.main()
