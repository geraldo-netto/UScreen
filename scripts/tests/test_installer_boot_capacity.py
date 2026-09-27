"""T709: installing again must preserve explicitly provisioned boot capacity."""
from pathlib import Path
import tempfile
import unittest

from shell_fixture import run
from test_installer import REPO, SOURCE


STUBS = r'''
T709_ROOT=$1
sudo() {
    case "$1" in
        mkdir) command mkdir -p "$T709_ROOT/etc/modprobe.d" "$T709_ROOT/etc/modules-load.d" ;;
        tee) command tee "$T709_ROOT$2" ;;
        sh) command sh -c "$3" fixture "$T709_ROOT$5" ;;
        *) echo "unexpected privileged command: $1" >&2; return 99 ;;
    esac
}
configure_boot_modules
'''


class InstallerBootCapacityTest(unittest.TestCase):
    def configure(self, root):
        result = run(SOURCE + STUBS, [root], cwd=REPO,
                     capture_output=True, text=True, timeout=10)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual((root / 'etc/modules-load.d/blent.conf').read_text(), 'evdi\nuinput\n')

    def test_t709_reinstall_preserves_capacity_and_unrelated_options_verbatim(self):
        for count in [1, 2, 3, 4]:
            with self.subTest(count=count), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                config = root / 'etc/modprobe.d/blent-evdi.conf'
                config.parent.mkdir(parents=True)
                before = f'# local configuration\noptions evdi initial_device_count={count} loglevel=3\nsoftdep evdi pre: amdgpu\n'
                config.write_text(before)
                self.configure(root)
                self.assertEqual(config.read_text(), before, 'T709: installer overwrote provisioned boot settings')

    def test_t709_new_install_retains_the_two_device_default(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.configure(root)
            self.assertEqual((root / 'etc/modprobe.d/blent-evdi.conf').read_text(),
                             'options evdi initial_device_count=2\n')

    def test_t709_existing_empty_or_unrecognized_configuration_is_not_replaced(self):
        for content in ['', '# intentionally managed elsewhere\n', 'options evdi initial_device_count=invalid\n']:
            with self.subTest(content=content), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                config = root / 'etc/modprobe.d/blent-evdi.conf'
                config.parent.mkdir(parents=True)
                config.write_text(content)
                self.configure(root)
                self.assertEqual(config.read_text(), content)

    def test_t709_dangling_configuration_link_is_preserved(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            config = root / 'etc/modprobe.d/blent-evdi.conf'
            config.parent.mkdir(parents=True)
            destination = root / 'absent-managed-config'
            config.symlink_to(destination)
            self.configure(root)
            self.assertTrue(config.is_symlink())
            self.assertFalse(destination.exists(), 'T709: installer followed a managed link')


if __name__ == '__main__':
    unittest.main()
