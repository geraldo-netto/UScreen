"""Keep isolated shell bodies on disk so native tracing retains their origins."""
from pathlib import Path
import re
import subprocess
import tempfile


def run(source, args=(), **options):
    with tempfile.TemporaryDirectory(prefix='blent-shell-fixture-') as name:
        script = Path(name)/'fixture.sh'
        script.write_text(re.sub(r'(?ms)^\[\(\) \{.*?^\}', test_override, source))
        return subprocess.run(['bash', str(script), *map(str, args)], **options)


def test_override(match):
    # Bash accepts [() but tree-sitter cannot parse that function name. Keep
    # the override semantics while leaving copied production bodies unchanged.
    return match.group().replace('[()', 'fixture_test()', 1) + '''
fixture_definition=$(declare -f fixture_test)
eval "${fixture_definition/fixture_test/[}"
'''
