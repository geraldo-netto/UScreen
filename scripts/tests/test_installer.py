"""Installer refactors retain distro commands without changing the machine."""
from pathlib import Path
import mmap
import subprocess
import tempfile
import unittest

REPO = Path(__file__).resolve().parents[2]
SOURCE = (REPO / 'scripts/install.sh').read_text().removesuffix('main "$@"\n')


class InstallerTest(unittest.TestCase):
    def test_t345_installed_binaries_are_executable(self):
        with tempfile.TemporaryDirectory(prefix='uscreen-executable-install-') as tmp:
            source, installed = self.upgrade_fixture(Path(tmp))
            for name in ['uscreen', 'uscreen-gui', 'evdi_helper']:
                (source / name).chmod(0o644)
            result = self.install_fixture(source, installed)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            for name in ['uscreen', 'evdi_helper', 'uscreen-gui']:
                with self.subTest(binary=name):
                    launched = subprocess.run([str(installed / name)], capture_output=True, text=True)
                    self.assertEqual(launched.returncode, 0, launched.stdout + launched.stderr)

    def test_t249_upgrade_preserves_a_running_library_mapping(self):
        with tempfile.TemporaryDirectory(prefix='uscreen-upgrade-') as tmp:
            root = Path(tmp)
            project = root / 'release'
            source = project / 'bin'
            installed = root / 'installed bin'
            source.mkdir(parents=True)
            installed.mkdir()
            for name in ['uscreen', 'uscreen-gui', 'evdi_helper']:
                (source / name).write_text('#!/bin/sh\nexit 0\n')
            library = 'libevdi.so.1.15.0'
            (source / library).write_bytes(b'N' * 4096)
            (installed / library).write_bytes(b'O' * 4096)
            (source / 'libevdi.so.1').symlink_to(library)
            (installed / 'libevdi.so.1').symlink_to(library)
            with (installed / library).open('rb') as old:
                with mmap.mmap(old.fileno(), 0, access=mmap.ACCESS_READ) as active:
                    result = subprocess.run(['bash', '-s', '--', str(project), str(installed)],
                        input=SOURCE + '\nPROJECT_DIR=$1\nBIN_DIR=$2\ninstall_binaries\n',
                        cwd=REPO, capture_output=True, text=True)
                    self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                    self.assertTrue(active[:] == b'O' * 4096,
                                    'T249: upgrade modified a running helper library mapping')
            self.assertEqual((installed / library).read_bytes(), b'N' * 4096)
            self.assertTrue((installed / 'libevdi.so.1').is_symlink())
            self.assertEqual((installed / 'libevdi.so.1').resolve(), installed / library)

    def test_t261_failed_upgrade_preserves_installed_executables(self):
        for failure in ['missing-helper', 'failed-copy']:
            with self.subTest(failure=failure), tempfile.TemporaryDirectory(prefix='uscreen-upgrade-') as tmp:
                source, installed = self.upgrade_fixture(Path(tmp))
                if failure == 'missing-helper':
                    (source / 'evdi_helper').unlink()
                stub = 'cp() { case "$1" in */uscreen-gui) return 77 ;; *) command cp "$@" ;; esac; }\n' if failure == 'failed-copy' else ''
                result = self.install_fixture(source, installed, stub)
                self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
                for name in ['uscreen', 'uscreen-gui', 'evdi_helper']:
                    self.assertEqual((installed / name).read_text(), 'old-' + name,
                                     'T261: failed staging replaced an installed executable')
                self.assertEqual(list(installed.glob('.uscreen-*')), [])

    def test_t261_omitted_optional_gui_keeps_existing_installation(self):
        with tempfile.TemporaryDirectory(prefix='uscreen-upgrade-') as tmp:
            source, installed = self.upgrade_fixture(Path(tmp))
            (source / 'uscreen-gui').unlink()
            result = self.install_fixture(source, installed)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            self.assertEqual((installed / 'uscreen-gui').read_text(), 'old-uscreen-gui')
            for name in ['uscreen', 'evdi_helper']:
                self.assertEqual((installed / name).read_text(), '#!/bin/sh\nexit 0\n')
            self.assertEqual(list(installed.glob('.uscreen-*')), [])

    def upgrade_fixture(self, root):
        source = root / 'release' / 'bin'
        installed = root / 'installed bin'
        source.mkdir(parents=True)
        installed.mkdir()
        for name in ['uscreen', 'uscreen-gui', 'evdi_helper']:
            (source / name).write_text('#!/bin/sh\nexit 0\n')
            (installed / name).write_text('old-' + name)
        return source, installed

    def install_fixture(self, source, installed, stub=''):
        return subprocess.run(['bash', '-s', '--', str(source.parent), str(installed)],
            input=SOURCE + '\nPROJECT_DIR=$1\nBIN_DIR=$2\n' + stub + 'install_binaries\n',
            cwd=REPO, capture_output=True, text=True)

    def run_installer(self, commands):
        result = subprocess.run(['bash', '-s'], input=SOURCE + commands,
                                capture_output=True, text=True, cwd=REPO)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        return result.stdout

    def test_t316_tarball_installs_gui_x11_runtime_libraries(self):
        # These libraries are loaded by winit/xkbcommon at runtime; ldd misses
        # them. T314's isolated GUI launch demonstrates the resulting failure.
        debian = ['libx11-6', 'libx11-xcb1', 'libxcursor1', 'libxi6', 'libxkbcommon-x11-0']
        fedora = ['libX11', 'libX11-xcb', 'libXcursor', 'libXi', 'libxkbcommon-x11']
        cases = [
            ('debian', '', debian),
            ('debian', 'FAIL_DEBIAN=1\n', debian),
            ('fedora', '', fedora),
            ('fedora', 'IMMUTABLE=1\n', fedora),
            ('arch', '', ['libx11', 'libxcursor', 'libxi', 'libxkbcommon-x11']),
            ('opensuse', '', ['libX11-6', 'libX11-xcb1', 'libXcursor1', 'libXi6', 'libxkbcommon-x11-0']),
        ]
        stubs = r'''
sudo() {
    if [[ ${FAIL_DEBIAN:-0} == 1 && $* == *libevdi-dev* ]]; then return 1; fi
    printf 'packages: %s\n' "$*"
}
pacman() { return 1; }
yay() { :; }
dnf() { return 0; }
rpm-ostree() { :; }
[() {
    if [[ $* == '-e /run/ostree-booted ]' ]]; then [[ ${IMMUTABLE:-0} == 1 ]];
    else builtin [ "$@"; fi
}
'''
        for distro, scenario, packages in cases:
            with self.subTest(distro=distro, scenario=scenario):
                output = self.run_installer(stubs + scenario + f'install_distro_deps "{distro}"\n')
                if scenario == 'IMMUTABLE=1\n':
                    self.assertIn('packages: rpm-ostree install', output)
                installed = [line.split() for line in output.splitlines() if line.startswith('packages:')]
                for package in packages:
                    self.assertTrue(any(package in command for command in installed),
                                    f'T316: {package} missing from successful installation commands: {output}')

    def test_t214_distro_dependencies_keep_commands_and_fallbacks(self):
        stubs = '''
sudo() { printf 'sudo %s\\n' "$*"; }
pacman() { return 1; }
yay() { printf 'yay %s\\n' "$*"; }
dnf() { return 1; }
rpm() { echo 42; }
'''
        families = {
            'fedora': ['ffmpeg android-tools'],
            'ubuntu debian': ['apt-get update', 'ffmpeg adb libevdi1 libevdi-dev', 'evdi-dkms'],
            'cachyos arch': ['pacman -S --needed --noconfirm ffmpeg android-tools', 'yay -S --needed --noconfirm evdi-dkms'],
            'opensuse': ['zypper --non-interactive install --no-recommends ffmpeg android-tools', 'evdi libevdi1'],
            'unknown': ['Unknown distro. Install manually:'],
        }
        for distro, expected in families.items():
            with self.subTest(distro=distro):
                output = self.run_installer(stubs + f'install_distro_deps "{distro}"\n')
                for command in expected:
                    self.assertIn(command, output)

    def test_t214_debian_fallback_keeps_kernel_install_independent(self):
        output = self.run_installer('''
sudo() {
    printf 'sudo %s\\n' "$*"
    case "$*" in *libevdi-dev*) return 1 ;; esac
}
install_debian_deps
''')
        self.assertIn('ffmpeg android-tools-adb libevdi1', output)
        self.assertIn('apt-get install -y evdi-dkms', output)

    def test_t216_system_setup_keeps_boot_configuration_and_live_fallback(self):
        output = self.run_installer(r'''
sudo() {
    printf 'sudo %s\n' "$*" >&3
    if [ "$1" = tee ]; then command cat >&3; fi
}
lsmod() { echo 'evdi 123 0'; }
cat() {
    if [ "$1" = /sys/devices/evdi/count ]; then echo 0;
    else command cat "$@"; fi
}
system_setup 3>&1 2>&1
''')
        for expected in ['mkdir -p /etc/modprobe.d /etc/modules-load.d',
                         'options evdi initial_device_count=2', 'evdi\nuinput',
                         'sudo modprobe uinput', 'sudo sh', '/setup-evdi.sh 2']:
            self.assertIn(expected, output)


if __name__ == '__main__':
    unittest.main()
