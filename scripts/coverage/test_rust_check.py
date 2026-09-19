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
            with self.assertRaisesRegex(ValueError, 'Windows-only counters'): reports(root, manifest, counters)

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
