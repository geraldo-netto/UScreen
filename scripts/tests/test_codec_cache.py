"""T462: cached quality results need retained, matching decode evidence."""
import importlib.util
import hashlib
import json
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location('codec_match', ROOT / 'scripts/benchmarks/codec-match.py')
MATCH = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MATCH)


class CodecCacheTests(unittest.TestCase):
    def cached(self, directory, diagnostics='', **changes):
        root = Path(directory)
        folder = root / 'sweep/static-h264_vaapi-q18'
        folder.mkdir(parents=True)
        pixels = bytes(4 * 4 * 3 // 2 * 240)
        source = root / 'corpus'
        source.mkdir()
        (source / 'static.nv12').write_bytes(pixels)
        scene = dict(scene='static', path='static.nv12', sha256=hashlib.sha256(pixels).hexdigest(), bytes=len(pixels))
        meta = dict(width=4, height=4, frames=240, fps=60, pixel_format='nv12')
        encoded = b'cache fixture'
        (folder / 'encoded.mkv').write_bytes(encoded)
        row = dict(returncode=0, scene='static', encoder='h264_vaapi', quality=18,
                   reference_sha256=scene['sha256'], decoded_frames=240, reference_format=meta.copy(),
                   encoded_bytes=len(encoded), sha256=hashlib.sha256(encoded).hexdigest(),
                   stream=dict(width=4, height=4),
                   psnr_db={'all': 40.0, 'luma': 40.0, 'text_crop': 40.0})
        row.update(changes)
        (folder / 'result.json').write_text(json.dumps(row))
        if diagnostics is not None:
            (folder / 'decode.log').write_text(diagnostics)
        args = SimpleNamespace(sweep=root / 'sweep', output=root / 'output', corpus=source)
        return args, meta, scene, row

    def test_t462_bad_or_missing_decode_evidence_rejects_cached_metrics(self):
        for diagnostics in ('cu_qp_delta 99 is outside the valid range\n', None):
            with self.subTest(diagnostics=diagnostics), tempfile.TemporaryDirectory() as directory:
                args, meta, scene, _ = self.cached(directory, diagnostics)
                with self.assertRaisesRegex(ValueError, 'fresh measurement'):
                    MATCH.measure(args, meta, scene, 'h264_vaapi', 18)

    def test_t462_stale_reference_or_frame_count_rejects_cached_metrics(self):
        for changes in ({'reference_sha256': 'old-reference'}, {'decoded_frames': 239}):
            with self.subTest(changes=changes), tempfile.TemporaryDirectory() as directory:
                args, meta, scene, _ = self.cached(directory, **changes)
                with self.assertRaisesRegex(ValueError, 'fresh measurement'):
                    MATCH.measure(args, meta, scene, 'h264_vaapi', 18)

    def test_t462_clean_matching_cache_is_reused_without_encoding(self):
        with tempfile.TemporaryDirectory() as directory:
            args, meta, scene, expected = self.cached(directory)
            with patch.object(MATCH.HOST, 'trial') as encode:
                row, path = MATCH.measure(args, meta, scene, 'h264_vaapi', 18)
            encode.assert_not_called()
            self.assertEqual(row, expected)
            self.assertEqual(Path(path).name, 'result.json')

    def test_t462_output_cache_uses_the_same_validation(self):
        with tempfile.TemporaryDirectory() as directory:
            args, meta, scene, _ = self.cached(directory, 'concealed frame\n')
            args.sweep.rename(args.output)
            with self.assertRaisesRegex(ValueError, 'fresh measurement'):
                MATCH.measure(args, meta, scene, 'h264_vaapi', 18)


if __name__ == '__main__':
    unittest.main()
