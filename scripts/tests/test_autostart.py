"""T536: installer login startup must not depend on graphical-session.target."""
from pathlib import Path
import subprocess
import tempfile
import unittest

import autostart_fixture
from test_build_output import write
import test_user_paths


class AutostartTest(unittest.TestCase):
    def check_install(self, root, mode, previously_enabled):
        env, (_, config) = test_user_paths.UserPathsTest().environment(root, 'absolute')
        state = autostart_fixture.manager(root, env, root / 'tools/systemctl')
        if previously_enabled:
            (state / 'enabled').touch()
        source = (root / 'scripts/install.sh').read_text().removesuffix('main "$@"\n')
        write(root, 'scripts/install-fixture.sh', source + '\ninstall_files "$1"\n')
        expected = previously_enabled or mode == 'enable'
        for attempt in range(2):
            result = subprocess.run(['bash', 'scripts/install-fixture.sh', mode], cwd=root,
                                    env=env, capture_output=True, text=True, timeout=20)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            entry = config / 'autostart/uscreen.desktop'
            self.assertEqual(entry.exists(), expected, 'T536: service preference has no working desktop login route')
            self.assertEqual((state / 'enabled').exists(), expected)
            self.assertFalse((state / 'launches').exists(), 'T536: installation started the daemon')
        if expected:
            self.assertEqual(autostart_fixture.login(entry, env, 1), 'daemon\n')
            self.assertEqual(autostart_fixture.login(entry, env, 2), 'daemon\n')

    def test_t536_installer_enable_and_reinstall_preserve_cinnamon_login(self):
        for mode in ['enable', 'no-enable']:
            for enabled in [False, True]:
                with self.subTest(mode=mode, enabled=enabled), tempfile.TemporaryDirectory() as temp:
                    self.check_install(Path(temp), mode, enabled)


if __name__ == '__main__':
    unittest.main()
