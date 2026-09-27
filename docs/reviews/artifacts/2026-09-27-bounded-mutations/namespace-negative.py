import sys, unittest
from pathlib import Path
from unittest.mock import patch
sys.path.insert(0, str(Path.cwd()/'scripts/tests'))
import test_bounded_mutation as suite
original = suite.isolation.command

def old_order(command, output):
    result = original(command, output)
    at = result.index('--bind')
    binding = result[at:at+3]
    del result[at:at+3]
    at = result.index('--tmpfs')
    return result[:at]+binding+result[at:]

with patch.object(suite.isolation, 'command', old_order):
    result = unittest.TextTestRunner(verbosity=2).run(unittest.TestSuite([
        suite.BoundedEvidenceTest('test_t652_namespace_protects_external_file_and_allows_private_output')]))
raise SystemExit(not result.wasSuccessful())
