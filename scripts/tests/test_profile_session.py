"""T624: temporary APK lifecycle cannot replace or remove production data."""
import importlib.util
from pathlib import Path
import signal
import subprocess
import unittest
from unittest.mock import Mock, patch

PATH = Path(__file__).resolve().parents[1] / 'benchmarks/profile-session.py'
SPEC = importlib.util.spec_from_file_location('profile_session', PATH)
SESSION = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(SESSION)
PROFILE = 'io.github.geraldo_netto.blent.profile'
BADGING = f"package: name='{PROFILE}' versionCode='12'\n"


class ProfileSessionTests(unittest.TestCase):
    def test_t624_rejects_production_unknown_and_launcher_apks(self):
        for package, report in [
            ('io.github.geraldo_netto.blent', BADGING), ('other.app', BADGING),
            (PROFILE, BADGING.replace(PROFILE, 'io.github.geraldo_netto.blent')),
            (PROFILE, BADGING + "launchable-activity: name='com.blent.MainActivity'\n"),
            (PROFILE, BADGING * 2), (PROFILE, ''),
        ]:
            with self.subTest(package=package, report=report), self.assertRaises(ValueError):
                SESSION.validate_apk(package, report)
        SESSION.validate_apk(PROFILE, BADGING)

    def test_t624_cleanup_after_success_failure_and_interruption(self):
        for failure in [None, RuntimeError('failed workload'), KeyboardInterrupt(), SystemExit(143)]:
            with self.subTest(failure=failure):
                adb = Mock(return_value='')
                workload = Mock(side_effect=failure)
                try:
                    SESSION.session(adb, PROFILE, 'test.apk', workload)
                except BaseException as error:
                    self.assertIs(error, failure)
                self.assertEqual(adb.call_args_list[-1].args, ('uninstall', PROFILE))
                self.assertEqual(adb.call_args_list[1].args, ('install', 'test.apk'))
                self.assertFalse(any('io.github.geraldo_netto.blent' in call.args for call in adb.call_args_list))
                workload.assert_called_once_with()

    def test_t624_does_not_claim_existing_package_or_production(self):
        for package in [PROFILE, 'io.github.geraldo_netto.blent']:
            adb, workload = Mock(return_value='package:' + package), Mock()
            with self.assertRaises(ValueError):
                SESSION.session(adb, package, 'test.apk', workload)
            self.assertFalse(any(call.args[0] in ('install', 'uninstall') for call in adb.call_args_list))
            workload.assert_not_called()

    def test_t624_failed_install_still_cleans_partial_install(self):
        adb = Mock(side_effect=['', subprocess.CalledProcessError(1, 'adb'), 'Success'])
        workload = Mock()
        with self.assertRaises(subprocess.CalledProcessError):
            SESSION.session(adb, PROFILE, 'test.apk', workload)
        self.assertEqual(adb.call_args_list[-1].args, ('uninstall', PROFILE))
        workload.assert_not_called()

    def test_t624_signal_handler_raises_and_restores_handlers(self):
        original = signal.getsignal(signal.SIGTERM)
        with self.assertRaises(SystemExit):
            with SESSION.interruptible():
                signal.getsignal(signal.SIGTERM)(signal.SIGTERM, None)
        self.assertIs(signal.getsignal(signal.SIGTERM), original)

    def test_t624_interruption_reaps_workload_before_uninstall(self):
        child = Mock()
        child.wait.side_effect = [KeyboardInterrupt(), 0]
        with patch.object(SESSION.subprocess, 'Popen', return_value=child):
            with self.assertRaises(KeyboardInterrupt):
                SESSION.run_workload(['test'])
        child.terminate.assert_called_once_with()
        self.assertEqual(child.wait.call_count, 2)


if __name__ == '__main__':
    unittest.main()
