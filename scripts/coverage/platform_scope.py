"""Explicit accepted Rust platforms; excluded functions remain visible in reports."""
from rust_check import platform_inventory
from inventory import fixture
from pathlib import Path


def apply(root, manifest, rows, platforms):
    if not platforms:
        return rows, []
    if set(platforms) - {'linux', 'windows'}:
        raise ValueError('unsupported coverage platform scope')
    inventories = [platform_inventory(root, manifest, platform) for platform in platforms]
    excluded = [row for row in rows if row['language'] == 'rust' and all(
        row['file'] in files or (row['file'], row['line']) in functions
        for files, functions in inventories)]
    selected = [row for row in rows if row not in excluded]
    if not selected:
        raise ValueError('selected platform scope contains no maintained functions')
    return selected, excluded


def native_hashes(root, path):
    import json
    from model import verify_sources
    hashes = json.loads(path.read_text())
    if not isinstance(hashes, dict) or not hashes:
        raise ValueError('native source fingerprints must be a nonempty mapping')
    verify_sources(root, hashes)
    return hashes


def attest_native(root, paths, data, platforms):
    if 'windows' in platforms and not paths:
        raise ValueError('Windows scope requires native source fingerprints')
    hashes = {}
    for path in paths:
        hashes.update(native_hashes(root, path))
    if paths:
        missing = {name for name in data if name.endswith('.rs') and not fixture(Path(name))} - set(hashes)
        if missing:
            raise ValueError(f'native Rust counters lack source fingerprints: {sorted(missing)}')
