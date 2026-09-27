#!/usr/bin/env python3
"""T652: retain maintained files, include! reachability and unsampled scope.

Copyright (c) 2026 Geraldo Netto. Uses the existing coverage inventory, not the
mutation engine's incomplete file discovery, as the denominator of this listing.
"""
import argparse
from collections import Counter, defaultdict
import json
from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'scripts/coverage'))
from inventory import module_path, parse, test_only, walk
from report import snapshot
from rust_platform import guarded

ROOTS = ['common/src/lib.rs', 'host/src/lib.rs', 'host/src/main.rs', 'gui/src/main.rs']


def destination(root, path, node):
    if node.type == 'mod_item':
        return module_path(root, path, node)
    if node.type == 'macro_invocation':
        match = re.fullmatch(r'include!\s*\(\s*"([^"]+)"\s*\)', node.text.decode())
        if match:
            return (root / path.parent / match[1]).resolve()
    return None


def references(root, paths, platform):
    edges = defaultdict(list)
    for path in paths:
        for node in walk(parse((root / path).read_text(), 'rust')):
            if test_only(node) or guarded(node, platform):
                continue
            target = destination(root, path, node)
            if target is not None:
                edges[str(path)].append((target.relative_to(root.resolve()).as_posix(), node.type))
    return edges


def reachable(edges, roots=ROOTS):
    direct, included = set(), set()
    pending = [(name, False) for name in roots]
    visited = set()
    while pending:
        name, via_include = pending.pop()
        if (name, via_include) in visited:
            continue
        visited.add((name, via_include))
        (included if via_include else direct).add(name)
        pending.extend((target, via_include or kind == 'macro_invocation') for target, kind in edges[name])
    return direct, included


def row(name, functions, manifest, discovered, explicit, native):
    language = functions[0]['language']
    platforms = [platform for platform, (direct, included) in native.items() if name in direct | included]
    includes = [platform for platform, (direct, included) in native.items() if name in included - direct]
    return dict(file=name, language=language, languages=sorted({function['language'] for function in functions}), sha256=manifest['sources'][name],
                maintained_functions=len(functions), native_rust_files=platforms if language == 'rust' else None,
                rust_include_only=includes if language == 'rust' else [],
                cargo_discovered_candidates=discovered[name] if language == 'rust' else None,
                reviewed_bounded_candidates=explicit[name],
                disposition='bounded candidates selected' if explicit[name] else 'not selected by bounded catalog',
                no_generated_rust_candidates=language == 'rust' and discovered[name] == 0)


def inventory(root, generated, catalog):
    manifest = snapshot(root)
    rust = [Path(name) for name in manifest['sources'] if name.endswith('.rs')]
    native = {platform: reachable(references(root, rust, platform)) for platform in ['linux', 'windows']}
    discovered = Counter(candidate['file'] for candidate in generated)
    explicit = defaultdict(list)
    for candidate in catalog['mutations']:
        explicit[candidate['file']].append(candidate['id'])
    functions = defaultdict(list)
    for function in manifest['functions']:
        functions[function['file']].append(function)
    rows = [row(name, items, manifest, discovered, explicit, native) for name, items in sorted(functions.items())]
    return dict(version=1, files=rows, whole_project_mutation_score=None,
                note='File reachability is not function execution. Mixed cfg/feature branches still need native test evidence; discovery counts are not tested counts.',
                source_files_without_maintained_functions=sorted(set(manifest['sources']) - set(functions)))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--cargo-inventory', required=True, type=Path)
    parser.add_argument('--catalog', type=Path, default=Path(__file__).with_name('catalog.json'))
    parser.add_argument('--output', required=True, type=Path)
    args = parser.parse_args()
    result = inventory(ROOT, json.loads(args.cargo_inventory.read_text()), json.loads(args.catalog.read_text()))
    args.output.write_text(json.dumps(result, indent=2) + '\n')
    print(f'{len(result["files"])} maintained source files; sampled and unselected scope retained')


if __name__ == '__main__':
    main()
