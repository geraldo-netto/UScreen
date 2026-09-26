"""T606: replay archived launch statements without exposing tokens in argv."""
import ast
import gzip
from pathlib import Path
import subprocess
import unittest
from unittest.mock import Mock

ROOT = Path(__file__).resolve().parents[2]
RECIPES = ROOT / 'docs/reviews/artifacts/2026-09-26-followup/t598'


class ProfilingRecipeTokenTests(unittest.TestCase):
    def test_t606_launch_transports_token_only_on_stdin(self):
        token = 'b7' * 32
        for name in ['profile.py', 'scheduler.py']:
            with self.subTest(recipe=name):
                source = ast.parse(gzip.decompress((RECIPES / (name + '.gz')).read_bytes()))
                fake_path = Mock()
                fake_path.Path.return_value.read_text.return_value = token
                fake_process = Mock(spec=subprocess)
                fake_process.run.return_value.stdout = b'ok'
                scope = dict(s=fake_process, pathlib=fake_path,
                             base=['adb', '-s', 'test-tablet'], package='test.package')
                helpers = [n for n in source.body if isinstance(n, ast.FunctionDef) and n.name in ['adb', 'shell']]
                launch = next(n for n in source.body if isinstance(n, ast.Try))
                statements = []
                for node in launch.body:
                    if isinstance(node, ast.Expr) and ast.unparse(node).startswith('time.sleep('):
                        break
                    statements.append(node)
                module = ast.fix_missing_locations(ast.Module(body=helpers + statements, type_ignores=[]))
                exec(compile(module, name, 'exec'), scope)
                call = fake_process.run.call_args
                self.assertIsNotNone(call)
                self.assertNotIn(token, repr(call.args), 'T606: token leaked into host process argv')
                self.assertIn(token.encode(), call.kwargs.get('input', b''))
                self.assertIn(b'TokenActivity', call.kwargs['input'])
