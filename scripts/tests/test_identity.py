#!/usr/bin/env python3
"""T585: fresh Blent identities retain inherited license notices."""
import hashlib
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[2]


class BlentIdentityTests(unittest.TestCase):
    def test_t585_original_notices_remain_byte_identical(self):
        expected = {
            'LICENSE': 'a56b3410502afc790faa75e4d430147bd35a3dda330129efd9ac4a798c6d308c',
            'host/evdi/evdi_lib.h': 'ece987245ac3939fe5f12fca894a29c7d586bdbf80796d3f6fa08a7c52bf9128',
            'licenses/libevdi-LGPL-2.1.txt': '592987e8510228d546540b84a22444bde98e48d03078d3b2eefcd889bec5ce8c',
        }
        for name, digest in expected.items():
            with self.subTest(name=name):
                self.assertEqual(hashlib.sha256((ROOT / name).read_bytes()).hexdigest(), digest)

    def test_t585_android_host_and_verifier_agree(self):
        for name in ['android/app/build.gradle.kts', 'common/src/android.rs',
                     'scripts/verify-release-apk.py']:
            with self.subTest(name=name):
                self.assertIn('io.github.geraldo_netto.blent', (ROOT / name).read_text())
        self.assertIn('com.blent.MainActivity',
                      (ROOT / 'scripts/verify-release-apk.py').read_text())

    def test_t585_linux_entrypoints_use_new_identity(self):
        for directory, package in [('host', 'blent'), ('gui', 'blent-gui'),
                                   ('common', 'blent-config')]:
            self.assertIn(f'name = "{package}"', (ROOT / directory / 'Cargo.toml').read_text())
        self.assertIn('/usr/bin/blent start', (ROOT / 'packaging/blent.service').read_text())
        self.assertIn('Name=Blent', (ROOT / 'scripts/blent.desktop').read_text())

    def test_t585_android_display_name_and_namespace(self):
        self.assertIn('android:label="Blent Display"',
                      (ROOT / 'android/app/src/main/AndroidManifest.xml').read_text())
        activity = ROOT / 'android/app/src/main/java/com/blent/MainActivity.kt'
        self.assertIn('package com.blent', activity.read_text())

    def test_t585_current_readme_retains_author_without_original_repo_link(self):
        text = (ROOT / 'README.md').read_text()
        self.assertTrue(text.startswith('# Blent'))
        self.assertIn('majmichu1', text)
        self.assertNotIn('github.com/majmichu1/', text)


if __name__ == '__main__':
    unittest.main()
