"""T518: corpus generation owns and closes the source it hashes."""
import importlib.util
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

from PIL import Image

SOURCE = Path(__file__).resolve().parents[1]/'benchmarks/codec-corpus.py'
SPEC = importlib.util.spec_from_file_location('codec_corpus', SOURCE)
CORPUS = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CORPUS)


class CorpusOwnershipTests(unittest.TestCase):
    def test_t518_corpus_closes_hash_reader_before_returning(self):
        with tempfile.TemporaryDirectory() as directory:
            args = SimpleNamespace(output=Path(directory), frames=1)
            opened = []
            real_open = Path.open
            def capture(path, *args, **kwargs):
                stream = real_open(path, *args, **kwargs)
                if path.suffix == '.nv12' and args == ('rb',):
                    opened.append(stream)
                return stream
            try:
                with patch.object(Path, 'open', capture), patch.object(CORPUS, 'WIDTH', 64), \
                        patch.object(CORPUS, 'HEIGHT', 64):
                    row = CORPUS.corpus('text', args, Image.new('RGB', (64, 64), 'white'))
                self.assertEqual(row['bytes'], 64*64*3//2)
                self.assertEqual(len(opened), 1)
                self.assertTrue(opened[0].closed, 'corpus returned with its hash reader still open')
            finally:
                for stream in opened:
                    stream.close()


if __name__ == '__main__':
    unittest.main()
