"""T576: exact-revision conversion builds run outside the source tree."""
import hashlib
import importlib.util
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'scripts/benchmarks'))
import conversion
import damage_regions


class ConversionSourcesTests(unittest.TestCase):
    def test_t576_current_and_pre_header_revisions_compile_in_clean_directories(self):
        for revision in [None, 'HEAD', 'd1de64a']:
            with self.subTest(revision=revision), tempfile.TemporaryDirectory() as directory:
                folder = Path(directory) / 'build'
                binary, sources = conversion.build(folder, revision)
                self.assertTrue(binary.is_file())
                for relative, digest in sources.items():
                    data = (subprocess.check_output(['git', 'show', f'{revision}:{relative}'], cwd=ROOT)
                            if revision else (ROOT / relative).read_bytes())
                    self.assertEqual((folder / Path(relative).name).read_bytes(), data)
                    self.assertEqual(digest, hashlib.sha256(data).hexdigest())
                self.assertEqual((folder / 'pixel_span.h').exists(), revision != 'd1de64a')

    def test_t576_damage_replays_compile_with_current_and_pre_header_baselines(self):
        for revision in ['HEAD', 'd1de64a']:
            with self.subTest(revision=revision), tempfile.TemporaryDirectory() as directory:
                folder = Path(directory)
                damage_regions.build(folder, revision)
                rows = damage_regions.run_pair(folder, 0, 1, 0)
                self.assertEqual(rows[0]['checksum'], rows[1]['checksum'])


if __name__ == '__main__':
    unittest.main()
