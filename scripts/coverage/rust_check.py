#!/usr/bin/env python3
"""T497: gate Linux Rust and retain Windows-only gaps in a separate full report."""
import argparse
import json
from pathlib import Path
import re

from inventory import attributes, module_path, parse, walk
from report import ROOT, collect, summary, validate_snapshot
from readers import lcov

SCOPES = ['common/src/', 'host/src/', 'gui/src/']
WINDOWS_CFG = re.compile(r'#\[cfg\(\s*(?:all\(\s*)?(?:windows\b|target_os\s*=\s*"windows")')


def guarded(node):
    while node is not None:
        if WINDOWS_CFG.search(attributes(node)):
            return True
        node = node.parent
    return False


def platform_inventory(root, manifest):
    references, functions = {}, set()
    for name in manifest['sources']:
        path = Path(name)
        if path.suffix != '.rs':
            continue
        for node in walk(parse((root/path).read_text(), 'rust')):
            record_node(root, path, node, references, functions)
    windows = set()
    while True:
        found = {path for path, owners in references.items()
                 if all(explicit or source in windows for source, explicit in owners)}
        if found <= windows:
            return windows, functions
        windows.update(found)


def record_node(root, path, node, references, functions):
    if node.type == 'function_item' and guarded(node):
        functions.add((str(path), node.start_point.row + 1))
    if node.type == 'mod_item':
        destination = module_path(root, path, node)
        if destination is not None:
            references.setdefault(str(destination.relative_to(root.resolve())), []).append((str(path), guarded(node)))


def reports(root, manifest, data):
    rows = collect(manifest, data, ([], {}), SCOPES)
    files, functions = platform_inventory(root, manifest)
    windows = [row for row in rows if row['file'] in files or (row['file'], row['line']) in functions]
    linux = [row for row in rows if row not in windows]
    if not linux:
        raise ValueError('Linux Rust coverage scope is empty')
    if any(row['total'] for row in windows):
        raise ValueError('Windows-only counters were supplied to a Linux collection')
    return report(rows, 'all Rust application platforms'), report(linux, 'Linux Rust applications'), windows


def report(rows, scope):
    return dict(scopes=[scope], minimum_percent=80, passes=all(row['passes'] for row in rows),
                summary=summary(rows), functions=rows)


def run(manifest_path, paths, directory):
    manifest = json.loads(manifest_path.read_text())
    validate_snapshot(ROOT, manifest)
    data = {}
    for path in paths:
        lcov(path, ROOT, data)
    full, linux, windows = reports(ROOT, manifest, data)
    directory.mkdir(parents=True, exist_ok=True)
    linux['unavailable_platform'] = dict(platform='Windows', todo='T493/T497', functions=len(windows))
    (directory/'all-platforms.json').write_text(json.dumps(full, indent=2) + '\n')
    (directory/'linux.json').write_text(json.dumps(linux, indent=2) + '\n')
    print(json.dumps(linux['summary'], indent=2))
    print(f'Windows-only functions remain visible in all-platforms.json: {len(windows)}')
    print('Linux PASS' if linux['passes'] else 'Linux FAIL: uncovered or unmeasured functions remain')
    return int(not linux['passes'])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--manifest', type=Path, required=True)
    parser.add_argument('--lcov', type=Path, action='append', required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    return run(args.manifest, args.lcov, args.output)


if __name__ == '__main__':
    raise SystemExit(main())
