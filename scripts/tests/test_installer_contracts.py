"""T497: installer fallbacks and dispatch without changing host configuration."""
import os
from pathlib import Path
import tempfile
import unittest

from shell_fixture import run
from test_installer import SOURCE, REPO


class InstallerContractsTest(unittest.TestCase):
    def execute(self, commands, args=()):
        with tempfile.TemporaryDirectory() as directory:
            environment = dict(os.environ, HOME=directory, XDG_CONFIG_HOME=directory + '/config',
                               XDG_DATA_HOME=directory + '/data')
            result = run(SOURCE + commands, args, cwd=REPO, env=environment,
                         capture_output=True, text=True, timeout=10)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            return result.stdout

    def test_t497_failed_runtime_packages_provide_family_specific_guidance(self):
        output = self.execute('''
sudo() { echo "rejected: $*"; return 1; }
modinfo() { return 1; }
has_evdi_library() { return 0; }
install_rpm_layered_deps
install_rpm_runtime_deps --allowerasing
check_rpm_evdi_module
# apt update succeeds, but both runtime spellings and DKMS fail.
sudo() { echo "rejected: $*"; [[ $* == 'apt-get update' ]]; }
install_debian_deps
install_suse_deps
''')
        for message in ['Layering failed', 'Install runtime packages', 'No evdi module',
                        'ffmpeg android-tools-adb', 'Check the package names', 'evdi-dkms did not install',
                        'Install ffmpeg, android-tools', 'evdi did not install']:
            self.assertIn(message, output)

    def test_t497_arch_selects_installed_yay_paru_or_manual_guidance(self):
        stubs = '''
sudo() { echo "package: $*"; }
pacman() { [[ $T497_ARCH == installed ]]; }
command() {
    if [[ $* == '-v yay' ]]; then [[ $T497_ARCH == yay ]];
    elif [[ $* == '-v paru' ]]; then [[ $T497_ARCH == paru ]];
    else builtin command "$@"; fi
}
yay() { echo "yay: $*"; }
paru() { echo "paru: $*"; }
T497_ARCH=$1
install_arch_deps
'''
        for route, expected in [('installed', 'evdi already installed'), ('yay', 'yay: -S'),
                                ('paru', 'paru: -S'), ('missing', 'evdi lives in the AUR')]:
            with self.subTest(route=route):
                output = self.execute(stubs, [route])
                self.assertIn(expected, output)
                self.assertIn('package: pacman -S', output)

    def test_t497_distro_detection_passes_complete_tokens(self):
        output = self.execute('''
install_distro_deps() { printf 'detected=<%s>\\n' "$1"; }
install_deps
. /etc/os-release
printf 'expected=<%s %s>\\n' "${ID:-unknown}" "${ID_LIKE:-}"
''')
        self.assertEqual(output.splitlines()[0].replace('detected=', ''),
                         output.splitlines()[1].replace('expected=', ''))

    def test_t497_missing_path_warns_without_modifying_startup_files(self):
        output = self.execute('''
BIN_DIR="$HOME/new-bin"
check_path
PATH="$BIN_DIR:$PATH"
check_path
[[ ! -e $HOME/.bashrc ]]
''')
        self.assertEqual(output.count('is not in your PATH'), 1)
        self.assertIn('export PATH=', output)

    def test_t497_failed_service_staging_preserves_existing_unit(self):
        output = self.execute('''
mkdir -p "$CONFIG_BASE/systemd/user"
echo original > "$CONFIG_BASE/systemd/user/uscreen.service"
write_user_service() { echo partial; return 7; }
if write_installed_user_service; then exit 8; fi
[[ $(cat "$CONFIG_BASE/systemd/user/uscreen.service") == original ]]
shopt -s nullglob
staged=("$CONFIG_BASE/systemd/user/".uscreen.*)
[[ ${#staged[@]} == 0 ]]
echo preserved
''')
        self.assertIn('preserved', output)

    def test_t497_boot_configuration_errors_are_reported_without_aborting(self):
        output = self.execute('''
sudo() { if [[ $1 == tee ]]; then command cat >/dev/null; return 9; fi; }
configure_boot_modules
''')
        self.assertIn('Could not write /etc/modprobe.d/uscreen-evdi.conf', output)
        self.assertIn('Could not write /etc/modules-load.d/uscreen.conf', output)

    def test_t497_uinput_missing_rule_and_manager_failures_remain_actionable(self):
        stubs = '''
sudo() { printf 'request: %s\\n' "$*" >&3; [[ $1 != udevadm ]]; }
[() {
    if [[ $1 == '!' && $2 == -e && $3 == */60-uscreen-uinput.rules ]]; then return 0;
    else builtin [ "$@"; fi
}
PROJECT_DIR=$1
configure_uinput 3>&1
'''
        installed = self.execute(stubs, [REPO])
        self.assertIn('request: install -Dm644', installed)
        self.assertIn('Reload the uinput rule', installed)
        self.assertIn('Activate the uinput permissions', installed)
        missing = self.execute(stubs, ['/no/fixture/project'])
        self.assertIn('not found', missing)
        self.assertNotIn('request: install', missing)

    def test_t497_full_install_dispatch_preserves_order(self):
        output = self.execute('''
install_deps() { echo step:dependencies; }
check_deps() { echo step:check; }
build_if_needed() { echo step:build; }
install_files() { echo step:files; }
system_setup() { echo step:system; }
check_path() { echo step:path; }
main
''')
        self.assertEqual([line for line in output.splitlines() if line.startswith('step:')],
                         ['step:dependencies', 'step:check', 'step:build', 'step:files', 'step:system', 'step:path'])
        self.assertIn('Launch', output)


if __name__ == '__main__':
    unittest.main()
