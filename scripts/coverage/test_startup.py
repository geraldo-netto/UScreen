"""T497: inactive hooks have no effect and measurement failures cannot pass silently."""
import contextlib
import io
import os
import unittest
from unittest.mock import patch

from startup import start


class StartupTest(unittest.TestCase):
    def test_t497_hook_starts_both_observers_only_when_configured(self):
        with patch.dict(os.environ, {}, clear=True), patch('coverage.process_startup') as lines, \
                patch('calls.start') as calls:
            start()
            lines.assert_not_called()
            calls.assert_not_called()
            with patch.dict(os.environ, USCREEN_PYTHON_CALLS='fixture'):
                start()
            lines.assert_called_once_with()
            calls.assert_called_once_with()

    def test_t497_failed_line_collection_aborts_before_invocation_collection(self):
        error = io.StringIO()
        with patch.dict(os.environ, USCREEN_PYTHON_CALLS='fixture'), contextlib.redirect_stderr(error), \
                patch('coverage.process_startup', side_effect=ValueError('broken manifest')), \
                patch('calls.start') as calls, patch('startup.os._exit', side_effect=SystemExit(86)):
            with self.assertRaises(SystemExit) as result: start()
            self.assertEqual(result.exception.code, 86)
            calls.assert_not_called()
        self.assertIn('broken manifest', error.getvalue())
