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

    def recv_into(self, buffer):
        data = self.recv(len(buffer))
        buffer[:len(data)] = data
        return len(data)

    def send(self, data):
        self.sent.extend(data[:1])
        return min(1, len(data))

    def sendall(self, data):
        while data:
            data = data[self.send(data):]


class PartialIoTest(unittest.TestCase):
    def test_t408_exact_binary_reads_preserve_every_byte_and_eof(self):
        payload = bytes(range(256)) * 5
        chunks = [payload[i:i + 3] for i in range(0, len(payload), 3)]
        socket = FragmentedSocket(chunks)
        self.assertEqual(tablet.read_exact(socket, len(payload)), payload)
        self.assertEqual(tablet.read_exact(socket, 0), b'')
        with self.assertRaises(EOFError):
            tablet.read_exact(socket, 1)

    def test_t408_upgrade_leftovers_precede_recv_into_socket_data(self):
        socket = tablet.BufferedSocket(FragmentedSocket([b'ef']), b'abcd')
        received = bytearray(6)
        offset = 0
        while offset < len(received):
            count = socket.recv_into(memoryview(received)[offset:])
            if not count:
                break
            offset += count
        self.assertEqual(received, b'abcdef')

    def test_t408_large_video_uses_bounded_reads_and_exact_ack(self):
        class RecordingSocket(FragmentedSocket):
            largest = 0
            def recv(self, size):
                self.largest = max(self.largest, size)
                return super().recv(size)
        payload = b'x' * (2 * 1024 * 1024)
        packet = struct.pack('>I', len(payload) + 5) + b'\x01' + struct.pack('>I', 73) + payload
        socket = RecordingSocket([packet])
        with patch.object(tablet, 'ws_send') as send:
            self.assertEqual(tablet.receive_video(socket, object(), 5), (False, 1, 73))
            send.assert_called_once()
        self.assertLessEqual(socket.largest, 64 * 1024)

    def test_t408_rejects_invalid_video_before_ack_or_large_read(self):
        packets = [struct.pack('>I', 0), struct.pack('>I', 1),
                   struct.pack('>I', 8 * 1024 * 1024 + 2),
                   struct.pack('>I', 5) + b'\x01' + b'\x00' * 4,
                   struct.pack('>I', 3) + b'\x07ab']
        for packet in packets:
            with self.subTest(packet=packet), patch.object(tablet, 'ws_send') as send:
                result = tablet.receive_video(FragmentedSocket([packet]), object(), 5)
                self.assertEqual(result, (False, 0, None))
                send.assert_not_called()

    def test_t408_every_video_fragment_boundary_and_sequence_wrap(self):
        packets = [b'\x00csd'] + [b'\x01' + struct.pack('>I', seq) + b'payload'
                                   for seq in [0xffffffff, 0, 1]]
        wire = b''.join(struct.pack('>I', len(packet)) + packet for packet in packets)
        for fragment in [1, 2, 3, 5, 7, 31]:
            chunks = [wire[i:i + fragment] for i in range(0, len(wire), fragment)]
            with self.subTest(fragment=fragment), patch.object(tablet, 'ws_send') as send:
                result = tablet.receive_video(FragmentedSocket(chunks), object(), 5)
                self.assertEqual(result, (True, 3, 1))
                self.assertEqual([call.args[1]['seq'] for call in send.call_args_list],
                                 [0xffffffff, 0, 1])

    def test_t408_partial_frame_is_never_acknowledged(self):
        packet = struct.pack('>I', 12) + b'\x01' + struct.pack('>I', 42) + b'payload'
        for cut in range(len(packet)):
            with self.subTest(cut=cut), patch.object(tablet, 'ws_send') as send:
                self.assertEqual(tablet.receive_video(FragmentedSocket([packet[:cut]]), object(), 5),
                                 (False, 0, None))
                send.assert_not_called()

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


class RuntimePaths(unittest.TestCase):
    """T242: shared with the Rust selector; no host runtime directory is written."""
    def test_t242_shared_runtime_base_contract(self):
        import json
        import os
        from unittest import mock
        fixture = Path(__file__).parents[2] / 'testdata/runtime-bases.json'
        cases = json.loads(fixture.read_text())
        paths = {'xdg': '/fixture/xdg with spaces', 'home': '/fixture/home',
                 'run': f'/run/user/{os.getuid()}', 'missing': '/fixture/missing'}
        for case in cases:
            with self.subTest(case=case['name']):
                env = {key: paths.get(case[name], case[name]) for key, name in
                       [('XDG_RUNTIME_DIR', 'xdg'), ('HOME', 'home')] if case[name] is not None}
                exists = {paths[name] for name in case['directories']}
                expected = case['expected'].replace('home/', paths['home'] + '/')
                expected = paths.get(expected, expected)
                with mock.patch.dict(os.environ, env, clear=True), \
                        mock.patch.object(os.path, 'isdir', side_effect=lambda path: path in exists):
                    self.assertEqual(tablet.runtime_dir(), os.path.join(os.path.realpath(expected), 'blent'))

    def test_t242_runtime_base_alias_resolves_like_the_host(self):
        import os
        import tempfile
        from unittest import mock
        with tempfile.TemporaryDirectory() as root:
            base = Path(root) / 'actual'
            base.mkdir()
            alias = Path(root) / 'alias'
            alias.symlink_to(base, target_is_directory=True)
            with mock.patch.dict(os.environ, {'XDG_RUNTIME_DIR': str(alias)}):
                self.assertEqual(tablet.runtime_dir(), str(base / 'blent'))

class AttachmentTokenTest(unittest.TestCase):
    def test_t444_slot_uses_its_own_credential(self):
        import tempfile
        with tempfile.TemporaryDirectory() as directory:
            for slot in range(4):
                name = 'token' if slot == 0 else f'token-{slot}'
                Path(directory, name).write_text(str(slot) * 64)
            for slot in range(4):
                with self.subTest(slot=slot), \
                        patch.object(tablet.sys, 'argv', ['fake-tablet', '--slot', str(slot)]), \
                        patch.object(tablet, 'runtime_dir', return_value=directory), \
                        patch.object(tablet, 'ws_connect', return_value=object()), \
                        patch.object(tablet, 'ws_send', side_effect=RuntimeError('stop after auth')) as send:
                    with self.assertRaisesRegex(RuntimeError, 'stop after auth'):
                        tablet.main()
                    self.assertEqual(send.call_args.args[1], {'type': 'auth', 'token': str(slot) * 64})

    def test_t444_slot_bounds_reject_before_io(self):
        import io
        for slot in ['-1', '4', '4294967295', '999999999999999999999', 'x']:
            with self.subTest(slot=slot), \
                    patch.object(tablet.sys, 'argv', ['fake-tablet', '--slot', slot]), \
                    patch.object(tablet.sys, 'stderr', io.StringIO()), \
                    patch.object(tablet, 'runtime_dir') as runtime:
                with self.assertRaises(SystemExit) as failure:
                    tablet.main()
                self.assertEqual(failure.exception.code, 2)
                runtime.assert_not_called()


if __name__ == '__main__':
    unittest.main()
