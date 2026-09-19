"""T500: the normal formatter must cover Rust include roots and their children."""
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]


class FormatTest(unittest.TestCase):
    def test_t500_included_supervisor_and_children_are_checked_and_formatted(self):
        with tempfile.TemporaryDirectory(prefix='uscreen format ') as tmp:
            root = Path(tmp)
            (root / 'scripts').mkdir()
            (root / 'host/src').mkdir(parents=True)
            shutil.copy(ROOT / 'scripts/format-rust.py', root / 'scripts')
            (root / 'Cargo.toml').write_text('[workspace]\nmembers = ["host"]\nresolver = "2"\n')
            (root / 'host/Cargo.toml').write_text('[package]\nname="formatter-fixture"\nversion="0.0.1"\nedition="2021"\n')
            (root / 'host/src/main.rs').write_text('include!("linux_main.rs");\n')
            source = root / 'host/src/linux_main.rs'
            child = root / 'host/src/worker.rs'
            source.write_text('mod worker;\nfn main(){worker::run();}\n')
            child.write_text('pub fn run(){println!("fixture");}\n')
            command = [sys.executable, str(root / 'scripts/format-rust.py')]
            before = subprocess.run(command + ['--check'], capture_output=True, text=True)
            self.assertNotEqual(before.returncode, 0, 'T500: included sources escaped checking')
            self.assertIn('worker.rs', before.stdout)
            formatted = subprocess.run(command, capture_output=True, text=True)
            self.assertEqual(formatted.returncode, 0, formatted.stdout + formatted.stderr)
            checked = subprocess.run(command + ['--check'], capture_output=True, text=True)
            self.assertEqual(checked.returncode, 0, checked.stdout + checked.stderr)
            self.assertIn('fn main() {', source.read_text())
            self.assertIn('pub fn run() {', child.read_text())
            malformed = subprocess.run(command + ['--check=2'], capture_output=True, text=True)
            self.assertEqual(malformed.returncode, 2)


if __name__ == '__main__':
    unittest.main()
