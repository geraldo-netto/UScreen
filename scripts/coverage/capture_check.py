#!/usr/bin/env python3
"""T497: run the normal C regressions and enforce every capture function at 80%."""
import argparse
from dataclasses import asdict
import json
import os
from pathlib import Path
import shlex
import shutil
import subprocess
import sys

from compiler import export_reports
from inventory import owned_functions
from model import fingerprint, result, verify_sources
from readers import gcov_json
from report import summary

ROOT = Path(__file__).resolve().parents[2]


def prepare(directory):
    # Never merge previous executions or overwrite another measurement directory.
    directory.mkdir(parents=True)
    functions = [function for function in owned_functions(ROOT) if function.file.startswith('host/evdi/')]
    sources = {function.file: fingerprint(ROOT / function.file) for function in functions}
    manifest = dict(sources=sources, functions=[asdict(function) for function in functions])
    (directory/'manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
    return functions, sources


def environment(directory):
    compiler = shutil.which('cc')
    if compiler is None:
        raise ValueError('C coverage requires GCC cc and gcov')
    wrappers = directory/'tools'
    wrappers.mkdir()
    wrapper = wrappers/'cc'
    command = [sys.executable, str(Path(__file__).with_name('compiler.py'))]
    wrapper.write_text('#!/bin/sh\nexec ' + shlex.join(command) + ' "$@"\n')
    wrapper.chmod(0o700)
    return dict(os.environ, BLENT_C_COVERAGE=str(directory/'native'), BLENT_REAL_CC=compiler,
                PATH=str(wrappers) + os.pathsep + os.environ.get('PATH', ''))


def run(directory):
    directory = directory.resolve()
    functions, sources = prepare(directory)
    command = ['cargo', 'test', '--locked', '-p', 'blent']
    for target in ['evdi_helper', 'conversion', 'evdi_modules', 'frame_retirement']:
        command.extend(['--test', target])
    with (directory/'tests.log').open('w') as log:
        subprocess.run(command, cwd=ROOT, env=environment(directory), check=True, stdout=log, stderr=subprocess.STDOUT)
    verify_sources(ROOT, sources)
    data = {}
    for path in export_reports(directory/'native'):
        gcov_json(path, ROOT, data)
    rows = [result(function, data) for function in functions]
    passed = bool(rows) and all(row['passes'] for row in rows)
    report = dict(scopes=['host/evdi/'], minimum_percent=80, passes=passed, summary=summary(rows), functions=rows)
    (directory/'report.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report['summary'], indent=2))
    return int(not passed)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('output', type=Path, help='new directory for raw counters, log and report')
    args = parser.parse_args()
    return run(args.output)


if __name__ == '__main__':
    raise SystemExit(main())
