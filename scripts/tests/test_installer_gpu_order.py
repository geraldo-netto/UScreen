"""T700: provision GPU pre-dependencies without changing the running machine."""
# Copyright (c) 2026 Geraldo Netto
from pathlib import Path
import subprocess
import random
import tempfile
import unittest

from shell_fixture import run
from test_installer import REPO, SOURCE
from test_evdi_setup import setup_program


STUBS = r'''
T700_ROOT=$1
SCRIPT_DIR=$2
sudo() {
    case "$1" in
        mkdir) command mkdir -p "$T700_ROOT/etc/modprobe.d" "$T700_ROOT/etc/modules-load.d" ;;
        bash) command bash "$2" "$T700_ROOT/sys/class/drm" "$T700_ROOT/etc/modprobe.d" ;;
        tee) command tee "$T700_ROOT$2" ;;
        sh) command sh -c "$3" fixture "$T700_ROOT$5" ;;
        *) echo "unexpected privileged command: $1" >&2; return 99 ;;
    esac
}
configure_boot_modules "$T700_ROOT/sys/class/drm"
'''


class InstallerGpuOrderTest(unittest.TestCase):
    def helper(self, root, prefix=''):
        return run(prefix + '\nexec bash "$@"\n',
                   [REPO / 'scripts/gpu-boot-order.sh', root / 'sys/class/drm',
                    root / 'etc/modprobe.d'], capture_output=True, text=True, timeout=10)

    def card(self, root, name, module):
        card = root / 'sys/class/drm' / name
        provider = root / 'sys/devices' / name
        native_card = provider / 'drm' / name
        native_card.mkdir(parents=True)
        card.parent.mkdir(parents=True, exist_ok=True)
        card.symlink_to(native_card)
        (native_card / 'device').symlink_to(provider)
        driver = provider / 'driver'
        driver.mkdir(parents=True)
        if module is not None:
            target = root / 'sys/module' / module
            target.mkdir(parents=True, exist_ok=True)
            (driver / 'module').symlink_to(target)

    def configure(self, root):
        result = run(SOURCE + STUBS, [root, REPO / 'scripts'], cwd=REPO,
                     capture_output=True, text=True, timeout=10)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        return result.stdout + result.stderr

    def test_t700_real_gpu_is_a_predependency_of_boot_evdi(self):
        for module in ['amdgpu', 'i915', 'xe', 'nouveau', 'nvidia', 'virtio_gpu', 'virtio_pci']:
            with self.subTest(module=module), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                self.card(root, 'card0', None)  # Built-in simpledrm is not a loadable GPU.
                self.card(root, 'card1', 'evdi')
                self.card(root, 'card4', module)
                self.configure(root)
                config = root / 'etc/modprobe.d/blent-gpu-order.conf'
                self.assertTrue(config.exists(), 'T700: boot EVDI has no physical-GPU predependency')
                expected = {'nvidia': 'nvidia_drm', 'virtio_pci': 'virtio_gpu'}.get(module, module)
                self.assertEqual(config.read_text(), f'softdep evdi pre: {expected}\n')

    def test_t700_multiple_gpus_are_deduplicated_and_connectors_are_ignored(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for name, module in [('card0', 'amdgpu'), ('card1', 'i915'),
                                 ('card2', 'amdgpu'), ('card3', 'evdi'),
                                 ('card0-HDMI-A-1', 'unrelated')]:
                self.card(root, name, module)
            self.configure(root)
            self.assertEqual((root / 'etc/modprobe.d/blent-gpu-order.conf').read_text(),
                             'softdep evdi pre: amdgpu i915\n')

    def test_t700_existing_admin_gpu_order_is_preserved(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.card(root, 'card0', 'amdgpu')
            config = root / 'etc/modprobe.d/blent-gpu-order.conf'
            config.parent.mkdir(parents=True)
            before = '# managed locally\nsoftdep evdi pre: i915\n'
            config.write_text(before)
            self.configure(root)
            self.assertEqual(config.read_text(), before)

    def test_t700_failed_write_does_not_publish_partial_boot_configuration(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.card(root, 'card0', 'amdgpu')
            result = self.helper(root, r'''
printf() {
    if [[ $1 == 'softdep evdi pre: %s\n' ]]; then builtin printf partial; return 17; fi
    builtin printf "$@"
}
export -f printf
''')
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(list((root / 'etc/modprobe.d').iterdir()), [],
                             'T700: failed write left a partial boot configuration')

    def test_t700_invalid_module_metadata_never_becomes_configuration(self):
        randomizer = random.Random(700)
        invalid = ['amdgpu\n', 'amdgpu\noptions evdi initial_device_count=99',
                   '-amdgpu', 'a' * 65, 'bad name', 'bad;name', '#comment']
        invalid += ['gpu' + randomizer.choice(' \n\t;#$') + str(i) for i in range(16)]
        for module in invalid:
            with self.subTest(module=repr(module)), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                self.card(root, 'card0', module)
                result = self.helper(root)
                self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
                self.assertIn('Invalid GPU module metadata', result.stderr)
                self.assertFalse((root / 'etc/modprobe.d/blent-gpu-order.conf').exists())

    def test_t700_module_token_boundaries(self):
        for module in ['a', 'a' * 64, 'gpu_test-1', '3dfx']:
            with self.subTest(module=module), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                self.card(root, 'card0', module)
                result = self.helper(root)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual((root / 'etc/modprobe.d/blent-gpu-order.conf').read_text(),
                                 f'softdep evdi pre: {module}\n')

    def test_t700_drm_provider_takes_precedence_over_transport_module(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.card(root, 'card0', 'virtio_gpu')
            transport = root / 'sys/devices/pci-transport'
            (transport / 'driver').mkdir(parents=True)
            module = root / 'sys/module/virtio_pci'
            module.mkdir()
            (transport / 'driver/module').symlink_to(module)
            device = root / 'sys/class/drm/card0/device'
            device.unlink()
            device.symlink_to(transport)
            result = self.helper(root)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual((root / 'etc/modprobe.d/blent-gpu-order.conf').read_text(),
                             'softdep evdi pre: virtio_gpu\n')

    def test_t700_missing_builtin_virtual_and_vanished_devices_do_not_invent_drivers(self):
        for modules in [[], [None], ['evdi', 'simpledrm']]:
            with self.subTest(modules=modules), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                for index, module in enumerate(modules):
                    self.card(root, f'card{index}', module)
                drm = root / 'sys/class/drm'
                drm.mkdir(parents=True, exist_ok=True)
                (drm / 'card99').symlink_to(root / 'vanished')
                result = self.helper(root)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertIn('No loadable physical GPU found', result.stderr)
                self.assertFalse((root / 'etc/modprobe.d/blent-gpu-order.conf').exists())

    def test_t700_managed_links_are_preserved_without_following_them(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.card(root, 'card0', 'amdgpu')
            config = root / 'etc/modprobe.d/blent-gpu-order.conf'
            config.parent.mkdir(parents=True)
            target = root / 'absent-managed-config'
            config.symlink_to(target)
            result = self.helper(root)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn('preserved', result.stderr)
            self.assertTrue(config.is_symlink())
            self.assertFalse(target.exists())

    def test_t700_staging_and_publish_failures_leave_no_owned_files(self):
        for command in ['mktemp', 'chmod', 'ln']:
            with self.subTest(command=command), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                self.card(root, 'card0', 'amdgpu')
                prefix = f'{command}() {{ return 23; }}\nexport -f {command}\n'
                result = self.helper(root, prefix)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(list((root / 'etc/modprobe.d').iterdir()), [])

    def test_t700_concurrent_admin_creation_is_preserved(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.card(root, 'card0', 'amdgpu')
            result = self.helper(root, r'''
ln() { printf 'admin-created\n' > "${@: -1}"; command ln "$@"; }
export -f ln
''')
            self.assertNotEqual(result.returncode, 0)
            config = root / 'etc/modprobe.d/blent-gpu-order.conf'
            self.assertEqual(config.read_text(), 'admin-created\n')
            self.assertEqual(list(config.parent.iterdir()), [config])

    def test_t700_concurrent_directory_cannot_redirect_publication(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.card(root, 'card0', 'amdgpu')
            result = self.helper(root, r'''
ln() { mkdir "${@: -1}"; command ln "$@"; }
export -f ln
''')
            self.assertNotEqual(result.returncode, 0, 'T700: publication followed a concurrent directory')
            config = root / 'etc/modprobe.d/blent-gpu-order.conf'
            self.assertEqual(list(config.iterdir()), [])
            self.assertEqual(list(config.parent.iterdir()), [config])

    def test_t700_native_package_hooks_configure_gpu_before_evdi(self):
        for entry in ['rpm', 'post_install', 'post_upgrade']:
            with self.subTest(entry=entry), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                self.card(root, 'card0', 'amdgpu')
                source, args = setup_program(entry)
                stubs = r'''
T700_ROOT=$1
T700_HELPER=$2
shift 2
bash() {
    [[ $1 == /usr/share/blent/gpu-boot-order.sh ]] || return 91
    command bash "$T700_HELPER" "$T700_ROOT/sys/class/drm" "$T700_ROOT/etc/modprobe.d"
}
sh() {
    [[ $1 == /usr/share/blent/setup-evdi.sh ]] || return 92
    [[ $(cat "$T700_ROOT/etc/modprobe.d/blent-gpu-order.conf") == 'softdep evdi pre: amdgpu' ]] || return 93
    echo 'T700 GPU configured before EVDI'
}
touch() { :; }
modprobe() { :; }
udevadm() { :; }
gtk-update-icon-cache() { :; }
'''
                result = run(stubs + source.replace('%{_datadir}', '/usr/share'),
                             [root, REPO / 'scripts/gpu-boot-order.sh', *args],
                             capture_output=True, text=True, timeout=10)
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                self.assertIn('T700 GPU configured before EVDI', result.stdout)

    def test_t700_invalid_helper_arguments_and_config_directory_fail(self):
        result = subprocess.run(['bash', str(REPO / 'scripts/gpu-boot-order.sh'),
                                 'one', 'two', 'three'], capture_output=True, text=True, timeout=10)
        self.assertEqual(result.returncode, 2)
        self.assertIn('Usage:', result.stderr)
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'etc').mkdir()
            (root / 'etc/modprobe.d').write_text('not a directory')
            self.assertNotEqual(self.helper(root).returncode, 0)

    def test_t700_make_setup_configures_order_before_loading_evdi(self):
        result = subprocess.run(['make', '-n', 'setup-system'], cwd=REPO,
                                capture_output=True, text=True, timeout=10)
        self.assertEqual(result.returncode, 0, result.stderr)
        commands = result.stdout
        order = 'sudo bash scripts/gpu-boot-order.sh'
        self.assertIn(order, commands, 'T700: make setup omitted GPU boot ordering')
        self.assertLess(commands.index(order), commands.index('sudo sh scripts/setup-evdi.sh'))


if __name__ == '__main__':
    unittest.main()
