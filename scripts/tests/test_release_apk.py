"""T250: official identity gate, malformed reports and SDK discovery, offline."""
import contextlib
import importlib.util
import io
import os
from pathlib import Path
import random
import subprocess
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location('release_apk', ROOT / 'scripts/verify-release-apk.py')
APK = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(APK)
DIGEST = '1b34ed115e476f4d178b49f6076cf9ed6cc07d474f9230ec952bc97bbba70400'
SIGNER = 'Signer #1 certificate SHA-256 digest: ' + DIGEST + '\n'
MANIFEST = "package: name='io.github.geraldo_netto.uscreen' versionCode='12'\nlaunchable-activity: name='com.uscreen.MainActivity'\n"


class ReleaseApkTest(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)

    def test_t250_designated_public_certificate_and_invalid_pem(self):
        self.assertEqual(APK.certificate_digest(ROOT / 'docs/release-certificate.pem'), DIGEST)
        path = self.root / 'cert'
        for text in ('', 'private key', '-----BEGIN CERTIFICATE-----\n=\n-----END CERTIFICATE-----',
                     '-----BEGIN CERTIFICATE-----\n \n-----END CERTIFICATE-----', 'x' * 65537):
            path.write_text(text)
            with self.assertRaises(ValueError):
                APK.certificate_digest(path)

    def test_t250_signer_rejects_absent_duplicate_wrong_and_malformed(self):
        APK.verify_signer(SIGNER, DIGEST)
        APK.verify_signer(SIGNER.replace(DIGEST, DIGEST.upper()), DIGEST)
        for report in ('', SIGNER * 2, SIGNER.replace(DIGEST, '0' * 64),
                       SIGNER.replace(DIGEST, DIGEST[:-1]), SIGNER.replace(DIGEST, DIGEST + '0'),
                       'x' * (APK.MAX_OUTPUT + 1)):
            with self.assertRaises(ValueError):
                APK.verify_signer(report, DIGEST)

    def test_t250_manifest_rejects_wrong_package_class_duplicate_and_debug(self):
        APK.verify_manifest(MANIFEST)
        for report in ('', MANIFEST * 2, MANIFEST.replace(APK.PACKAGE, 'com.uscreen'),
                       MANIFEST.replace('com.uscreen.MainActivity', APK.PACKAGE + '.MainActivity'),
                       MANIFEST + 'application-debuggable\n', 'x' * (APK.MAX_OUTPUT + 1)):
            with self.assertRaises(ValueError):
                APK.verify_manifest(report)

    def test_t250_seeded_invalid_reports_never_verify(self):
        rng = random.Random(250)
        for _ in range(512):
            malformed = ''.join(chr(rng.randrange(32, 127)) for _ in range(rng.randrange(129)))
            with self.assertRaises(ValueError):
                APK.verify_signer('Signer #1 certificate SHA-256 digest: ' + malformed, DIGEST)
            with self.assertRaises(ValueError):
                APK.verify_manifest("package: name='" + malformed + "'\n")

    def test_t250_sdk_environment_local_properties_and_missing(self):
        with patch.dict(os.environ, {}, clear=True), patch.object(APK, 'ROOT', self.root):
            with self.assertRaises(ValueError):
                APK.sdk_root()
            (self.root / 'android').mkdir()
            props = self.root / 'android/local.properties'
            props.write_text('# local SDK\nsdk.dir=/sdk path\n')
            self.assertEqual(APK.sdk_root(), Path('/sdk path'))
            for key in ('ANDROID_HOME', 'ANDROID_SDK_ROOT'):
                with patch.dict(os.environ, {key: '/override'}):
                    self.assertEqual(APK.sdk_root(), Path('/override'))
            props.write_text('# no SDK\n')
            with self.assertRaises(ValueError):
                APK.sdk_root()

    def test_t250_build_tools_find_path_or_latest_stable_sdk(self):
        with patch.object(APK.shutil, 'which', return_value='/path/apksigner'):
            self.assertEqual(APK.build_tool('apksigner'), '/path/apksigner')
        with patch.object(APK.shutil, 'which', return_value=None), patch.object(APK, 'sdk_root', return_value=self.root):
            with self.assertRaises(ValueError):
                APK.build_tool('apksigner')
            for version in ('9.0.0', '34.0.0', '36.0.0-rc1'):
                path = self.root / 'build-tools' / version / 'apksigner'
                path.parent.mkdir(parents=True)
                path.touch()
            self.assertEqual(APK.build_tool('apksigner'), str(self.root / 'build-tools/34.0.0/apksigner'))

    def test_t250_external_tools_must_succeed_and_output_is_bounded(self):
        with patch.object(APK.subprocess, 'run', return_value=subprocess.CompletedProcess([], 0, SIGNER)) as run:
            self.assertEqual(APK.output(['signer', 'verify']), SIGNER)
            self.assertTrue(run.call_args.kwargs['check'])
            run.return_value.stdout = 'x' * (APK.MAX_OUTPUT + 1)
            with self.assertRaises(ValueError):
                APK.output(['signer'])
        for error in (subprocess.TimeoutExpired('signer', 60), subprocess.CalledProcessError(1, 'signer')):
            with patch.object(APK.subprocess, 'run', side_effect=error), self.assertRaises(subprocess.SubprocessError):
                APK.output(['signer'])

    def test_t250_verification_orders_certificate_then_manifest(self):
        with patch.object(APK, 'build_tool', side_effect=lambda name: name), patch.object(APK, 'output', side_effect=[SIGNER, MANIFEST]) as output:
            self.assertEqual(APK.verify(Path('release space.apk')), DIGEST)
            self.assertEqual(output.call_args_list[0].args[0][-1], 'release space.apk')
            self.assertEqual(output.call_args_list[1].args[0][:3], ['aapt2', 'dump', 'badging'])

    def test_t250_cli_success_and_failure_status(self):
        with patch('sys.argv', ['verify', 'release.apk']), contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
            with patch.object(APK, 'verify', return_value=DIGEST):
                APK.main()
            with patch.object(APK, 'verify', side_effect=ValueError('wrong key')), self.assertRaises(SystemExit) as exit:
                APK.main()
            self.assertEqual(exit.exception.code, 1)


if __name__ == '__main__':
    unittest.main()
