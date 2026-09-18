"""T461: decoder error concealment must not become a quality measurement."""
import importlib.util
from pathlib import Path
import re
import subprocess
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location('codec_host', ROOT / 'scripts/benchmarks/codec-host.py')
HOST = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(HOST)
FIXTURE = ROOT / 'testdata/t461-corrupt-hevc.hevc'


class CodecValidationTests(unittest.TestCase):
    def test_t461_concealed_frames_are_rejected_before_quality(self):
        with tempfile.TemporaryDirectory() as directory:
            folder = Path(directory)
            with patch.object(HOST, 'quality', return_value={}) as quality:
                with self.assertRaises((ValueError, subprocess.CalledProcessError)):
                    HOST.inspect(FIXTURE, folder, folder / 'reference', {})
                quality.assert_not_called()
            self.assertTrue((folder / 'decode.log').read_text().strip(),
                            'T461: preserve native decode errors even on failure')
            self.assertFalse((folder / 'decoded.nv12').exists())

    def test_t461_clean_access_unit_still_reaches_quality_measurement(self):
        data = FIXTURE.read_bytes()
        # Each fixture access unit starts with an AUD. Keep the original first
        # unit's parameter sets and picture, excluding the corrupt later unit.
        starts = [m.start() for m in re.finditer(b'\x00\x00(?:\x00)?\x01\x46\x01', data)]
        self.assertGreaterEqual(len(starts), 3)
        with tempfile.TemporaryDirectory() as directory:
            folder = Path(directory)
            encoded = folder / 'clean.hevc'
            encoded.write_bytes(data[:starts[1]])

            def quality(reference, decoded, meta):
                self.assertEqual(decoded.stat().st_size, 1280 * 800 * 3 // 2)
                return {'decoded_frames': 1}

            with patch.object(HOST, 'quality', side_effect=quality) as measured:
                result = HOST.inspect(encoded, folder, folder / 'reference', {})
            measured.assert_called_once()
            self.assertEqual(result['decoded_frames'], 1)
            self.assertEqual(result['stream']['codec_name'], 'hevc')
            self.assertEqual((folder / 'decode.log').read_text(), '')
            self.assertFalse((folder / 'decoded.nv12').exists())

    def test_t461_failed_process_also_preserves_diagnostics(self):
        with tempfile.TemporaryDirectory() as directory:
            folder = Path(directory)
            encoded = folder / 'invalid.hevc'
            encoded.write_bytes(b'not an encoded stream')
            with self.assertRaises(subprocess.CalledProcessError):
                HOST.inspect(encoded, folder, folder / 'reference', {})
            self.assertTrue((folder / 'decode.log').read_text().strip())
            self.assertFalse((folder / 'decoded.nv12').exists())


if __name__ == '__main__':
    unittest.main()
