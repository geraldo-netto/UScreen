"""T497: native tracing preserves exit status and never records argument values."""
from dataclasses import asdict
import os
import json
import re
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

from inventory import syntax_functions
from model import fingerprint, result
from shell import merge, records


class ShellTest(unittest.TestCase):
    def test_t497_hook_functions_are_observable_without_recursively_tracing_themselves(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            hook = Path(__file__).with_name('trace.sh').resolve()
            bodies = re.findall(r'(?ms)^    blent_coverage_\w+\(\) \{.*?^    \}\n', hook.read_text())
            self.assertEqual(len(bodies), 2)
            fixture = root/'fixture.sh'
            fixture.write_text('set -eu\n' + ''.join(bodies) + HOOK_EXERCISE)
            traces = Path(os.environ.get('BLENT_SHELL_COVERAGE_DIR', root/'traces'))
            env = dict(os.environ, BASH_ENV=str(hook), BLENT_SHELL_COVERAGE_DIR=str(traces))
            measured = subprocess.run(['bash', fixture, root], env=env, capture_output=True)
            self.assertEqual(measured.returncode, 0, measured.stderr)
            self.assertEqual(measured.stdout, b'origin attestation failed\n')
            native = list(records(traces/('self-' + fingerprint(fixture) + '.bin')))
            self.assertTrue(native)
            lines = {number for digest, name, number in native if name == str(fixture)}
            functions = list(syntax_functions(Path(fixture.name), fixture.read_text(), 'shell'))
            for function in functions:
                self.assertGreater(len([line for line in lines if function.first <= line <= function.last]), 5)
            self.assertNotIn(b'private-argument-value', (traces/('self-' + fingerprint(fixture) + '.bin')).read_bytes())

    def test_t497_extracted_functions_count_only_when_the_complete_body_matches(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)/'repo'
            root.mkdir()
            source = root/'install.sh'
            body = 'choose() {\n  printf "yes\\n"\n}\n'
            source.write_text('# original prefix\n' + body + '# unexecuted entry point\n')
            functions = list(syntax_functions(Path('install.sh'), source.read_text(), 'shell'))
            manifest = dict(sources={'install.sh': fingerprint(source)}, functions=[asdict(f) for f in functions])
            manifest_path = Path(directory)/'manifest.json'
            manifest_path.write_text(json.dumps(manifest))
            for modified in [False, True]:
                trace = Path(directory)/str(modified)
                fragment = Path(directory)/'extracted.sh'
                fragment.write_text((body.replace('yes', 'changed') if modified else body) + 'choose\n')
                tools = Path(__file__).resolve().parent
                env = dict(os.environ, BASH_ENV=str(tools/'trace.sh'),
                           BLENT_SHELL_COVERAGE_DIR=str(trace), BLENT_COVERAGE_ROOT=str(root),
                           BLENT_COVERAGE_MANIFEST=str(manifest_path),
                           BLENT_SHELL_COVERAGE_PYTHON=sys.executable,
                           BLENT_SHELL_COVERAGE_ORIGINS=str(tools/'shell_origins.py'))
                # This is a different source manifest. Do not combine its
                # attester subprocesses with an enclosing project's counters.
                env.pop('COVERAGE_PROCESS_START', None)
                env.pop('BLENT_PYTHON_CALLS', None)
                process = subprocess.run(['bash', fragment], env=env, capture_output=True)
                self.assertEqual(process.returncode, 0, process.stderr)
                fragment.unlink()
                data = {}
                merge(root, trace, manifest, data)
                self.assertEqual(result(functions[0], data)['passes'], not modified)

    def test_t497_trace_preserves_status_and_weird_paths_without_logging_arguments(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            script = root/'literal $(touch escaped)\nfixture.sh'
            source = '''choose() {
  if [[ $1 -gt 0 ]]; then
    printf 'yes\\n'
  else
    printf 'no\\n'
  fi
}
choose 1
choose 0
false
printf 'status=%s private-argument-value\\n' "$?"
exit 7
'''
            script.write_text(source)
            baseline = subprocess.run(['bash', script], capture_output=True)
            traces = root/'traces'
            env = dict(os.environ, BLENT_SHELL_COVERAGE_DIR=str(traces),
                       BASH_ENV=str(Path(__file__).with_name('trace.sh').resolve()))
            measured = subprocess.run(['bash', script], env=env, capture_output=True)
            self.assertEqual((measured.returncode, measured.stdout, measured.stderr),
                             (baseline.returncode, baseline.stdout, baseline.stderr))
            self.assertFalse((root/'escaped').exists())
            all_records = []
            for path in traces.glob('*.bin'):
                self.assertNotIn(b'private-argument-value', path.read_bytes())
                all_records.extend(records(path))
            self.assertTrue(all_records)
            functions = list(syntax_functions(Path(script.name), source, 'shell'))
            manifest = dict(sources={script.name: fingerprint(script)}, functions=[asdict(fn) for fn in functions])
            data = {}
            merge(root, traces, manifest, data)
            self.assertTrue(result(functions[0], data)['passes'])

    def test_t497_corrupt_trace_records_fail_closed(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)/'trace.bin'
            for data in [b'incomplete', b'hash\0name\01\0', b'0'*64 + b'\0name\00\0']:
                path.write_bytes(data)
                with self.assertRaises(ValueError): list(records(path))


# Execute byte-identical hook bodies under an independent native DEBUG observer.
# A DEBUG handler cannot trace its own execution; tracing explicit calls here
# exercises its real branches without fabricating counters or expanded commands.
HOOK_EXERCISE = r'''
fixture_source=${BASH_SOURCE[0]}
fixture_hash=$(/usr/bin/sha256sum -- "$fixture_source")
fixture_hash=${fixture_hash%% *}
exec {fixture_fd}>>"$BLENT_SHELL_COVERAGE_DIR/self-$fixture_hash.bin"
trap 'printf "%s\0%s\0%s\0" "$fixture_hash" "${BASH_SOURCE[0]}" "$LINENO" >&"$fixture_fd"' DEBUG
BLENT_SHELL_COVERAGE_DIR=$1/private
mkdir -p "$BLENT_SHELL_COVERAGE_DIR/origins"
BLENT_SHELL_COVERAGE_ORIGINS=
blent_coverage_origin
BLENT_SHELL_COVERAGE_ORIGINS=/unused
BLENT_SHELL_COVERAGE_PYTHON=
blent_coverage_origin
BLENT_SHELL_COVERAGE_PYTHON=/bin/true
blent_coverage_hash=fixture
touch "$BLENT_SHELL_COVERAGE_DIR/origins/fixture.json"
blent_coverage_origin
rm "$BLENT_SHELL_COVERAGE_DIR/origins/fixture.json"
blent_coverage_origin
BLENT_SHELL_COVERAGE_PYTHON=/bin/false
blent_coverage_origin
[[ -f $BLENT_SHELL_COVERAGE_DIR/origins.failed ]]
cat "$BLENT_SHELL_COVERAGE_DIR/origins.failed"
BLENT_SHELL_COVERAGE_ORIGINS=
blent_coverage_hook=$fixture_source
blent_coverage_location
blent_coverage_hook=/never-this-fixture
blent_coverage_source=
blent_coverage_location private-argument-value
[[ $blent_coverage_source == "$fixture_source" ]]
[[ $blent_coverage_hash == "$fixture_hash" ]]
blent_coverage_location
trap - DEBUG
'''
