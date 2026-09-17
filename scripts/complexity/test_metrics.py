"""Permanent T381 rule, boundary, parser and ownership fixtures."""
import contextlib
import io
from pathlib import Path
import subprocess
import tempfile
import unittest
from check import collect, file_scores, report, source_files
from kotlin import kotlin_scores
from syntax import embedded_python, python_scores, syntax_scores


def boundary(language, count):
    templates = {
        'rust': ('fn sample() {', 'if flag { work(); }\n', '}'),
        'c': ('void sample() {', 'if (flag) work();\n', '}'),
        'shell': ('sample() {\n', 'if true; then :; fi\n', '}\n'),
        'python': ('def sample():\n', '    if flag: work()\n', ''),
        'kotlin': ('fun sample() {\n', 'if (flag) work()\n', '}'),
    }
    before, branch, after = templates[language]
    return before + branch * (count - 1) + after


class MetricsTest(unittest.TestCase):
    def test_t381_nine_passes_ten_fails_for_every_language(self):
        for kind in ['rust', 'c', 'shell', 'python']:
            for count in [9, 10]:
                with self.subTest(language=kind, count=count):
                    source = boundary(kind, count)
                    scores = python_scores(source) if kind == 'python' else syntax_scores(source, kind)
                    self.assertEqual([row[2] for row in scores], [count])
        with contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(report([('fixture', 1, 'sample', 9)]), 0)
            self.assertEqual(report([('fixture', 1, 'sample', 10)]), 1)

    def test_t381_kotlin_boundaries_and_rules(self):
        with tempfile.TemporaryDirectory() as directory:
            paths = [Path(directory) / f'boundary{count}.kt' for count in [9, 10]]
            for path, count in zip(paths, [9, 10]):
                path.write_text(boundary('kotlin', count))
            rules = Path(directory) / 'rules.kt'
            rules.write_text('fun f() { while (a && b || c) { if (a) break }; '
                             'when(x) { 1 -> work(); else -> {} } }\nfun empty() {}')
            self.assertEqual([row[3] for row in kotlin_scores(paths + [rules])], [9, 10, 7, 1])
            rules.write_text('fun broken(')
            with self.assertRaises(subprocess.CalledProcessError):
                kotlin_scores([rules])

    def test_t381_rust_counts_closures_but_not_macro_expansion(self):
        source = 'fn f() { let c = || { if a && b { 1 } else { 2 } }; match x { 0 => 1, _ => {} } }'
        self.assertEqual(list(syntax_scores(source, 'rust'))[0][2], 5)
        source = 'fn f() { opaque! { if a && b { work() } }; }\nfn empty() {}'
        self.assertEqual([row[2] for row in syntax_scores(source, 'rust')], [1, 0])

    def test_t381_python_sonar_rules_and_nested_function_ownership(self):
        source = ('def f():\n    if a: pass\n    elif b: pass\n'
                  '    return [x for x in xs if a and b]\n    def nested():\n'
                  '        while flag: pass\n')
        self.assertEqual([row[2] for row in python_scores(source)], [4, 2])

    def test_t381_shell_cases_loops_and_short_circuit(self):
        source = 'f() { for x in a; do if true; then a && b || c; elif false; then :; fi; done; case x in a) :;; *) :;; esac; }'
        self.assertEqual(list(syntax_scores(source, 'shell'))[0][2], 8)

    def test_t381_parse_errors_fail_closed(self):
        for kind, source in [('rust', 'fn ('), ('c', 'void f( {'), ('shell', 'f() { if')]:
            with self.subTest(language=kind), self.assertRaises(ValueError):
                list(syntax_scores(source, kind))

    def test_t381_embedded_python_is_measured_at_source_lines(self):
        source = "#!/bin/sh\npython3 - <<'PY'\n" + boundary('python', 10) + 'PY\n'
        offset, body = next(embedded_python(source))
        line, name, score = next(python_scores(body))
        self.assertEqual((offset + line, name, score), (3, 'sample', 10))

    def test_t381_package_functions_are_measured(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'sample.spec'
            path.write_text('Name: sample\n%post\n' + boundary('shell', 10) + '%files\n/bin/sample\n')
            self.assertEqual(list(file_scores(path)), [(3, 'sample', 10)])

    def test_t381_git_scope_includes_new_tests_and_excludes_generated(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            subprocess.run(['git', 'init', '-q', str(root)], check=True)
            for name in ['host/src/main.rs', 'new_test.py', 'target/debug/generated.rs',
                         'android/app/build/Generated.kt', 'host/evdi/evdi_lib.h']:
                path = root / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text('')
            subprocess.run(['git', '-C', str(root), 'add', 'host'], check=True)
            self.assertEqual(source_files(root), [Path('host/src/main.rs'), Path('new_test.py')])
            self.assertEqual(list(collect(root, source_files(root))), [])


if __name__ == '__main__':
    unittest.main()
