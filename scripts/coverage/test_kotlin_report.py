"""T497: callback allocation cannot masquerade as callback execution."""
from pathlib import Path
import tempfile
import unittest

from kotlin_report import assign_methods, function_key, function_result, read
from model import Function


class KotlinReportTest(unittest.TestCase):
    def test_t497_nested_declaration_line_does_not_steal_the_parent_method(self):
        outer = Function('A.kt', '<lambda>', 2, 8, 'kotlin', body=3)
        inner = Function('A.kt', '<lambda>', 3, 7, 'kotlin', body=4)
        parent_method = dict(file='A.kt', name='invoke', line=3, covered=3, total=3)
        child_method = dict(file='A.kt', name='invoke', line=4, covered=0, total=2)
        assigned = assign_methods([outer, inner], [parent_method, child_method])
        self.assertEqual(assigned, {function_key(outer): [parent_method], function_key(inner): [child_method]})
        self.assertTrue(function_result(outer, assigned[function_key(outer)], {})['passes'])
        self.assertFalse(function_result(inner, assigned[function_key(inner)], {})['passes'])

    def test_t497_nested_callbacks_are_attributed_to_their_smallest_source_body(self):
        parent = Function('A.kt', 'parent', 1, 10, 'kotlin')
        outer = Function('A.kt', '<lambda>', 2, 8, 'kotlin')
        inner = Function('A.kt', '<lambda>', 3, 4, 'kotlin')
        method = dict(file='A.kt', name='parent$lambda$0', line=3, covered=0, total=2)
        assigned = assign_methods([parent, outer, inner], [method])
        self.assertEqual(assigned, {function_key(inner): [method]})
    def test_t497_same_line_parent_cannot_cover_a_never_called_callback(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            path = root/'jacoco.xml'
            path.write_text('''<report><package name="com/example">
<class name="com/example/A" sourcefilename="A.kt"><method name="parent" desc="()V" line="1"><counter type="LINE" covered="5" missed="0"/></method></class>
<class name="com/example/A$parent$1" sourcefilename="A.kt"><method name="invoke" desc="()V" line="2"><counter type="LINE" covered="0" missed="1"/></method></class>
<sourcefile name="A.kt"><line nr="1" ci="5" mi="0"/><line nr="2" ci="5" mi="7"/></sourcefile>
</package></report>''')
            methods, data = read(path, root, root)
            parent = Function('com/example/A.kt', 'parent', 1, 5, 'kotlin')
            callback = Function('com/example/A.kt', '<lambda>', 2, 2, 'kotlin')
            self.assertTrue(function_result(parent, methods, data)['passes'])
            self.assertFalse(function_result(callback, methods, data)['passes'])
            self.assertFalse(function_result(callback, [], data)['passes'])

    def test_t497_explicit_getters_match_without_counting_generated_default_wrappers(self):
        function = Function('Prefs.kt', 'get:enabled', 2, 3, 'kotlin')
        methods = [dict(file='Prefs.kt', name='getEnabled', line=2, covered=1, total=1),
                   dict(file='Prefs.kt', name='getEnabled$default', line=2, covered=0, total=1)]
        self.assertTrue(function_result(function, methods, {})['passes'])
        self.assertFalse(function_result(function, [], {})['passes'])
