"""T633: interrupted status monitoring must not retain guest file locks."""
import importlib.util
import base64
from pathlib import Path
import unittest
from unittest.mock import Mock, patch

SOURCE = Path(__file__).resolve().parents[1] / 'dev/windows-vm/guest_status.py'
SPEC = importlib.util.spec_from_file_location('windows_vm_guest_status', SOURCE)
STATUS = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(STATUS)


class InterruptedAgent:
    """QGA file handles survive a host disconnect, unlike process-owned reads."""

    def __init__(self):
        self.open_handles = set()

    def request(self, command, arguments):
        if command == 'guest-file-open':
            self.open_handles.add(42)
        raise TimeoutError('host lost the reply after guest accepted request')

    def write_status(self):
        if self.open_handles:
            raise PermissionError('status file remains open in QGA')
        return 'written'


class StatusTests(unittest.TestCase):
    def test_t633_lost_open_reply_cannot_block_guest_status_writer(self):
        agent = InterruptedAgent()
        with self.assertRaises(TimeoutError):
            STATUS.read_file(agent.request, r'C:\BlentSetup\phase.json')
        self.assertEqual(agent.write_status(), 'written')

    def test_t633_text_roundtrip_and_no_agent_file_handles(self):
        for text in ('', '\ufeff{"phase":"ready"}', 'Unicode: \u00e9\U0001f600', 'x' * STATUS.MAX_BYTES):
            with self.subTest(length=len(text)):
                request = Mock(side_effect=[{'pid': 7}, {'exited': False}, {
                    'exited': True, 'exitcode': 0,
                    'out-data': base64.b64encode(text.encode()).decode()}])
                with patch.object(STATUS.time, 'sleep'):
                    self.assertEqual(STATUS.read_file(request, "C:\\a'b\\status.json"), text.lstrip('\ufeff'))
                commands = [call.args[0] for call in request.call_args_list]
                self.assertEqual(commands, ['guest-exec', 'guest-exec-status', 'guest-exec-status'])

    def test_t633_invalid_inputs_do_not_reach_guest(self):
        for path in ('', None, 17, 'bad\0path'):
            request = Mock()
            with self.subTest(path=path), self.assertRaises(ValueError):
                STATUS.read_file(request, path)
            request.assert_not_called()
        for timeout in (-1, 0, 61, float('nan'), float('inf')):
            request = Mock()
            with self.subTest(timeout=timeout), self.assertRaises(ValueError):
                STATUS.read_file(request, 'status.json', timeout)
            request.assert_not_called()

    def test_t633_failed_truncated_oversized_and_invalid_outputs_are_rejected(self):
        invalid = [{'exitcode': 1}, {'exitcode': 0, 'out-truncated': True},
                   {'exitcode': 0, 'err-truncated': True},
                   {'exitcode': 0, 'out-data': 'not base64'},
                   {'exitcode': 0, 'out-data': base64.b64encode(b'\xff').decode()},
                   {'exitcode': 0, 'out-data': base64.b64encode(b'x' * (STATUS.MAX_BYTES + 1)).decode()}]
        for result in invalid:
            with self.subTest(keys=list(result)), self.assertRaises((RuntimeError, ValueError)):
                STATUS.decode_result(result)

    def test_t633_timeout_never_duplicates_guest_process(self):
        request = Mock(return_value={'pid': 7})
        with patch.object(STATUS.time, 'monotonic', side_effect=[0, 1]):
            with self.assertRaises(TimeoutError):
                STATUS.read_file(request, 'status.json', timeout=.1)
        self.assertEqual(request.call_count, 1)


if __name__ == '__main__':
    unittest.main()
