"""T497: explicit platform scope cannot hide shared or Windows coverage gaps."""
from dataclasses import asdict
from pathlib import Path
import tempfile
import unittest

from inventory import syntax_functions
from platform_scope import apply


class PlatformScopeTest(unittest.TestCase):
    def test_t497_only_provably_unsupported_platforms_leave_the_selected_scope(self):
        sources = {
            'common/src/lib.rs': '#[cfg(windows)]\nmod windows;\n#[cfg(target_os="macos")]\nmod macos;\nmod shared;\n',
            'common/src/windows.rs': 'fn native() {}\n',
            'common/src/macos.rs': 'fn native() {}\n',
            'common/src/shared.rs': '#[cfg(any(feature="unknown", windows))]\nfn shared() {}\n',
        }
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            functions = []
            for name, source in sources.items():
                path = root/name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(source)
                functions.extend(syntax_functions(Path(name), source, 'rust'))
            manifest = dict(sources=dict.fromkeys(sources, ''), functions=[asdict(f) for f in functions])
            rows = [dict(file=f.file, line=f.first, language='rust', passes=False) for f in functions]
            rows.append(dict(file='app/Thing.kt', line=1, language='kotlin', passes=False))
            selected, excluded = apply(root, manifest, rows, ['linux', 'windows'])
            self.assertEqual([r['file'] for r in excluded], ['common/src/macos.rs'])
            self.assertEqual(len(selected), 3)
            with self.assertRaises(ValueError): apply(root, manifest, excluded, ['linux', 'windows'])
            self.assertFalse(any(row['passes'] for row in selected))
            self.assertEqual(apply(root, manifest, rows, []), (rows, []))
            linux, foreign = apply(root, manifest, rows, ['linux'])
            self.assertEqual(len(linux), 2)
            self.assertEqual(len(foreign), 2)
            with self.assertRaises(ValueError): apply(root, manifest, rows, ['unknown'])

    def test_t497_windows_boolean_predicates_do_not_exclude_unknown_features(self):
        from rust_platform import unavailable
        for condition, expected in [
            ('windows', False), ('not(windows)', True), ('unix', True),
            ('target_os="windows"', False), ('target_family="windows"', False),
            ('all(feature="x",unix)', True), ('any(feature="x",unix)', False),
            ('not(any(windows,unix))', True), ('any(target_os="linux",windows)', False),
        ]:
            self.assertEqual(unavailable(f'#[cfg({condition})]', 'windows'), expected)

    def test_t497_native_import_rejects_absent_partial_changed_and_invalid_attestation(self):
        import json
        from model import fingerprint
        from platform_scope import attest_native
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root/'module.rs'
            source.write_text('fn work() {}\n')
            stamp = root/'sources.json'
            data = {'module.rs': {1: 1}}
            with self.assertRaises(ValueError): attest_native(root, [], data, ['windows'])
            attest_native(root, [], data, ['linux'])
            for invalid in [{}, [], None, {'module.rs': '0'*64}, {'../escape.rs': '0'*64}]:
                stamp.write_text(json.dumps(invalid))
                with self.assertRaises(ValueError): attest_native(root, [stamp], data, ['windows'])
            stamp.write_text(json.dumps({'module.rs': fingerprint(source)}))
            attest_native(root, [stamp], data, ['linux', 'windows'])
            attest_native(root, [stamp], dict(data, **{'testdata/daemon_process.rs': {1: 1}}), ['linux', 'windows'])
            with self.assertRaises(ValueError):
                attest_native(root, [stamp], dict(data, **{'missing.rs': {1: 1}}), ['windows'])
            source.write_text('fn changed() {}\n')
            with self.assertRaises(ValueError): attest_native(root, [stamp], data, ['windows'])
