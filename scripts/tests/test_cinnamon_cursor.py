"""T604: keep pointer visible through owned touchscreen lifecycle, without X11."""
from pathlib import Path
import subprocess
import unittest

ROOT = Path(__file__).resolve().parents[2]


class CinnamonCursorTests(unittest.TestCase):
    def test_t604_owned_touch_never_hides_pointer_and_policy_retires(self):
        result = subprocess.run([
            'node', str(ROOT / 'host/tests/cinnamon_cursor.js'),
            str(ROOT / 'host/src/input/linux/cinnamon_cursor.js'),
        ], capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
