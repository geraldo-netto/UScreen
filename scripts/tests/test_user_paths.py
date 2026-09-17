"""T233: both user installers honor the same absolute XDG base directories."""
from pathlib import Path
import subprocess
import tempfile
import unittest

from test_build_output import fixture, write


class UserPathsTest(unittest.TestCase):
    def environment(self, root, mode):
        env, _ = fixture(root, 'relative', True)
        write(root, 'host/evdi/evdi_helper', '#!/bin/sh\nexit 0\n', True)
        for name in ['gtk-update-icon-cache', 'kbuildsycoca6']:
            write(root, 'tools/' + name, '#!/bin/sh\nexit 0\n', True)
        write(root, 'tools/systemctl', '#!/bin/sh\nprintf "%s\\n" "$*" >> "$USCREEN_SERVICE_LOG"\n', True)
        env['USCREEN_SERVICE_LOG'] = str(root / 'service.log')
        defaults = [Path(env['HOME']) / '.local/share', Path(env['HOME']) / '.config']
        selected = [root / "data's space;$(touch injected)", root / "config's space;$(touch injected)"]
        for key, value in zip(['XDG_DATA_HOME', 'XDG_CONFIG_HOME'], selected):
            env.pop(key, None)
            if mode != 'unset':
                env[key] = {'absolute': str(value), 'relative': 'relative-path', 'empty': ''}[mode]
        return env, selected if mode == 'absolute' else defaults

    def verify_files(self, root, data, config):
        self.assertTrue((data / 'applications/uscreen.desktop').is_file())
        for name in ['uscreen.svg', 'uscreen-pen.svg']:
            self.assertEqual((data / 'icons/hicolor/scalable/apps' / name).read_bytes(),
                             (root / 'packaging/icons' / name).read_bytes())
        self.assertEqual((config / 'systemd/user/uscreen.service').read_bytes(),
                         (root / 'scripts/uscreen.service').read_bytes())
        self.assertFalse((root / 'injected').exists())
        self.assertFalse((root / 'relative-path').exists())

    def test_t233_make_and_script_install_into_configured_xdg_locations(self):
        for installer in ['make', 'script']:
            for mode in ['absolute', 'unset', 'relative', 'empty']:
                with self.subTest(installer=installer, mode=mode), tempfile.TemporaryDirectory() as temp:
                    root = Path(temp)
                    env, (data, config) = self.environment(root, mode)
                    source = (root / 'scripts/install.sh').read_text().removesuffix('main "$@"\n')
                    write(root, 'scripts/install-fixture.sh', source + '\ninstall_files\n')
                    command = ['make', 'install'] if installer == 'make' else ['bash', 'scripts/install-fixture.sh']
                    result = subprocess.run(command, cwd=root, env=env, capture_output=True, text=True)
                    self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                    self.verify_files(root, data, config)
                    service = (root / 'service.log').read_text()
                    self.assertIn('--user daemon-reload', service)
                    self.assertEqual('--user enable uscreen.service' in service, installer == 'script',
                                     'T233: path consolidation changed service-enable policy')


if __name__ == '__main__':
    unittest.main()
