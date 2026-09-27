"""T652: include! discovery must retain native guards and shared paths."""
from collections import defaultdict
from pathlib import Path
import tempfile
import unittest
import scope


class ScopeTest(unittest.TestCase):
    def test_t652_guarded_include_and_shared_descendants_keep_platform_identity(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            files = {'main.rs': '#[cfg(target_os="linux")] include!("linux_main.rs");\n#[cfg(windows)] mod windows;',
                     'linux_main.rs': '#[path="shared.rs"] mod shared;\nmod linux_only;',
                     'windows.rs': '#[path="shared.rs"] mod shared;',
                     'shared.rs': 'fn policy() {}', 'linux_only.rs': 'fn native() {}'}
            for name, text in files.items(): (root/name).write_text(text)
            paths = [Path(name) for name in files]
            direct, included = scope.reachable(scope.references(root, paths, 'linux'), ['main.rs'])
            self.assertIn('linux_main.rs', included); self.assertIn('shared.rs', included)
            self.assertIn('linux_only.rs', included)
            self.assertNotIn('windows.rs', direct | included)
            direct, included = scope.reachable(scope.references(root, paths, 'windows'), ['main.rs'])
            self.assertIn('windows.rs', direct); self.assertIn('shared.rs', direct)
            self.assertNotIn('linux_main.rs', direct | included)
            self.assertNotIn('linux_only.rs', direct | included)

    def test_t652_cycles_and_dual_include_direct_references_are_bounded(self):
        edges = defaultdict(list, main=[('same', 'mod_item'), ('same', 'macro_invocation')],
                            same=[('main', 'mod_item')])
        direct, included = scope.reachable(edges, ['main'])
        self.assertEqual(direct, {'main', 'same'})
        self.assertEqual(included, {'main', 'same'})


if __name__ == '__main__':
    unittest.main()
