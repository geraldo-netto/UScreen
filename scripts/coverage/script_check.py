#!/usr/bin/env python3
"""Collect Python and Bash from the normal isolated tooling regressions (T497)."""
import argparse
import configparser
from dataclasses import asdict
import json
import os
from pathlib import Path
import subprocess
import sys

import coverage
from calls import read_calls
from inventory import essential_script, owned_functions
from model import fingerprint, verify_sources
from readers import python_json
from report import collect, sources, summary
from shell import merge

ROOT = Path(__file__).resolve().parents[2]


def script_sources(root):
    return [path for path in sources(root) if essential_script(path)
            or path.as_posix() == 'host/tests/tooling.rs']


def snapshot(root):
    return dict(version=1, scope='Python and shell',
                sources={str(path): fingerprint(root/path) for path in script_sources(root)},
                functions=[asdict(f) for f in owned_functions(root) if essential_script(Path(f.file))])


def validate_snapshot(root, manifest):
    if manifest['version'] != 1 or set(manifest['sources']) != {str(p) for p in script_sources(root)}:
        raise ValueError('script coverage source inventory changed')
    verify_sources(root, manifest['sources'])


def configuration(directory, root):
    config = configparser.ConfigParser(interpolation=None)
    config['run'] = dict(parallel='true', plugins='copies',
                         source=str(root), data_file=str(directory/'.coverage'))
    path = directory/'coverage.ini'
    with path.open('w') as stream:
        config.write(stream)
    return path


def environment(directory, root):
    hook = directory/'hook'
    hook.mkdir()
    tools = Path(__file__).resolve().parent
    # Append tool imports; do not replace the tested program's own module search.
    (hook/'sitecustomize.py').write_text(
        f'import sys\nsys.path.append({str(tools)!r})\nfrom startup import start\nstart()\n')
    # Coverage's installed .pth startup can precede sitecustomize; its plugin
    # must already be importable at that point, including in system Python.
    paths = [str(hook), str(tools), str(Path(coverage.__file__).resolve().parent.parent)]
    paths.extend(filter(None, os.environ.get('PYTHONPATH', '').split(os.pathsep)))
    return dict(os.environ, PYTHONPATH=os.pathsep.join(paths), PYTHONNOUSERSITE='1',
                COVERAGE_PROCESS_START=str(configuration(directory, root)),
                USCREEN_COVERAGE_ROOT=str(root), USCREEN_COVERAGE_MANIFEST=str(directory/'manifest.json'),
                USCREEN_PYTHON_CALLS=str(directory/'calls'),
                USCREEN_SHELL_COVERAGE_PYTHON=sys.executable,
                USCREEN_SHELL_COVERAGE_ORIGINS=str(tools/'shell_origins.py'),
                BASH_ENV=str(tools/'trace.sh'), USCREEN_SHELL_COVERAGE_DIR=str(directory/'shell'))


def commands(root):
    yield ['cargo', 'test', '--locked', '-p', 'uscreen', '--test', 'tooling']
    for directory in ['scripts/tests', 'scripts/complexity', 'scripts/coverage']:
        executable = 'python3' if directory == 'scripts/tests' else sys.executable
        yield [executable, '-m', 'unittest', 'discover', '-s', str(root/directory), '-p', 'test_*.py']


def python_report(directory, root, env):
    base = [sys.executable, '-m', 'coverage']
    option = '--rcfile=' + str(directory/'coverage.ini')
    # Reporting is outside the measured subprocesses, using their exact same source manifest.
    clean = {key: value for key, value in env.items()
             if key not in {'COVERAGE_PROCESS_START', 'USCREEN_PYTHON_CALLS', 'BASH_ENV'}}
    with (directory/'reporting.log').open('w') as log:
        for args in [['combine', option, '--keep', str(directory)],
                     ['json', option, '-o', str(directory/'python.json')]]:
            subprocess.run([*base, *args], cwd=root, env=clean, check=True, stdout=log, stderr=subprocess.STDOUT)


def finish(directory, manifest, env):
    validate_snapshot(ROOT, manifest)
    python_report(directory, ROOT, env)
    data = {}
    python_json(directory/'python.json', ROOT, data)
    merge(ROOT, directory/'shell', manifest, data)
    relevant = dict(manifest, functions=[f for f in manifest['functions'] if f['language'] in {'python', 'shell'}])
    rows = collect(relevant, data, ([], {}), [], read_calls(directory/'calls'))
    report = dict(scopes=['essential installation, EVDI setup and packaging scripts'], minimum_percent=80,
                  passes=all(row['passes'] for row in rows), summary=summary(rows), functions=rows)
    (directory/'report.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report['summary'], indent=2))
    return report['passes']


def run(directory, report_only):
    directory = directory.resolve()
    directory.mkdir(parents=True)
    manifest = snapshot(ROOT)
    (directory/'manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
    env = environment(directory, ROOT)
    with (directory/'tests.log').open('w') as log:
        for command in commands(ROOT):
            subprocess.run(command, cwd=ROOT, env=env, check=True, stdout=log, stderr=subprocess.STDOUT)
    passed = finish(directory, manifest, env)
    print('PASS' if passed else 'FAIL: script functions remain below threshold or unmeasured')
    return 0 if report_only else int(not passed)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('output', type=Path, help='new evidence directory; existing directories are rejected')
    parser.add_argument('--report-only', action='store_true', help='report remaining gaps without claiming success')
    args = parser.parse_args()
    return run(args.output, args.report_only)


if __name__ == '__main__':
    raise SystemExit(main())
