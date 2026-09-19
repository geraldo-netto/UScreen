#!/usr/bin/env python3
"""T497: snapshot sources before measuring, then enforce 80% for every function."""
import argparse
from collections import Counter
from dataclasses import asdict
import json
from pathlib import Path

from inventory import coverage_source, fixture, owned_functions, source_files
from kotlin_inventory import functions as kotlin_functions
from kotlin_report import assign_methods, function_key, function_result, read as read_kotlin
from model import Function, fingerprint, result, verify_sources
import readers

ROOT = Path(__file__).resolve().parents[2]


def sources(root):
    paths = {path for path in source_files(root) if coverage_source(path)}
    app_run = Path('packaging/appimage/AppRun')
    if (root / app_run).is_file():
        paths.add(app_run)
    return sorted(paths)


def snapshot(root):
    paths = sources(root)
    functions = [function for function in owned_functions(root) if coverage_source(Path(function.file))]
    kotlin = [root / path for path in paths if path.suffix in {'.kt', '.kts'} and not fixture(path)]
    functions.extend(kotlin_functions(root, kotlin))
    return dict(version=1, sources={str(path): fingerprint(root / path) for path in paths},
                functions=[asdict(function) for function in functions])


def validate_snapshot(root, manifest):
    if manifest['version'] != 1:
        raise ValueError('unsupported coverage manifest')
    current = {str(path) for path in sources(root)}
    if current != set(manifest['sources']):
        raise ValueError('source inventory changed after coverage began')
    verify_sources(root, manifest['sources'])


def native_lines(args, root):
    data = {}
    for path in args.lcov:
        readers.lcov(path, root, data, args.prefix)
    for path in args.python_json:
        readers.python_json(path, root, data, args.prefix)
    for directory in args.gcov:
        for path in sorted(directory.rglob('*.gcov.json.gz')):
            readers.gcov_json(path, root, data, args.prefix)
    return data


def selected(function, scopes):
    return not scopes or any(function.file.startswith(scope) for scope in scopes)


def inline_body(function, native, kotlin_lines):
    # Inline code has no independent method; its owner retains those counters.
    # An entirely missing file provides no evidence for inferring inlining.
    return (function.language == 'kotlin' and function.name == '<lambda>'
            and not native and function.file in kotlin_lines)


def collect(manifest, data, kotlin, scopes, python_calls=frozenset()):
    rows = []
    methods, kotlin_lines = kotlin
    functions = [Function(**entry) for entry in manifest['functions']]
    assigned = assign_methods([function for function in functions if function.language == 'kotlin'], methods)
    for function in functions:
        if not selected(function, scopes):
            continue
        native = assigned.get(function_key(function), [])
        if inline_body(function, native, kotlin_lines):
            continue
        row = function_result(function, native, kotlin_lines) if function.language == 'kotlin' else result(function, data)
        if function.language == 'python':
            require_invocation(row, function, python_calls)
        rows.append(row)
    if not rows:
        raise ValueError('coverage scope contains no maintained functions')
    return rows


def require_invocation(row, function, observed):
    from calls import invoked
    row['invoked'] = invoked(function, observed)
    if not row['invoked']:
        row.update(covered=0, total=0, percent=None, passes=False,
                   evidence='no matching Python function invocation recorded')


def summary(rows):
    counts = Counter(row['language'] for row in rows)
    passing = Counter(row['language'] for row in rows if row['passes'])
    missing = Counter(row['language'] for row in rows if not row['total'])
    return {language: dict(functions=total, passing=passing[language],
                           below_80=total - passing[language] - missing[language], unmeasured=missing[language])
            for language, total in sorted(counts.items())}


def generate(args):
    manifest = json.loads(args.manifest.read_text())
    validate_snapshot(ROOT, manifest)
    data = native_lines(args, ROOT)
    if args.shell:
        from shell import merge
        merge(ROOT, args.shell, manifest, data)
    kotlin = read_kotlin(args.jacoco, ROOT / args.kotlin_source, ROOT) if args.jacoco else ([], {})
    from calls import read_calls
    observed = read_calls(args.python_calls) if args.python_calls else set()
    rows = collect(manifest, data, kotlin, args.scope, observed)
    report = dict(minimum_percent=80, scopes=args.scope or ['Rust applications, Android app, C EVDI helper and essential installation/packaging scripts; benchmarks excluded'],
                  passes=all(row['passes'] for row in rows), summary=summary(rows), functions=rows)
    args.output.write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report['summary'], indent=2))
    print('PASS' if report['passes'] else 'FAIL: below-threshold or unmeasured functions remain')
    return 0 if args.report_only else int(not report['passes'])


def arguments():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest='command', required=True)
    capture = sub.add_parser('snapshot')
    capture.add_argument('output', type=Path)
    report = sub.add_parser('check')
    report.add_argument('--manifest', type=Path, required=True)
    report.add_argument('--output', type=Path, required=True)
    for option in ['lcov', 'gcov', 'python-json']:
        report.add_argument('--'+option, type=Path, action='append', default=[])
    report.add_argument('--jacoco', type=Path)
    report.add_argument('--python-calls', type=Path)
    report.add_argument('--shell', type=Path)
    report.add_argument('--kotlin-source', type=Path, default=Path('android/app/src/main/java'))
    report.add_argument('--prefix', type=Path, help='source root used inside a build container')
    report.add_argument('--scope', action='append', default=[], help='explicit source prefix; recorded in report')
    report.add_argument('--report-only', action='store_true', help='write gaps without claiming the gate passed')
    return parser.parse_args()


def main():
    args = arguments()
    if args.command == 'snapshot':
        args.output.write_text(json.dumps(snapshot(ROOT), indent=2) + '\n')
        return 0
    return generate(args)


if __name__ == '__main__':
    raise SystemExit(main())
