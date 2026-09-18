"""T469: benchmark provenance follows the bytes, not copied hash strings."""
import hashlib
import importlib.util
import json
from pathlib import Path
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1] / 'benchmarks'


def load(name):
    spec = importlib.util.spec_from_file_location(name.replace('-', '_'), ROOT / (name + '.py'))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


HOST, MATCH, LATENCY, FIXTURES = map(load, ['codec-host', 'codec-match', 'codec-latency', 'codec-fixtures'])


def fixture(root):
    corpus = root / 'corpus'
    corpus.mkdir()
    pixels = bytes(range(24))
    (corpus / 'static.nv12').write_bytes(pixels)
    scene = dict(scene='static', path='static.nv12', bytes=len(pixels), sha256=hashlib.sha256(pixels).hexdigest())
    meta = dict(width=4, height=4, frames=1, fps=60, pixel_format='nv12', scenes=[scene])
    (corpus / 'metadata.json').write_text(json.dumps(meta))
    folder = root / 'sweep/static-libx264-q18'
    folder.mkdir(parents=True)
    encoded = b'fixture encoded payload'
    (folder / 'encoded.mkv').write_bytes(encoded)
    (folder / 'decode.log').write_text('')
    row = dict(returncode=0, scene='static', encoder='libx264', quality=18,
               reference_sha256=scene['sha256'], decoded_frames=1,
               reference_format={key: meta[key] for key in ['width', 'height', 'frames', 'fps', 'pixel_format']},
               encoded_bytes=len(encoded), sha256=hashlib.sha256(encoded).hexdigest(),
               stream=dict(codec_name='h264', width=4, height=4), psnr_db={'all': 40})
    result = folder / 'result.json'
    result.write_text(json.dumps(row))
    selected = dict(scene='static', encoder='libx264', selected_quantizer=18,
                    selection=dict(path=str(result), result=row))
    args = SimpleNamespace(corpus=corpus, sweep=root / 'sweep', output=root / 'output',
                           vaapi_device='/not-used', async_depth=None)
    return args, meta, scene, row, selected


class ArtifactTests(unittest.TestCase):
    def test_t469_same_length_raw_change_is_rejected_before_encoding_or_replay(self):
        for module in [HOST, LATENCY]:
            with self.subTest(module=module.__name__), tempfile.TemporaryDirectory() as directory:
                args, meta, scene, _, selected = fixture(Path(directory))
                (args.corpus / scene['path']).write_bytes(b'x' * scene['bytes'])
                args.output.mkdir()
                with patch.object(HOST, 'encode', return_value=dict(returncode=1, wall_seconds=1)) as encode, \
                     patch.object(LATENCY, 'replay', return_value=dict(frames=[dict(admitted_ns=0)], packets=[dict(pts=0, observed_ns=1)])) as replay:
                    with self.assertRaises(ValueError):
                        if module is HOST:
                            HOST.trial(args, meta, scene, 'libx264', 18)
                        else:
                            LATENCY.trial(args, meta, scene, selected, 0)
                    encode.assert_not_called(); replay.assert_not_called()

    def test_t469_cache_rejects_substituted_encoded_bytes_and_stale_dimensions(self):
        for mutation in ['encoded', 'raw', 'dimensions', 'size', 'identity', 'fps']:
            with self.subTest(mutation=mutation), tempfile.TemporaryDirectory() as directory:
                args, meta, scene, row, selected = fixture(Path(directory))
                result = Path(selected['selection']['path'])
                if mutation == 'encoded':
                    result.with_name('encoded.mkv').write_bytes(b'x' * row['encoded_bytes'])
                elif mutation == 'raw':
                    (args.corpus / scene['path']).write_bytes(b'x' * scene['bytes'])
                elif mutation == 'dimensions':
                    meta['width'], meta['height'] = 2, 8
                elif mutation == 'size':
                    scene['bytes'] += 1
                elif mutation == 'fps':
                    meta['fps'] = 30
                else:
                    row['encoder'] = 'libx265'
                    result.write_text(json.dumps(row))
                with self.assertRaises(ValueError):
                    MATCH.measure(args, meta, scene, 'libx264', 18)

    def test_t469_matching_artifacts_keep_quality_and_skip_encoding(self):
        with tempfile.TemporaryDirectory() as directory:
            args, meta, scene, expected, _ = fixture(Path(directory))
            with patch.object(MATCH.HOST, 'trial') as encode:
                row, _ = MATCH.measure(args, meta, scene, 'libx264', 18)
            self.assertEqual(row, expected)
            encode.assert_not_called()

    def test_t469_fixture_export_rejects_replaced_encoded_source(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            args, _, _, row, selected = fixture(root)
            Path(selected['selection']['path']).with_name('encoded.mkv').write_bytes(b'x' * row['encoded_bytes'])
            selection = root / 'selection.json'
            selection.write_text(json.dumps(dict(selections=[selected])))
            argv = ['codec-fixtures', '--corpus', str(args.corpus), '--output', str(args.output), '--selection', str(selection)]
            with patch.object(sys, 'argv', argv), patch.object(FIXTURES, 'elementary') as export, \
                 patch.object(FIXTURES, 'annex_frames', return_value=[b'frame']), \
                 patch.object(FIXTURES, 'config_for', return_value=b'config'):
                with self.assertRaises(ValueError):
                    FIXTURES.main()
                export.assert_not_called()

    def test_t469_historical_command_provenance_is_verified_without_relabeling(self):
        with tempfile.TemporaryDirectory() as directory:
            args, meta, scene, row, selected = fixture(Path(directory))
            del row['reference_format']
            row['command'] = ['ffmpeg', '-video_size', '4x4', '-framerate', '60', '-frames:v', '1', '-pixel_format', 'nv12']
            path = Path(selected['selection']['path'])
            path.write_text(json.dumps(row))
            before = path.read_bytes()
            self.assertEqual(MATCH.measure(args, meta, scene, 'libx264', 18)[0], row)
            self.assertEqual(path.read_bytes(), before)

    def test_t469_changed_selection_metadata_is_rejected_before_replay(self):
        with tempfile.TemporaryDirectory() as directory:
            args, meta, scene, _, selected = fixture(Path(directory))
            selected['selection']['result']['quality'] = 19
            args.output.mkdir()
            with patch.object(LATENCY, 'replay') as replay:
                with self.assertRaises(ValueError):
                    LATENCY.trial(args, meta, scene, selected, 0)
                replay.assert_not_called()


if __name__ == '__main__':
    unittest.main()
