"""T269: every setup entry point preserves live EVDI devices (no kernel access)."""
from pathlib import Path
import json
import os
import subprocess
import tempfile
import unittest
from shell_fixture import run as run_shell

REPO = Path(__file__).resolve().parents[2]

MOCK = r'''#!/usr/bin/env python3
from pathlib import Path
import json, os, sys
root = Path(os.environ['BLENT_SETUP_FIXTURE'])
name, args = Path(sys.argv[0]).name, sys.argv[1:]
with (root / 'commands').open('a') as log:
    log.write(json.dumps([name] + args) + '\n')
count = root / 'count'
if name == 'sudo':
    os.execvp(args[0], args)
elif name == 'lsmod':
    if count.exists(): print('evdi 123 2')  # Active consumers, not just loaded.
elif name == 'cat' and args == ['/sys/devices/evdi/count']:
    if not count.exists(): sys.exit(1)
    print(count.read_text())
elif name == 'modprobe':
    if args == ['evdi'] and not count.exists():
        if os.environ.get('FAIL_LOAD'): sys.exit(1)
        count.write_text('0')
elif name == 'tee' and args == ['/sys/devices/evdi/add']:
    value = sys.stdin.read().strip()
    if os.environ.get('FAIL_ADD'): sys.exit(1)
    if os.environ.get('INVALID_AFTER_ADD'): count.write_text('invalid')
    elif not os.environ.get('IGNORE_ADD'): count.write_text(str(int(count.read_text()) + int(value)))
elif name not in ['touch', 'udevadm', 'gtk-update-icon-cache']:
    raise RuntimeError('Unexpected fixture command: ' + repr([name] + args))
'''


def setup_program(entry):
    if entry == 'script':
        return (REPO / 'scripts/setup-evdi.sh').read_text(), ['2']
    if entry == 'installer':
        source = (REPO / 'scripts/install.sh').read_text().removesuffix('main "$@"\n')
        return source + '\nSCRIPT_DIR=$1\nactivate_evdi\n', [str(REPO / 'scripts')]
    if entry == 'deb':
        return (REPO / 'packaging/deb/postinst').read_text(), ['configure', '1.2.2']
    if entry == 'rpm':
        source = (REPO / 'packaging/rpm/blent.spec').read_text()
        return source.split('\n%post\n')[1].split('\n%files\n')[0], ['2']
    source = (REPO / 'packaging/arch/blent.install').read_text()
    return source + '\n' + entry + '\n', []


class EvdiSetupTest(unittest.TestCase):
    def run_setup(self, entry, count, failure=''):
        with tempfile.TemporaryDirectory(prefix='blent-evdi-setup-') as tmp:
            root = Path(tmp)
            (root / 'bin').mkdir()
            if count is not None:
                (root / 'count').write_text(str(count))
            for name in ['sudo', 'lsmod', 'modprobe', 'cat', 'tee', 'touch',
                         'udevadm', 'gtk-update-icon-cache']:
                program = root / 'bin' / name
                program.write_text(MOCK)
                program.chmod(0o755)
            source, args = setup_program(entry)
            # Map the installed, architecture-independent script into the source fixture.
            source = source.replace('%{_datadir}', '/usr/share')
            source = '''sh() {
    [[ $1 == /usr/share/blent/setup-evdi.sh ]] || return 91
    command sh "$BLENT_SETUP_SCRIPT" "${@:2}"
}
''' + source
            env = dict(os.environ, PATH=f'{root}/bin:{os.environ["PATH"]}', BLENT_SETUP_FIXTURE=tmp,
                       BLENT_SETUP_SCRIPT=str(REPO / 'scripts/setup-evdi.sh'))
            if failure:
                env[failure] = '1'
            result = run_shell(source, args, env=env, cwd=REPO, capture_output=True, text=True, timeout=5)
            trace = [json.loads(row) for row in (root / 'commands').read_text().splitlines()]
            final = (root / 'count').read_text() if (root / 'count').exists() else None
            return result, trace, final

    def test_t269_install_and_upgrade_never_unload_and_add_only_missing_capacity(self):
        for entry in ['script', 'installer', 'deb', 'rpm', 'post_install', 'post_upgrade']:
            for count in [None, 0, 1, 2, 4]:
                with self.subTest(entry=entry, existing=count):
                    result, trace, final = self.run_setup(entry, count)
                    self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                    self.assertNotIn(['modprobe', '-r', 'evdi'], trace, 'T269: live device removed')
                    self.assertEqual(int(final), max(count or 0, 2), 'T269: required capacity missing')
                    if count is not None and count >= 2:
                        self.assertNotIn(['tee', '/sys/devices/evdi/add'], trace)

    def test_t269_setup_failure_defers_to_reboot_without_unloading(self):
        for entry in ['script', 'installer', 'deb', 'rpm', 'post_install', 'post_upgrade']:
            for count, failure in [(None, 'FAIL_LOAD'), (0, 'FAIL_ADD'), ('invalid', '')]:
                with self.subTest(entry=entry, existing=count, failure=failure):
                    result, trace, final = self.run_setup(entry, count, failure)
                    self.assertNotIn(['modprobe', '-r', 'evdi'], trace)
                    self.assertEqual(final, None if count is None else str(count))
                    self.assertIn('reboot', (result.stdout + result.stderr).lower())

    def test_t497_capacity_is_confirmed_after_a_successful_sysfs_write(self):
        for failure, final in [('INVALID_AFTER_ADD', 'invalid'), ('IGNORE_ADD', '0')]:
            with self.subTest(failure=failure):
                result, trace, actual = self.run_setup('script', 0, failure)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(actual, final)
                self.assertIn(['tee', '/sys/devices/evdi/add'], trace)
                self.assertIn('reboot', result.stderr)

    def test_t497_invalid_capacity_never_runs_system_commands(self):
        for value in ['0', '5', '-1', '2.0', '4294967295', 'invalid']:
            with self.subTest(value=value):
                result = run_shell((REPO / 'scripts/setup-evdi.sh').read_text(), [value],
                                   env=dict(os.environ, PATH='/no/commands'), executable='/bin/bash',
                                   capture_output=True, text=True)
                self.assertEqual(result.returncode, 2)
                self.assertIn('Usage:', result.stderr)


if __name__ == '__main__':
    unittest.main()
