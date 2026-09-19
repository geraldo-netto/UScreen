"""T497: compiler PSI distinguishes authored callbacks, accessors and constructors."""
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import kotlin_inventory


class KotlinInventoryTest(unittest.TestCase):
    def test_t497_real_lambda_body_entries_disambiguate_shared_declaration_lines(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root/'Fixture.kt'
            source.write_text('''fun fixture() = consume {
    consume {
        println("inner")
    }
}
''')
            rows = kotlin_inventory.functions(root, [source])
            outer, inner = [row for row in rows if row.name == '<lambda>']
            self.assertEqual((outer.first, outer.body), (1, 2))
            self.assertEqual((inner.first, inner.body), (2, 3))

    def test_t497_real_compiler_inventory_and_cached_repeat_agree(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root/'Fixture.kt'
            source.write_text('''class Fixture {
    constructor(value: Int) { require(value > 0) { "positive" } }
    val value: Int get() = 7
    fun execute(block: () -> Int): Int = block()
    fun choice() = execute { value }
}
''')
            real_cache = kotlin_inventory.cache_dir()
            cache = root/'compiler'
            cache.mkdir()
            resolve = kotlin_inventory.artifact
            with patch.object(kotlin_inventory, 'cache_dir', return_value=cache), \
                    patch.object(kotlin_inventory, 'artifact', side_effect=lambda _, key, digest: resolve(real_cache, key, digest)):
                first = kotlin_inventory.functions(root, [source])
                self.assertEqual(first, kotlin_inventory.functions(root, [source]))
            self.assertEqual([row.name for row in first], ['<init>', '<lambda>', 'get:value', 'execute', 'choice', '<lambda>'])
            self.assertTrue(all(row.file == 'Fixture.kt' and row.last >= row.first > 0 for row in first))
            self.assertEqual(kotlin_inventory.functions(root, []), [])

    def test_t497_compiler_result_rejects_unowned_paths_and_malformed_records(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for record in ['broken', f'{root}/../outside.kt\t1\t2\trun\t1', f'{root}/a.kt\tnan\t2\trun\t1']:
                with self.assertRaises(ValueError): kotlin_inventory.parse_result(root, record)
