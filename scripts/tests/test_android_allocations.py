"""T589: missing/reset ART counters cannot become claimed zero allocation/GC."""
import importlib.util
from pathlib import Path
import unittest

SPEC = importlib.util.spec_from_file_location('allocations', Path(__file__).resolve().parents[1] / 'benchmarks/android-allocations.py')
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class AllocationCountersTests(unittest.TestCase):
    def test_t589_counter_differences_preserve_unknown_and_reset_states(self):
        self.assertEqual(MODULE.difference({'bytes': '100'}, {'bytes': '123'}, 'bytes'), 23)
        self.assertEqual(MODULE.difference({'bytes': '100'}, {'bytes': '100'}, 'bytes'), 0)
        for before, after in [({}, {}), ({'bytes': '5'}, {'bytes': '4'}),
                              ({'bytes': ''}, {'bytes': '4'}), ({'bytes': '-1'}, {'bytes': '4'})]:
            self.assertIsNone(MODULE.difference(before, after, 'bytes'))


if __name__ == '__main__':
    unittest.main()
