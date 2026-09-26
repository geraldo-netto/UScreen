"""T233: both user installers honor the same absolute XDG base directories."""
from pathlib import Path
import subprocess
import tempfile
import time
import unittest

from test_build_output import fixture, write


class UserPathsTest(unittest.TestCase):
    def test_t231_unreachable_enabled_service_does_not_create_a_second_startup(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            env, (_, config) = self.environment(root, 'unset')
            write(root, 'tools/systemctl', '#!/bin/sh\n[ "$2" = is-enabled ]\n', True)
            source = (root / 'scripts/install.sh').read_text().removesuffix('main "$@"\n')
            write(root, 'scripts/install-fixture.sh', source + '\ninstall_files\n')
            result = subprocess.run(['bash', 'scripts/install-fixture.sh'], cwd=root, env=env, capture_output=True, text=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertFalse((config / 'autostart/blent.desktop').exists())

    def test_t231_failed_enable_is_not_reported_as_success(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            env, _ = self.environment(root, 'unset')
            write(root, 'tools/systemctl', '#!/bin/sh\n[ "$2" != enable ]\n', True)
            source = (root / 'scripts/install.sh').read_text().removesuffix('main "$@"\n')
            write(root, 'scripts/install-fixture.sh', source + '\ninstall_files\n')
            result = subprocess.run(['bash', 'scripts/install-fixture.sh'], cwd=root, env=env, capture_output=True, text=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertNotIn('User service enabled', result.stdout)

    def test_t231_installer_uses_desktop_autostart_without_a_user_manager(self):
        for unavailable in [1, 127]:
            for enable in [True, False]:
                with self.subTest(unavailable=unavailable, enable=enable), tempfile.TemporaryDirectory() as temp:
                    root = Path(temp)
                    env, (_, config) = self.environment(root, 'absolute')
                    write(root, 'tools/systemctl', f'#!/bin/sh\nexit {unavailable}\n', True)
                    env['BLENT_AUTOSTART_LAUNCH'] = str(root / 'launched')
                    write(root, 'target/release/blent', '#!/bin/sh\nprintf "%s\\n" "$0" "$@" > "$BLENT_AUTOSTART_LAUNCH"\n', True)
                    source = (root / 'scripts/install.sh').read_text().removesuffix('main "$@"\n')
                    write(root, 'scripts/install-fixture.sh', source + '\ninstall_files "$1"\n')
                    result = subprocess.run(['bash', 'scripts/install-fixture.sh', 'enable' if enable else 'no-enable'],
                                            cwd=root, env=env, capture_output=True, text=True)
                    self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                    entry = config / 'autostart/blent.desktop'
                    self.assertEqual(entry.exists(), enable, 'T231: fallback did not preserve autostart choice')
                    self.assertIn('desktop', result.stdout.lower(), 'T231: missing fallback explanation')
                    if enable:
                        self.verify_autostart_launch(entry, env)

    def verify_autostart_launch(self, entry, env):
        result = subprocess.run(['gio', 'launch', str(entry)], env=env, capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        marker = Path(env['BLENT_AUTOSTART_LAUNCH'])
        deadline = time.monotonic() + 2
        while not marker.exists() and time.monotonic() < deadline:
            time.sleep(0.01)
        self.assertEqual(marker.read_text().splitlines(), [str(Path(env['HOME']) / '.local/bin/blent'), 'start'])

    def test_t396_service_commands_follow_the_installed_binary_prefix(self):
        # Golden spellings follow systemd syntax: quote/C-escape arguments,
        # double specifiers, and suppress environment expansion with ':' prefix.
        names = [('.local/bin', '.local/bin'), ('custom bin', 'custom bin'),
                 ('bin "quoted" \\ $HOME %h `tick`', r'bin \"quoted\" \\ $HOME %%h `tick`'),
                 ('bin\n\tcarriage\r', r'bin\n\tcarriage\r')]
        for installer in ['make', 'script']:
            for name, encoded in names:
                with self.subTest(installer=installer, name=name), tempfile.TemporaryDirectory() as temp:
                    root = Path(temp)
                    env, (_, config) = self.environment(root, 'unset')
                    prefix = root / 'home' / name
                    escaped = str(root / 'home') + '/' + encoded
                    command = ['make', 'install', 'BIN_DIR=' + str(prefix).replace('$', '$$')]
                    if installer == 'script':
                        # Exercise the full install-files entry point, without dependencies/system setup.
                        source = (root / 'scripts/install.sh').read_text().removesuffix('main "$@"\n')
                        write(root, 'scripts/install-fixture.sh', source + '\nBIN_DIR=$1\ninstall_files\n')
                        command = ['bash', 'scripts/install-fixture.sh', str(prefix)]
                    result = subprocess.run(command, cwd=root, env=env, capture_output=True, text=True)
                    self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                    for binary in ['blent', 'evdi_helper']:
                        self.assertTrue((prefix / binary).is_file())
                    unit = (config / 'systemd/user/blent.service').read_text()
                    self.assertIn(f'ExecStart=:/usr/bin/env "{escaped}/blent" --helper "{escaped}/evdi_helper" start\n', unit)
                    self.assertIn(f'ExecStop=:/usr/bin/env "{escaped}/blent" stop\n', unit)

    def environment(self, root, mode):
        env, _ = fixture(root, 'relative', True)
        write(root, 'host/evdi/evdi_helper', '#!/bin/sh\nexit 0\n', True)
        for name in ['gtk-update-icon-cache', 'kbuildsycoca6']:
            write(root, 'tools/' + name, '#!/bin/sh\nexit 0\n', True)
        write(root, 'tools/systemctl', '#!/bin/sh\nprintf "%s\\n" "$*" >> "$BLENT_SERVICE_LOG"\n', True)
        env['BLENT_SERVICE_LOG'] = str(root / 'service.log')
        defaults = [Path(env['HOME']) / '.local/share', Path(env['HOME']) / '.config']
        selected = [root / "data's space;$(touch injected)", root / "config's space;$(touch injected)"]
        for key, value in zip(['XDG_DATA_HOME', 'XDG_CONFIG_HOME'], selected):
            env.pop(key, None)
            if mode != 'unset':
                env[key] = {'absolute': str(value), 'relative': 'relative-path', 'empty': ''}[mode]
        return env, selected if mode == 'absolute' else defaults

    def verify_files(self, root, data, config):
        self.assertTrue((data / 'applications/blent.desktop').is_file())
        for name in ['blent.svg', 'blent-pen.svg']:
            self.assertEqual((data / 'icons/hicolor/scalable/apps' / name).read_bytes(),
                             (root / 'packaging/icons' / name).read_bytes())
        template = (root / 'scripts/blent.service').read_text().splitlines()
        installed = (config / 'systemd/user/blent.service').read_text().splitlines()
        # T396 substitutes the selected prefix; all other unit policy is retained.
        self.assertEqual([line for line in installed if not line.startswith('Exec')],
                         [line for line in template if not line.startswith('Exec')])
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
                    self.assertEqual('--user enable blent.service' in service, installer == 'script',
                                     'T233: path consolidation changed service-enable policy')


if __name__ == '__main__':
    unittest.main()
