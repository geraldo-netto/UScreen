"""Installer refactors retain distro commands without changing the machine."""
from pathlib import Path
import mmap
import subprocess
import tempfile
import unittest
from shell_fixture import run as run_shell

REPO = Path(__file__).resolve().parents[2]
SOURCE = (REPO / 'scripts/install.sh').read_text().removesuffix('main "$@"\n')

DEPENDENCY_STUBS = r'''
PROJECT_DIR=$1
MOCK_SYSTEM=$2
sudo() {
    if [[ ${REFUSE_EVDI:-0} == 1 && $* == *libevdi* ]]; then return 1; fi
    printf 'installed: %s\n' "$*"
}
ffmpeg() { :; }
adb() { :; }
ls() { return 1; }
[() {
    if [[ $1 == -f && ( $2 == /usr/* || $2 == /lib/* || $2 == /lib64/* ) ]]; then
        builtin [ -f "$MOCK_SYSTEM$2" ]
    else builtin [ "$@"; fi
}
'''


class InstallerTest(unittest.TestCase):
    def test_t231_other_init_systems_receive_boot_module_guidance(self):
        output = self.run_installer(r'''
sudo() { if [[ $1 == tee ]]; then command cat >/dev/null; fi; }
[() {
    if [[ $* == '! -d /run/systemd/system ]' ]]; then return 0;
    else builtin [ "$@"; fi
}
configure_boot_modules
''')
        self.assertIn('Configure your init system to load evdi and uinput at boot', output)
        self.assertIn('modules-load.d support is not guaranteed', output)

    def rpm_fixture(self, distro, version='9.4', fedora_macro='%fedora', immutable=False):
        stubs = r'''
sudo() { printf 'packages: %s\n' "$*"; }
dnf() { return 1; }
rpm() { printf '%s\n' "$MOCK_FEDORA"; }
rpm-ostree() { :; }
pacman() { return 1; }
yay() { printf 'packages: yay %s\n' "$*"; }
paru() { printf 'packages: paru %s\n' "$*"; }
[() {
    if [[ $* == '-e /run/ostree-booted ]' ]]; then [[ $MOCK_IMMUTABLE == 1 ]];
    else builtin [ "$@"; fi
}
VERSION_ID=$2
MOCK_FEDORA=$3
MOCK_IMMUTABLE=$4
install_distro_deps "$1"
'''
        result = run_shell(SOURCE + stubs, [distro, version, fedora_macro, str(int(immutable))],
                           cwd=REPO, capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        return result.stdout

    def test_t230_enterprise_families_never_enable_fedora_repositories(self):
        for distro in ['rhel', 'centos rhel fedora', 'rocky rhel centos fedora', 'almalinux rhel centos fedora']:
            with self.subTest(distro=distro):
                output = self.rpm_fixture(distro)
                self.assertNotIn('/free/fedora/', output, 'T230: Fedora repository offered to Enterprise Linux')
                self.assertNotIn('%fedora', output)
                self.assertIn('9.4', output, 'T230: repository guidance omitted the actual release')
                self.assertIn('packages: dnf install', output)
                self.assertIn('ffmpeg android-tools', output)

    def test_t230_only_a_numeric_fedora_release_selects_its_repository(self):
        valid = self.rpm_fixture('fedora', '44', '44')
        self.assertIn('/free/fedora/rpmfusion-free-release-44.noarch.rpm', valid)
        invalid = self.rpm_fixture('fedora')
        self.assertNotIn('/free/fedora/rpmfusion-free-release-%fedora', invalid)
        immutable = self.rpm_fixture('fedora', '44', '44', True)
        self.assertIn('packages: rpm-ostree install', immutable)
        self.assertNotIn('/free/fedora/', immutable)

    def test_t230_unknown_ids_do_not_match_substrings_of_supported_families(self):
        for distro in ['unknown', 'notfedora', 'pinearch', 'notubuntu', 'notsuse']:
            with self.subTest(distro=distro):
                output = self.rpm_fixture(distro)
                self.assertIn('Unknown distro. Install manually:', output)
                self.assertNotIn('packages:', output)

    def dependency_fixture(self, root, prebuilt, library):
        project, system = root / 'project', root / 'system'
        project.mkdir()
        system.mkdir()
        if prebuilt:
            (project / 'bin').mkdir()
            (project / 'bin/blent').write_text('fixture')
        if library:
            base = project if library.startswith('bin/') else system
            path = base / library
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text('fixture library')
        return project, system

    def dependency_output(self, fixture, commands):
        result = run_shell(SOURCE + DEPENDENCY_STUBS + commands, fixture,
                           cwd=REPO, capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        return result.stdout

    def test_t229_missing_evdi_packages_do_not_block_runtime_tools(self):
        for prebuilt in [True, False]:
            with self.subTest(prebuilt=prebuilt), tempfile.TemporaryDirectory() as temp:
                fixture = self.dependency_fixture(Path(temp), prebuilt, None)
                output = self.dependency_output(fixture, 'REFUSE_EVDI=1\ninstall_debian_deps\n')
                installed = [line.split() for line in output.splitlines() if line.startswith('installed:')]
                self.assertTrue(any('ffmpeg' in args and 'adb' in args for args in installed),
                                'T229: missing libevdi prevented runtime tools installation')
                self.assertTrue(any('evdi-dkms' in args for args in installed))

    def test_t229_library_requirements_follow_bundle_or_source(self):
        cases = [
            (True, 'bin/libevdi.so.1', None, True),
            (True, 'usr/local/lib/libevdi.so.1', None, True),
            (True, None, 'libevdi1', False),
            (False, 'usr/local/lib/libevdi.so', None, True),
            (False, 'usr/local/lib/libevdi.so.1', 'libevdi-dev', False),
            (False, 'bin/libevdi.so.1', 'libevdi-dev', False),
            (False, None, 'libevdi-dev', False),
        ]
        for prebuilt, library, package, complete in cases:
            with self.subTest(prebuilt=prebuilt, library=library), tempfile.TemporaryDirectory() as temp:
                fixture = self.dependency_fixture(Path(temp), prebuilt, library)
                output = self.dependency_output(fixture, 'install_debian_deps\ncheck_deps\n')
                packages = [part for line in output.splitlines() if line.startswith('installed:')
                            for part in line.split() if part.startswith('libevdi')]
                self.assertEqual(packages, [] if package is None else [package],
                                 'T229: source and runtime library requirements were mixed')
                self.assertEqual('Dependencies look complete' in output, complete,
                                 'T229: library availability did not match the build/load route')

    def test_t345_installed_binaries_are_executable(self):
        with tempfile.TemporaryDirectory(prefix='blent-executable-install-') as tmp:
            source, installed = self.upgrade_fixture(Path(tmp))
            for name in ['blent', 'blent-gui', 'evdi_helper']:
                (source / name).chmod(0o644)
            result = self.install_fixture(source, installed)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            for name in ['blent', 'evdi_helper', 'blent-gui']:
                with self.subTest(binary=name):
                    launched = subprocess.run([str(installed / name)], capture_output=True, text=True)
                    self.assertEqual(launched.returncode, 0, launched.stdout + launched.stderr)

    def test_t249_upgrade_preserves_a_running_library_mapping(self):
        with tempfile.TemporaryDirectory(prefix='blent-upgrade-') as tmp:
            root = Path(tmp)
            project = root / 'release'
            source = project / 'bin'
            installed = root / 'installed bin'
            source.mkdir(parents=True)
            installed.mkdir()
            for name in ['blent', 'blent-gui', 'evdi_helper']:
                (source / name).write_text('#!/bin/sh\nexit 0\n')
            library = 'libevdi.so.1.15.0'
            (source / library).write_bytes(b'N' * 4096)
            (installed / library).write_bytes(b'O' * 4096)
            (source / 'libevdi.so.1').symlink_to(library)
            (installed / 'libevdi.so.1').symlink_to(library)
            with (installed / library).open('rb') as old:
                with mmap.mmap(old.fileno(), 0, access=mmap.ACCESS_READ) as active:
                    result = run_shell(SOURCE + '\nPROJECT_DIR=$1\nBIN_DIR=$2\ninstall_binaries\n',
                                       [project, installed], cwd=REPO, capture_output=True, text=True)
                    self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                    self.assertTrue(active[:] == b'O' * 4096,
                                    'T249: upgrade modified a running helper library mapping')
            self.assertEqual((installed / library).read_bytes(), b'N' * 4096)
            self.assertTrue((installed / 'libevdi.so.1').is_symlink())
            self.assertEqual((installed / 'libevdi.so.1').resolve(), installed / library)

    def test_t261_failed_upgrade_preserves_installed_executables(self):
        for failure in ['missing-helper', 'failed-copy']:
            with self.subTest(failure=failure), tempfile.TemporaryDirectory(prefix='blent-upgrade-') as tmp:
                source, installed = self.upgrade_fixture(Path(tmp))
                if failure == 'missing-helper':
                    (source / 'evdi_helper').unlink()
                stub = 'cp() { case "$1" in */blent-gui) return 77 ;; *) command cp "$@" ;; esac; }\n' if failure == 'failed-copy' else ''
                result = self.install_fixture(source, installed, stub)
                self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
                for name in ['blent', 'blent-gui', 'evdi_helper']:
                    self.assertEqual((installed / name).read_text(), 'old-' + name,
                                     'T261: failed staging replaced an installed executable')
                self.assertEqual(list(installed.glob('.blent-*')), [])

    def test_t261_omitted_optional_gui_keeps_existing_installation(self):
        with tempfile.TemporaryDirectory(prefix='blent-upgrade-') as tmp:
            source, installed = self.upgrade_fixture(Path(tmp))
            (source / 'blent-gui').unlink()
            result = self.install_fixture(source, installed)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            self.assertEqual((installed / 'blent-gui').read_text(), 'old-blent-gui')
            for name in ['blent', 'evdi_helper']:
                self.assertEqual((installed / name).read_text(), '#!/bin/sh\nexit 0\n')
            self.assertEqual(list(installed.glob('.blent-*')), [])

    def upgrade_fixture(self, root):
        source = root / 'release' / 'bin'
        installed = root / 'installed bin'
        source.mkdir(parents=True)
        installed.mkdir()
        for name in ['blent', 'blent-gui', 'evdi_helper']:
            (source / name).write_text('#!/bin/sh\nexit 0\n')
            (installed / name).write_text('old-' + name)
        return source, installed

    def install_fixture(self, source, installed, stub=''):
        return run_shell(SOURCE + '\nPROJECT_DIR=$1\nBIN_DIR=$2\n' + stub + 'install_binaries\n',
                         [source.parent, installed], cwd=REPO, capture_output=True, text=True)

    def run_installer(self, commands):
        result = run_shell(SOURCE + commands, capture_output=True, text=True, cwd=REPO)
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
    if [[ ${FAIL_DEBIAN:-0} == 1 && ( $* == *libevdi-dev* || $* == *'ffmpeg adb '* ) ]]; then return 1; fi
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
has_evdi_library() { return 1; }
'''
        families = {
            'fedora': ['ffmpeg android-tools'],
            'ubuntu debian': ['apt-get update', 'ffmpeg adb', 'apt-get install -y libevdi-dev', 'evdi-dkms'],
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
    case "$*" in *'ffmpeg adb '*|*libevdi-dev*) return 1 ;; esac
}
has_evdi_library() { return 1; }
install_debian_deps
''')
        self.assertIn('ffmpeg android-tools-adb', output)
        self.assertIn('apt-get install -y libevdi-dev', output)
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
