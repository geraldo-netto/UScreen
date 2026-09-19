"""T497: stock software codec experiments retain bytes, clocks and quality boundaries."""
import contextlib
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import struct
import subprocess
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

from PIL import Image, ImageFont

BENCH = Path(__file__).resolve().parents[1] / 'benchmarks'


def load(name):
    spec = importlib.util.spec_from_file_location(name.replace('-', '_'), BENCH/(name + '.py'))
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


HOST, FIXTURES, MATCH, CORPUS, ADMISSION, LATENCY = map(load, [
    'codec-host', 'codec-fixtures', 'codec-match', 'codec-corpus', 'frame-admission', 'codec-latency'])


def corpus(root, width=1280, height=800, frames=2):
    folder = root/'corpus'
    folder.mkdir()
    data = b''.join(bytes([value])* (width*height*3//2) for value in range(100, 100 + frames))
    (folder/'text.nv12').write_bytes(data)
    scene = dict(scene='text', path='text.nv12', bytes=len(data), sha256=hashlib.sha256(data).hexdigest())
    meta = dict(width=width, height=height, frames=frames, fps=60, pixel_format='nv12', scenes=[scene])
    (folder/'metadata.json').write_text(json.dumps(meta))
    return folder, meta, scene


def command(module, arguments):
    with patch.object(sys, 'argv', [module.__name__, *map(str, arguments)]), contextlib.redirect_stdout(io.StringIO()):
        module.main()


class CodecResearchTests(unittest.TestCase):
    def test_t497_stock_software_encode_decode_export_and_paced_replay_share_provenance(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source, meta, scene = corpus(root)
            sweep = root/'sweep'
            command(HOST, ['--corpus', source, '--output', sweep, '--encoder', 'libx264', '--quality', '18'])
            result = sweep/'text-libx264-q18/result.json'
            row = json.loads(result.read_text())
            self.assertEqual(row['decoded_frames'], 2)
            self.assertEqual(row['returncode'], 0)
            self.assertGreater(row['fps_including_startup'], 0)
            selected = dict(scene='text', encoder='libx264', selected_quantizer=18,
                            selection=dict(path=str(result), result=row))
            selection = root/'selection.json'
            selection.write_text(json.dumps(dict(selections=[selected])))
            command(FIXTURES, ['--corpus', source, '--selection', selection, '--output', root/'export'])
            exported = json.loads((root/'export/metadata.json').read_text())['fixtures'][0]
            self.assertEqual((exported['codec'], exported['frames']), ('h264', 2))
            self.assertGreater(exported['config_bytes'], 0)
            command(LATENCY, ['--corpus', source, '--selection', selection, '--output', root/'latency',
                             '--encoder', 'libx264', '--trials', '2'])
            timing = json.loads((root/'latency/text-libx264-1/result.json').read_text())
            self.assertEqual([row['pts'] for row in timing['packets']], [0, 1])
            self.assertTrue(all(value >= 0 for value in timing['latency_ms']))
            self.assertEqual(timing['reference_sha256'], scene['sha256'])

    def test_t497_quality_reports_exact_error_and_held_picture_loss(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source, meta, scene = corpus(root)
            raw = source/scene['path']
            target = root/'decoded'
            target.write_bytes(raw.read_bytes())
            exact = HOST.quality(raw, target, meta)
            self.assertEqual(exact['mse'], dict(all=0, luma=0, text_crop=0))
            self.assertTrue(all(value is None for value in exact['psnr_db'].values()))
            target.write_bytes(bytes(value+1 for value in raw.read_bytes()))
            self.assertEqual(HOST.quality(raw, target, meta)['mse'], dict(all=1, luma=1, text_crop=1))
            target.write_bytes(b'short')
            with self.assertRaises(ValueError): HOST.quality(raw, target, meta)
            selected = root/'selected'
            frame_bytes = meta['width']*meta['height']*3//2
            ADMISSION.select_raw(raw, selected, frame_bytes, 2, 2)
            self.assertEqual(selected.stat().st_size, frame_bytes)
            self.assertEqual(ADMISSION.held_quality(raw, selected, frame_bytes, 2, 2)['mse'], 0.5)
            self.assertIsNone(ADMISSION.held_quality(raw, raw, frame_bytes, 2, 1)['psnr_db'])

    def test_t497_encoder_commands_keep_software_and_hardware_options_separate(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source, meta, scene = corpus(root, 4, 4)
            args = SimpleNamespace(corpus=source, vaapi_device='/fixture/nonexistent-render')
            for encoder in HOST.ENCODERS:
                with self.subTest(encoder=encoder):
                    argv = HOST.command_for(args, meta, scene, encoder, 18, root/'encoded')
                    self.assertEqual(argv[argv.index('-c:v')+1], encoder)
                    self.assertEqual('-vaapi_device' in argv, encoder.endswith('_vaapi'))
                    self.assertEqual(argv[-2:], ['-y', str(root/'encoded')])
            with self.assertRaises(ValueError): HOST.codec_options('unknown', 18)
            with self.assertRaises(SystemExit):
                command(HOST, ['--corpus', source, '--output', root/'invalid', '--encoder', 'libx264'])
            self.assertFalse((root/'invalid').exists())

    def test_t497_failed_encoder_retains_diagnostics_without_quality_claim(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source, meta, scene = corpus(root, 4, 4)
            args = SimpleNamespace(corpus=source, output=root, vaapi_device='/fixture/nonexistent-render')
            process = subprocess.CompletedProcess([], 17, stdout='', stderr='fixture encoder failure')
            with patch.object(HOST.subprocess, 'run', return_value=process), \
                    patch.object(HOST, 'inspect') as inspect, contextlib.redirect_stdout(io.StringIO()):
                HOST.trial(args, meta, scene, 'libx264', 18)
            inspect.assert_not_called()
            folder = root/'text-libx264-q18'
            self.assertEqual((folder/'encode.log').read_text(), 'fixture encoder failure')
            row = json.loads((folder/'result.json').read_text())
            self.assertEqual(row['returncode'], 17)
            self.assertNotIn('psnr_db', row)

    def test_t497_quality_search_obeys_all_floors_and_preserves_no_match(self):
        reference = dict(psnr_db=dict(all=40, luma=42, text_crop=44))
        self.assertTrue(MATCH.acceptable(dict(psnr_db=dict.fromkeys(reference['psnr_db'])), reference, 0.5))
        self.assertFalse(MATCH.acceptable(reference, dict(psnr_db=dict(all=None)), 0.5))
        args = SimpleNamespace(tolerance=0.5)
        def measured(_args, _meta, _scene, _encoder, quantizer):
            return dict(psnr_db={key: value+18-quantizer for key, value in reference['psnr_db'].items()}), str(quantizer)
        with patch.object(MATCH, 'measure', side_effect=measured):
            for encoder in HOST.ENCODERS:
                self.assertEqual(MATCH.select(args, {}, dict(scene='text'), encoder, reference)['selected_quantizer'], 18)
        with patch.object(MATCH, 'measure', return_value=(reference, 'fixture')):
            impossible = MATCH.select(args, {}, dict(scene='text'), 'libx264', dict(psnr_db=dict(all=None)))
            self.assertIsNone(impossible['selection'])

    def test_t497_fixture_formats_preserve_parameter_sets_and_reject_bounds(self):
        h264 = b'\0\0\1\x67sps\0\0\0\1\x68pps\0\0\1\x65slice'
        hevc = b'\0\0\1\x40vps\0\0\1\x42sps\0\0\1\x44pps'
        self.assertEqual(FIXTURES.config_for(h264, 'h264'), h264[:h264.index(b'\0\0\1\x65')])
        self.assertEqual(FIXTURES.config_for(hevc, 'hevc'), hevc)
        self.assertEqual(FIXTURES.config_for(b'keyframe', 'vp9'), b'')
        with self.assertRaises(ValueError): FIXTURES.config_for(b'\0\0\1\x65slice', 'h264')
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            header = b'DKIF\0\0' + struct.pack('<H', 32) + bytes(24)
            path = root/'stream.ivf'
            path.write_bytes(header + struct.pack('<IQ', 3, 0) + b'abc' + struct.pack('<IQ', 1, 1) + b'd')
            self.assertEqual(FIXTURES.ivf_frames(path), [b'abc', b'd'])
            for data in [b'invalid', header+struct.pack('<IQ', 0, 0), header+struct.pack('<IQ', 4, 0)+b'abc']:
                path.write_bytes(data)
                with self.assertRaises(ValueError): FIXTURES.ivf_frames(path)
            meta = dict(width=4, height=4, fps=60, frames=1)
            with self.assertRaises(ValueError): FIXTURES.write_fixture(root/'count', 'video/avc', meta, b'', [])
            with self.assertRaises(ValueError):
                FIXTURES.write_fixture(root/'size', 'video/avc', meta, b'', [bytes(8*1024*1024+1)])

    def test_t497_corpus_scene_motion_changes_pixels_and_conversion_retains_hashes(self):
        base = CORPUS.desktop(ImageFont.load_default())
        for scene in ['pen', 'motion']:
            self.assertNotEqual(CORPUS.paint(scene, base, 0).tobytes(), CORPUS.paint(scene, base, 10).tobytes())
        self.assertEqual(CORPUS.paint('text', base, 0).tobytes(), base.tobytes())
        with tempfile.TemporaryDirectory() as directory:
            args = SimpleNamespace(output=Path(directory), frames=2)
            with patch.object(CORPUS, 'WIDTH', 64), patch.object(CORPUS, 'HEIGHT', 64):
                row = CORPUS.corpus('text', args, Image.new('RGB', (64, 64), 'white'))
            data = (args.output/row['path']).read_bytes()
            self.assertEqual(row['bytes'], 64*64*3)
            self.assertEqual(row['sha256'], hashlib.sha256(data).hexdigest())


if __name__ == '__main__':
    unittest.main()
