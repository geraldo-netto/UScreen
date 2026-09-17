"""T302: distribution regressions must survive a normal release version bump."""
from contextlib import ExitStack
from pathlib import Path
import re
import tempfile
import unittest
from unittest.mock import patch

import test_distribution
import test_notices
import test_packages


class VersionFixtureTest(unittest.TestCase):
    def test_t302_next_release_keeps_distribution_coverage(self):
        with tempfile.TemporaryDirectory(prefix='uscreen-next-release-') as tmp:
            root = Path(tmp)
            test_notices.NoticeTest().copy_sources(root)
            for name, key in [('Makefile', 'VERSION'), ('packaging/arch/PKGBUILD', 'pkgver')]:
                path = root / name
                updated, count = re.subn(rf'(?m)^({key}\s*=\s*)\S+',
                                         lambda match: match[1] + '9.8.7', path.read_text())
                self.assertEqual(count, 1)
                path.write_text(updated)
            cases = [
                test_distribution.DistributionTest('test_t102_apk_failure_and_bundled_helper_loading'),
                test_notices.NoticeTest('test_t129_tar_deb_rpm_and_arch_include_notices'),
                test_packages.PackageTest('test_t235_portable_build_uses_literal_checkout_path'),
                test_packages.PackageTest('test_t236_release_rejects_new_glibc_in_bundled_evdi'),
            ]
            with ExitStack() as stack:
                for module in [test_notices, test_packages]:
                    stack.enter_context(patch.object(module, 'REPO', root))
                result = unittest.TestResult()
                unittest.TestSuite(cases).run(result)
            self.assertEqual(result.testsRun, len(cases))
            self.assertFalse(result.errors + result.failures,
                             '\n'.join(trace for _, trace in result.errors + result.failures))


if __name__ == '__main__':
    unittest.main()
