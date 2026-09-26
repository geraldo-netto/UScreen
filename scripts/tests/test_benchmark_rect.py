"""T419: research evidence keeps clocks, incomplete samples and memory bounds explicit."""
import importlib.util
import hashlib
import io
from pathlib import Path
import sys
import tarfile
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

BENCH = Path(__file__).resolve().parents[1] / 'benchmarks'
sys.path.insert(0, str(BENCH))


def load(name):
    spec = importlib.util.spec_from_file_location(name, BENCH / (name + '.py'))
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


SUMMARY, DEVICE, PROJECT = load('summarize-rect'), load('rect-device'), load('rect-project')
SURFACE = load('rect_surface')


class RectEvidenceTests(unittest.TestCase):
    def test_t419_android16_layer_names_keep_the_unique_id(self):
        raw = 'RequestedLayerState{abc SurfaceView[com.blent.rectbench/com.blent.benchmark.MainActivity](BLAST)#42 parentId=41}'
        self.assertEqual(SURFACE.layer_name(raw, 'com.blent.rectbench'),
                         'abc SurfaceView[com.blent.rectbench/com.blent.benchmark.MainActivity](BLAST)#42')
        with self.assertRaises(ValueError):
            SURFACE.layer_name(raw + '\n' + raw, 'com.blent.rectbench')

    def test_t419_surface_present_joins_sequence_ids_and_rejects_ambiguity(self):
        trace = [[1, 10_000_000], [2, 20_000_000]]
        frames = {(1000, 15_000_000, 1000), (2000, 25_000_000, 2000)}
        self.assertEqual(SUMMARY.video_presentations(trace, frames),
                         ([(10_000_000, 15_000_000), (20_000_000, 25_000_000)], 2))
        frames.add((1000, 16_000_000, 1000))
        with self.assertRaises(ValueError):
            SUMMARY.video_presentations(trace, frames)

    def test_t419_pinned_archive_alone_does_not_validate_mutated_extracted_sources(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / 'lz4/lib/lz4.c'
            source.parent.mkdir(parents=True)
            source.write_bytes(b'original')
            archive = root / 'lz4-source.tar.gz'
            with tarfile.open(archive, 'w:gz') as bundle:
                member = tarfile.TarInfo('lz4-1.9.4/lib/lz4.c')
                member.size = 8
                bundle.addfile(member, io.BytesIO(b'original'))
            digest = hashlib.sha256(archive.read_bytes()).hexdigest()
            PROJECT.verified_source_tree(root, 'lz4', digest)
            extra = source.with_name('untracked.c')
            extra.write_bytes(b'not in the pinned archive')
            with self.assertRaises(ValueError):
                PROJECT.verified_source_tree(root, 'lz4', digest)
            extra.unlink()
            source.write_bytes(b'modified')
            with self.assertRaises(ValueError):
                PROJECT.verified_source_tree(root, 'lz4', digest)

    def test_t419_noop_pending_and_missing_presentation_are_not_latency_samples(self):
        result = dict(count=4, compressed_storage_bytes=100, input_mode='read', trace=[
            [0, 1_000_000, 2_000_000, 3_000_000, 4_000_000, 20, 1, 11_000_000],
            [0, 1_000_000, 2_000_000, 3_000_000, 4_000_000, 20, 2, -2],
            [0, 1_000_000, 2_000_000, 3_000_000, 4_000_000, 20, 3, -1],
            [0, 1_000_000, 2_000_000, 3_000_000, 4_000_000, 0, -4, -4]])
        row = SUMMARY.rectangles(result)
        self.assertEqual((row['inputs'], row['updates'], row['presented'], row['absent_presentation']), (4, 3, 1, 2))
        self.assertEqual(row['admission_to_egl_present_ms']['p50'], 10)
        self.assertEqual(row['admission_to_swap_ms']['p50'], 3)
        self.assertNotIn('admission_to_callback_ms', row)

    def test_t419_changed_process_and_backwards_cpu_cannot_be_averaged(self):
        first = dict(pid=1, start_ticks=10, at_ns=1_000_000_000, ticks=100)
        last = dict(first, at_ns=3_000_000_000, ticks=200)
        self.assertEqual(SUMMARY.sampled_cpu(first, last, 100)['cpu_percent_one_core'], 50)
        for changed in [dict(last, start_ticks=11), dict(last, pid=2), dict(last, ticks=99)]:
            with self.subTest(changed=changed), self.assertRaises(ValueError):
                SUMMARY.sampled_cpu(first, changed, 100)

    def test_t419_cpu_is_percent_of_one_core_and_absent_percentiles_stay_absent(self):
        row = SUMMARY.cpu(dict(elapsed_ns=0, process_cpu_ms=100), dict(elapsed_ns=2_000_000_000, process_cpu_ms=1100))
        self.assertEqual(row['cpu_percent_one_core'], 50)
        self.assertIsNone(SUMMARY.distribution([]))

    def test_t419_foreground_check_never_starts_over_another_app(self):
        for activity in ['other.app/.MainActivity', 'com.blent.rectbench/com.blent.benchmark.MainActivity']:
            with self.subTest(activity=activity), patch.object(DEVICE, 'capture', return_value='topResumedActivity=' + activity):
                with self.assertRaises(RuntimeError):
                    DEVICE.foreground(SimpleNamespace(serial='fixture'))

    def test_t419_video_duplicates_are_rejected_and_missing_callbacks_are_reported(self):
        result = dict(sent=3, stats=dict(rendered=2, duplicates=0, invalidations=0, arrival_to_callback_us=[1000, 2000]))
        self.assertEqual(SUMMARY.video(result)['missing_callbacks'], 1)
        result['stats']['duplicates'] = 1
        with self.assertRaises(ValueError):
            SUMMARY.video(result)


if __name__ == '__main__':
    unittest.main()
