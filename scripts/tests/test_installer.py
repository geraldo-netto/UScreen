"""Installer refactors retain distro commands without changing the machine."""
from pathlib import Path
import subprocess
import unittest

REPO = Path(__file__).resolve().parents[2]
SOURCE = (REPO / 'scripts/install.sh').read_text().removesuffix('main "$@"\n')


class InstallerTest(unittest.TestCase):
    def run_installer(self, commands):
        result = subprocess.run(['bash', '-s'], input=SOURCE + commands,
                                capture_output=True, text=True, cwd=REPO)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        return result.stdout

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


if __name__ == '__main__':
    unittest.main()
