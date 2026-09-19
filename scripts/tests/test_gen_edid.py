"""T321: deterministic malformed/boundary corpus for the maintained EDID tool."""
import contextlib
import importlib.util
import io
from pathlib import Path
import random
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('edid', Path(__file__).parents[1] / 'gen-edid.py')
edid = importlib.util.module_from_spec(spec)
spec.loader.exec_module(edid)


class TimingTest(unittest.TestCase):
    def test_t321_rejection_explains_low_clock_and_supported_refresh(self):
        for fps in [10, 24]:
            with self.assertRaisesRegex(ValueError, '10 MHz.*25 Hz'):
                edid.make_edid(640, 480, fps)
        for width, height, fps in [(640, 480, 25), (640, 1212, 10), (90, 3960, 10)]:
            self.assertIsNotNone(self.check_mode(width, height, fps))

    def check_mode(self, width, height, fps):
        try:
            result = edid.make_edid(width, height, fps)
        except ValueError:
            return
        self.assertEqual(len(result), 128)
        self.assertEqual(sum(result) % 256, 0)
        self.assertGreaterEqual(int.from_bytes(result[54:56], 'little'), 1000)
        self.assertLessEqual(result[113], fps)
        self.assertTrue(1 <= width <= 4095 and 1 <= height <= 4095)
        self.assertTrue(10 <= fps <= 90)
        return result

    def test_t321_invalid_and_out_of_bounds_numeric_corpus(self):
        rng = random.Random(321)
        cases = [(rng.randrange(-100, 5000), rng.randrange(-100, 5000), rng.randrange(-5, 100))
                 for _ in range(1024)]
        for value in [-2**63, -1, 0, 1, 4095, 4096, 2**32-1, 2**64]:
            cases.extend([(value, 480, 25), (640, value, 25), (640, 480, value)])
        for values in cases:
            with self.subTest(values=values):
                self.check_mode(*values)

    def test_t321_invalid_physical_dimensions_and_generator_cli(self):
        for value in [-1, 0, 4096, 2**64]:
            with self.assertRaises(ValueError):
                edid.make_edid(640, 480, 25, width_mm=value)
            with self.assertRaises(ValueError):
                edid.make_edid(640, 480, 25, height_mm=value)
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory, 'display with spaces.bin')
            args = ['gen-edid', '640', '480', '25', str(output)]
            with patch.object(edid.sys, 'argv', args), contextlib.redirect_stdout(io.StringIO()):
                edid.main()
            expected = edid.make_edid(640, 480, 25)
            self.assertEqual(output.read_bytes(), expected)
            args[3] = '10'
            with patch.object(edid.sys, 'argv', args), self.assertRaisesRegex(ValueError, '10 MHz'):
                edid.main()
            self.assertEqual(output.read_bytes(), expected, 'rejected mode overwrote a valid EDID')


if __name__ == '__main__':
    unittest.main()
