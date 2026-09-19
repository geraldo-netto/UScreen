"""T497: scope and collection rules must fail closed on missing evidence."""
from dataclasses import asdict
import unittest

from model import Function
from report import collect, selected, summary


class DriverTest(unittest.TestCase):
    def test_t497_inline_bodies_count_with_owner_but_uninvoked_callbacks_still_fail(self):
        functions = [Function('android/A.kt', 'owner', 1, 8, 'kotlin'),
                     Function('android/A.kt', '<lambda>', 2, 3, 'kotlin'),
                     Function('android/A.kt', '<lambda>', 5, 6, 'kotlin'),
                     Function('android/Missing.kt', '<lambda>', 1, 2, 'kotlin')]
        manifest = {'functions': [asdict(function) for function in functions]}
        methods = [dict(file='android/A.kt', name='owner', line=1, covered=5, total=5),
                   dict(file='android/A.kt', name='invoke', line=5, covered=0, total=2)]
        rows = collect(manifest, {}, (methods, {'android/A.kt': {1: 1, 2: 1, 3: 1, 5: 0, 6: 0}}), [])
        self.assertEqual([(row['file'], row['line'], row['passes']) for row in rows],
                         [('android/A.kt', 1, True), ('android/A.kt', 5, False), ('android/Missing.kt', 1, False)])

    def test_t497_unknown_functions_remain_visible_and_fail(self):
        functions = [Function('common/src/a.rs', 'rust', 1, 5, 'rust'),
                     Function('android/A.kt', 'callback', 1, 5, 'kotlin')]
        manifest = {'functions': [asdict(function) for function in functions]}
        rows = collect(manifest, {'common/src/a.rs': {1: 1, 2: 0}}, ([], {}), [])
        self.assertEqual(len(rows), 2)
        self.assertFalse(any(row['passes'] for row in rows))
        counts = summary(rows)
        self.assertEqual(counts['kotlin']['unmeasured'], 1)
        self.assertEqual(counts['rust']['below_80'], 1)
        self.assertTrue(selected(functions[0], ['common/']))
        self.assertFalse(selected(functions[1], ['common/']))
        with self.assertRaises(ValueError): collect(manifest, {}, ([], {}), ['missing/'])
