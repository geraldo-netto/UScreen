"""T462: cached quality results need retained, matching decode evidence."""
import importlib.util
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
        row = dict(returncode=0, reference_sha256='reference-a', decoded_frames=240,
                   psnr_db={'all': 40.0, 'luma': 40.0, 'text_crop': 40.0})
        row.update(changes)
        (folder / 'result.json').write_text(json.dumps(row))
        if diagnostics is not None:
            (folder / 'decode.log').write_text(diagnostics)
        args = SimpleNamespace(sweep=root / 'sweep', output=root / 'output')
        return args, {'frames': 240}, {'scene': 'static', 'sha256': 'reference-a'}, row

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
