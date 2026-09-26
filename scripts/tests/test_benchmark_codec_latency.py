"""T430: encoder stalls cannot bypass the paced replay deadline."""
import importlib.util
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / 'benchmarks/codec-latency.py'


def replay_probe(folder, program):
    spec = importlib.util.spec_from_file_location('latency', SCRIPT)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    module.REPLAY_GRACE_SECONDS = 0.15
    source = folder / 'input.nv12'
    source.write_bytes(bytes(128 * 128 * 3 // 2))
    try:
        result = module.replay([sys.executable, '-c', program], source,
                              dict(width=128, height=128, frames=1, fps=60), folder)
        print(json.dumps(dict(ok=True, packets=result['packets'])))
    except Exception as error:
        print(json.dumps(dict(ok=False, error=type(error).__name__)))


class CodecLatencyTests(unittest.TestCase):
    def probe(self, program):
        with tempfile.TemporaryDirectory(prefix='blent-t430-') as directory:
            child = subprocess.Popen([sys.executable, '-B', __file__, directory, program],
                                     stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                     text=True, start_new_session=True)
            try:
                stdout, stderr = child.communicate(timeout=3)
            except subprocess.TimeoutExpired:
                os.killpg(child.pid, signal.SIGKILL)
                child.communicate()
                self.fail('T430: replay bypassed its deadline; outer watchdog had to kill it')
            self.assertEqual(child.returncode, 0, stderr)
            return json.loads(stdout)

    def test_t430_stalled_encoder_input_is_bounded(self):
        result = self.probe('import time; time.sleep(60)')
        self.assertEqual(result, dict(ok=False, error='TimeoutError'))

    def test_t430_stalled_encoder_exit_is_bounded(self):
        result = self.probe('import sys,time; sys.stdin.buffer.read(); time.sleep(60)')
        self.assertEqual(result, dict(ok=False, error='TimeoutError'))

    def test_t430_malformed_output_propagates_to_caller(self):
        result = self.probe('import sys; sys.stdin.buffer.read(); print("broken framecrc")')
        self.assertFalse(result['ok'], 'T430: reader error must not return a successful replay')

    def test_t430_partial_reads_and_packet_lines_preserve_frame(self):
        program = '''import os
count = 0
while True:
    block = os.read(0, 257)
    if not block: break
    count += len(block)
assert count == 128 * 128 * 3 // 2
for part in [b'#header\\n0, 0, ', b'0, 1, 23, checksum\\n']:
    os.write(1, part)
'''
        result = self.probe(program)
        self.assertTrue(result['ok'])
        self.assertEqual([(row['pts'], row['bytes']) for row in result['packets']], [(0, 23)])


if __name__ == '__main__':
    if len(sys.argv) == 3:
        replay_probe(Path(sys.argv[1]), sys.argv[2])
    else:
        unittest.main()
