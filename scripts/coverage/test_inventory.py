"""T497: fixtures cannot hide production code or inflate enclosing functions."""
from pathlib import Path
import tempfile
import unittest

from inventory import coverage_source, fixture, rust_test_files, syntax_functions
from model import result


class InventoryTest(unittest.TestCase):
    def test_t497_application_scope_excludes_benchmarks_in_every_language(self):
        for name in ['host/src/main.rs', 'common/src/lib.rs', 'host/evdi/evdi_helper.c', 'android/app/src/main/java/com/uscreen/MainActivity.kt',
                     'scripts/install.sh', 'scripts/setup-evdi.sh', 'scripts/ci/verify-portability.py',
                     'packaging/appimage/build.py', 'packaging/arch/PKGBUILD', 'packaging/appimage/AppRun']:
            self.assertTrue(coverage_source(Path(name)), name)
        for name in ['common/examples/encoder-options.rs', 'host/benches/queue.rs', 'scripts/benchmarks/main.rs', 'scripts/benchmarks/helper.c',
                     'scripts/benchmarks/app/Main.kt', 'scripts/coverage/KotlinFunctions.kt', 'scripts/fake-tablet.py',
                     'scripts/ci/gui-smoke.sh', 'docs/benchmarks/fixture.c']:
            self.assertFalse(coverage_source(Path(name)), name)

    def test_t497_rust_cfg_test_modules_are_excluded_but_not_production(self):
        source = '''fn production() { }
#[cfg(test)] mod tests { fn fixture() {} }
#[cfg(not(test))] fn normal_only() {}
#[cfg(any(test, feature = "runtime"))] fn shared() {}
'''
        functions = list(syntax_functions(Path('lib.rs'), source, 'rust'))
        self.assertEqual([fn.name for fn in functions], ['production', 'normal_only', 'shared'])
        self.assertTrue(fixture(Path('host/tests/helper.c')))
        self.assertFalse(fixture(Path('host/src/production.rs')))

    def test_t497_external_test_modules_preserve_shared_production_sources(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root/'lib.rs').write_text('''#[cfg(test)] #[path="fixture.rs"] mod a;
#[path="fixture.rs"] mod b;
#[cfg(test)] #[path="profile.rs"] mod c;
''')
            self.assertEqual(rust_test_files(root, [Path('lib.rs')]), {root/'profile.rs'})

    def test_t497_nested_functions_cannot_inflate_the_parent(self):
        source = 'fn outer() {\n fn inner() {\n  todo!();\n }\n inner();\n}\n'
        functions = list(syntax_functions(Path('a.rs'), source, 'rust'))
        self.assertEqual(functions[0].nested, ((2, 4),))
        data = {'a.rs': {1: 1, 2: 0, 3: 0, 4: 0, 5: 1, 6: 1}}
        self.assertEqual(result(functions[0], data)['percent'], 100)
        self.assertEqual(result(functions[1], data)['percent'], 0)

    def test_t497_implicit_test_module_paths_propagate_without_hiding_shared_code(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root/'lib.rs').write_text('#[cfg(test)] mod contracts;\nmod shared;\n')
            (root/'contracts.rs').write_text('#[path="shared.rs"] mod shared;\nmod nested;\n')
            (root/'shared.rs').write_text('fn production() {}')
            (root/'contracts').mkdir()
            (root/'contracts/nested.rs').write_text('fn fixture() {}')
            files = [path.relative_to(root) for path in root.rglob('*.rs')]
            self.assertEqual(rust_test_files(root, files), {root/'contracts.rs', root/'contracts/nested.rs'})


if __name__ == '__main__':
    unittest.main()
