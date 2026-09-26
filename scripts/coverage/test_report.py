"""T497: per-function gates cannot be replaced by aggregate percentages."""
import gzip
import json
from pathlib import Path
import random
import tempfile
import unittest
from unittest.mock import patch

import model
import readers


class ContractTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)

    def fixture(self, name, data):
        path = self.root / name
        path.write_bytes(data)
        return path

    def test_t497_one_uncovered_function_fails_even_with_high_aggregate_coverage(self):
        data = {'source.rs': {line: 1 for line in range(1, 100)}}
        data['source.rs'][100] = 0
        covered = model.Function('source.rs', 'covered', 1, 99, 'rust')
        missed = model.Function('source.rs', 'missed', 100, 100, 'rust')
        self.assertTrue(model.result(covered, data)['passes'])
        self.assertFalse(model.result(missed, data)['passes'])
        self.assertFalse(model.result(model.Function('missing.rs', 'absent', 1, 2, 'rust'), data)['passes'])

    def test_t497_exact_threshold_and_zero_counters_survive_report_merges(self):
        data = {}
        for n in range(1, 6): model.add_line(data, 'a.c', n, int(n < 5))
        fn = model.Function('a.c', 'boundary', 1, 5, 'c')
        self.assertTrue(model.result(fn, data)['passes'])
        model.add_line(data, 'a.c', 1, 0)
        self.assertTrue(model.result(fn, data)['passes'])
        model.add_line(data, 'a.c', 6, 0)
        self.assertFalse(model.result(model.Function('a.c', 'below', 1, 6, 'c'), data)['passes'])

    def test_t497_stale_or_external_sources_are_rejected(self):
        source = self.fixture('a.rs', b'fn a() {}\n')
        manifest = {'a.rs': model.fingerprint(source)}
        model.verify_sources(self.root, manifest)
        source.write_bytes(b'changed')
        with self.assertRaises(ValueError): model.verify_sources(self.root, manifest)
        with self.assertRaises(ValueError): model.verify_sources(self.root, {'../a.rs': 'invalid'})
        self.assertIsNone(model.source_path(self.root, '../outside'))
        self.assertIsNone(model.source_path(self.root, '/different/root.rs', Path('/work')))
        self.assertEqual(model.source_path(self.root, '/work/a.rs', Path('/work')), 'a.rs')

    def test_t497_invalid_counts_and_line_bounds_are_fuzzed(self):
        rng = random.Random(497)
        for invalid in [True, False, None, '1', 1.0, -1, 1 << 63] + [-rng.randrange(1, 1 << 100) for _ in range(256)]:
            with self.assertRaises(ValueError): model.add_line({}, 'a.rs', 1, invalid)
            with self.assertRaises(ValueError): model.add_line({}, 'a.rs', invalid, 1)
        with self.assertRaises(ValueError): model.add_line({}, 'a.rs', 0, 1)

    def test_t497_lcov_preserves_uncovered_lines_and_record_boundaries(self):
        path = self.fixture('report.lcov', b'SF:/work/a.rs\nDA:1,2\nDA:2,0\nend_of_record\nDA:3,99\nSF:/dependency/src.rs\nDA:1,10\n')
        data = {}
        readers.lcov(path, self.root, data, Path('/work'))
        self.assertEqual(data, {'a.rs': {1: 2, 2: 0}})
        path.write_bytes(b'SF:a.rs\nDA:0,1\n')
        with self.assertRaises(ValueError): readers.lcov(path, self.root, {})

    def test_t497_native_formats_keep_the_same_line_semantics(self):
        data = {}
        report = {'files': {'a.py': {'executed_lines': [1], 'missing_lines': [2]}}}
        readers.python_json(self.fixture('python.json', json.dumps(report).encode()), self.root, data)
        report = {'current_working_directory': str(self.root), 'files': [{'file': 'a.c', 'lines': [{'line_number': 1, 'count': 4}, {'line_number': 2, 'count': 0}]}]}
        readers.gcov_json(self.fixture('gcov.gz', gzip.compress(json.dumps(report).encode())), self.root, data)
        xml = b'<report><package name="com/example"><sourcefile name="A.kt"><line nr="1" ci="2" mi="0"/><line nr="2" ci="0" mi="3"/></sourcefile></package></report>'
        readers.jacoco(self.fixture('jacoco.xml', xml), self.root, self.root, data)
        self.assertEqual(data, {'a.py': {1: 1, 2: 0}, 'a.c': {1: 4, 2: 0}, 'com/example/A.kt': {1: 2, 2: 0}})

    def test_t636_windows_lcov_paths_merge_without_losing_zero_counters(self):
        report = b'SF:common\\src\\windows\\private.rs\nDA:20,1\nDA:21,0\nend_of_record\nSF:common/src/windows/private.rs\nDA:22,3\nend_of_record\n'
        data = {}
        readers.lcov(self.fixture('windows.lcov', report), self.root, data)
        self.assertEqual(data, {'common/src/windows/private.rs': {20: 1, 21: 0, 22: 3}})

    def test_t636_absolute_windows_paths_require_an_explicit_matching_origin(self):
        origin = Path(r'D:\work\repo')
        self.assertEqual(model.source_path(self.root, r'd:\WORK\repo\common\src\a.rs', origin), 'common/src/a.rs')
        self.assertEqual(model.source_path(self.root, r'\\server\share\repo\a.rs', Path(r'\\server\share\repo')), 'a.rs')
        for name in [r'D:\work\repository\a.rs', r'C:\work\repo\a.rs', r'D:a.rs', r'\rooted\a.rs']:
            self.assertIsNone(model.source_path(self.root, name, origin), name)
        self.assertIsNone(model.source_path(self.root, r'D:\work\repo\a.rs'))

    def test_t636_windows_traversal_cannot_escape_the_source_tree(self):
        for levels in range(1, 9):
            relative = '..\\' * levels + 'outside.rs'
            self.assertIsNone(model.source_path(self.root, relative))
            self.assertIsNone(model.source_path(self.root, 'D:\\repo\\' + relative, Path(r'D:\repo')))
        self.assertEqual(model.source_path(self.root, r'common\src\..\a.rs'), 'common/a.rs')

    def test_t636_source_fingerprints_still_require_identical_checkout_bytes(self):
        path = self.fixture('a.rs', b'fn a() {}\n')
        manifest = {'a.rs': model.fingerprint(path)}
        path.write_bytes(b'fn a() {}\r\n')
        with self.assertRaises(ValueError): model.verify_sources(self.root, manifest)

    def test_t497_oversized_compressed_reports_fail_before_parsing(self):
        path = self.fixture('oversize.gz', gzip.compress(b'x' * 129))
        with patch.object(readers, 'LIMIT', 128):
            with self.assertRaises(ValueError): readers.read_bounded(path, compressed=True)


if __name__ == '__main__':
    unittest.main()
