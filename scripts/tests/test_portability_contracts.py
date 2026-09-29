"""T497: shipping rejects unknown/new glibc requirements and broken helper linkage."""
import importlib.util
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

REPO = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location('portability', REPO / 'scripts/ci/verify-portability.py')
portability = importlib.util.module_from_spec(spec)
spec.loader.exec_module(portability)


class PortabilityContractsTest(unittest.TestCase):
    def test_t497_glibc_floor_checks_every_symbol_and_rejects_unknowns(self):
        for symbols, allowed in [('GLIBC_2.2.5 GLIBC_2.36', True), ('GLIBC_2.9 GLIBC_2.40', False),
                                 ('GLIBC_PRIVATE', False), ('', False)]:
            with self.subTest(symbols=symbols), patch.object(portability.subprocess, 'check_output', return_value=symbols):
                if allowed:
                    portability.verify_abi(Path('fixture'))
                else:
                    with self.assertRaises(ValueError): portability.verify_abi(Path('fixture'))

    def test_t497_bundle_requires_sibling_soname_and_origin_search(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root/'libevdi.so.1.15.0').touch()
            with patch.object(portability, 'verify_abi') as abi:
                with self.assertRaisesRegex(ValueError, 'SONAME link'): portability.verify_bundle(root)
                self.assertEqual(abi.call_count, 5)
                (root/'libevdi.so.1').symlink_to('libevdi.so.1.15.0')
                self.check_dynamic(root)

    def test_t718_audio_helper_must_meet_bundle_glibc_floor(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'libevdi.so.1.15.0').touch()
            (root / 'libevdi.so.1').symlink_to('libevdi.so.1.15.0')
            def output(command, **kwargs):
                if command[0] == 'readelf':
                    return '(RUNPATH) [$ORIGIN]\n(NEEDED) [libevdi.so.1]'
                return 'GLIBC_2.40' if Path(command[-1]).name == 'blent-audio' else 'GLIBC_2.36'
            with patch.object(portability.subprocess, 'check_output', side_effect=output):
                with self.assertRaisesRegex(ValueError, 'blent-audio.*exceeds 2.36'):
                    portability.verify_bundle(root)

    def check_dynamic(self, root):
        needed = '(NEEDED) Shared library: [libevdi.so.1]'
        for dynamic, error in [(needed, 'load replaceable'), ('(RUNPATH) [$ORIGIN]', 'bundled libevdi'),
                               ('(RUNPATH) [$ORIGIN]\n' + needed, None)]:
            with self.subTest(dynamic=dynamic), patch.object(portability.subprocess, 'check_output', return_value=dynamic):
                if error:
                    with self.assertRaisesRegex(ValueError, error): portability.verify_bundle(root)
                else:
                    portability.verify_bundle(root)


if __name__ == '__main__':
    unittest.main()
