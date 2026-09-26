"""T497: only explicitly Windows-guarded modules leave the Linux gate."""
from pathlib import Path
import tempfile
import unittest

from rust_check import reports


class RustPlatformTest(unittest.TestCase):
    def fixture(self, root):
        sources = {
            'common/src/lib.rs': '#[cfg(all(windows, feature="storage"))]\nmod windows;\nmod shared;\n',
            'common/src/windows.rs': 'mod nested;\nfn native() {}\n',
            'common/src/windows/nested.rs': 'fn native_child() {}\n',
            'common/src/shared.rs': 'fn shared() {}\n',
            'host/src/main.rs': '#[cfg(windows)]\nfn main() {}\n#[cfg(target_os="linux")]\nfn main() {}\n',
        }
        for name, source in sources.items():
            path = root/name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(source)
        from dataclasses import asdict
        from inventory import syntax_functions
        return dict(sources=dict.fromkeys(sources, ''), functions=[asdict(function)
                    for name, source in sources.items() for function in syntax_functions(Path(name), source, 'rust')])

    def test_t591_native_predicates_respect_order_and_boolean_structure(self):
        from dataclasses import asdict
        from inventory import syntax_functions
        conditions = {
            'windows_after_feature': ('all(feature="platform", windows)', False),
            'macos_after_feature': ('all(feature="commands", target_os="macos")', False),
            'either_foreign': ('any(windows, target_os="macos")', False),
            'linux_or_windows': ('any(windows, target_os="linux")', True),
            'feature_or_windows': ('any(feature="platform", windows)', True),
            'not_windows': ('not(windows)', True),
            'not_linux': ('not(target_os="linux")', False),
            'nested_foreign': ('all(feature="x", any(windows, target_os="macos"))', False),
            'unknown_feature': ('feature="x"', True),
            'unix': ('unix', True),
        }
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            manifest = self.fixture(root)
            source = ''.join(f'#[cfg({condition})]\nfn {name}() {{}}\n'
                             for name, (condition, _) in conditions.items())
            path = Path('common/src/shared.rs')
            (root/path).write_text(source)
            manifest['functions'] = [row for row in manifest['functions'] if row['file'] != str(path)]
            manifest['functions'].extend(asdict(row) for row in syntax_functions(path, source, 'rust'))
            counters = {str(path): {2 * (i + 1): 1 for i, (_, possible) in enumerate(conditions.values()) if possible},
                        'host/src/main.rs': {4: 1}}
            full, linux, foreign = reports(root, manifest, counters)
            self.assertFalse(full['passes'])
            self.assertTrue(linux['passes'])
            self.assertEqual({row['name'] for row in linux['functions'] if row['file'] == str(path)},
                             {name for name, (_, possible) in conditions.items() if possible})
            self.assertEqual(len(foreign), 8)
            counters[str(path)][4] = 1  # macOS counters must never validate a Linux run.
            with self.assertRaises(ValueError):
                reports(root, manifest, counters)

    def test_t591_reordered_guards_propagate_to_nested_modules(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            manifest = self.fixture(root)
            (root/'common/src/lib.rs').write_text(
                '#[cfg(all(feature="platform", windows))]\nmod windows;\nmod shared;\n')
            counters = {'common/src/shared.rs': {1: 1}, 'host/src/main.rs': {4: 1}}
            self.assertTrue(reports(root, manifest, counters)[1]['passes'])

    def test_t497_native_windows_gaps_never_pass_the_full_report(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            manifest = self.fixture(root)
            counters = {'common/src/shared.rs': {1: 1}, 'host/src/main.rs': {4: 1}}
            full, linux, windows = reports(root, manifest, counters)
            self.assertFalse(full['passes'])
            self.assertTrue(linux['passes'])
            self.assertEqual(len(windows), 3)
            counters['common/src/shared.rs'][1] = 0
            self.assertFalse(reports(root, manifest, counters)[1]['passes'])
            counters['host/src/main.rs'][2] = 1
            with self.assertRaisesRegex(ValueError, 'Non-Linux counters'): reports(root, manifest, counters)

    def test_t497_shared_module_does_not_inherit_one_windows_only_reference(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            manifest = self.fixture(root)
            (root/'common/src/lib.rs').write_text('#[cfg(windows)]\nmod windows;\n#[path="windows.rs"]\nmod shared;\n')
            full, linux, windows = reports(root, manifest, {})
            self.assertFalse(linux['passes'])
            self.assertEqual(len(windows), 1)
            self.assertEqual(len(linux['functions']), 4)


if __name__ == '__main__':
    unittest.main()
