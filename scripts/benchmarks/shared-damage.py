#!/usr/bin/env python3
"""T570: paired exact-source publication replay; no display/device changes."""
import argparse
import hashlib
import json
from pathlib import Path
import platform
import subprocess

ROOT = Path(__file__).resolve().parents[2]


def build(directory, baseline):
    old = directory / 'baseline'
    old.mkdir()
    files = subprocess.check_output(['git', 'ls-tree', '-r', '--name-only', baseline,
                                     'host/evdi'], cwd=ROOT, text=True).splitlines()
    for name in files:
        if Path(name).suffix in ['.c', '.h']:
            (old / Path(name).name).write_bytes(subprocess.check_output(
                ['git', 'show', f'{baseline}:{name}'], cwd=ROOT))
    for variant, sources in [('full', old), ('damage', ROOT / 'host/evdi')]:
        subprocess.run(['cc', '-std=c11', '-O3', '-pthread', '-Wall', '-Wextra', '-Werror',
                        '-ffunction-sections', '-fdata-sections', '-Wl,--gc-sections',
                        '-I', str(sources), str(ROOT / 'scripts/benchmarks/shared-damage.c'),
                        *[str(sources / name) for name in ['conversion.c', 'frame_exchange.c', 'raw_ring.c']],
                        '-o', str(directory / variant)], check=True)


def pair(directory, trial, scale, mode):
    rows = []
    order = ['full', 'damage'] if trial % 2 == 0 else ['damage', 'full']
    for variant in order:
        output = subprocess.check_output([str(directory / variant), str(scale), str(mode)],
                                         stderr=subprocess.DEVNULL, timeout=30)
        row = json.loads(output)
        row.update(variant=variant, trial=trial)
        rows.append(row)
    if rows[0]['checksum'] != rows[1]['checksum']:
        raise ValueError(f'Published pixel checksum mismatch: {rows}')
    return rows


def metadata(baseline):
    paths = list((ROOT / 'host/evdi').glob('*.[ch]'))
    paths += [Path(__file__).resolve(), ROOT / 'scripts/benchmarks/shared-damage.c']
    return dict(baseline=baseline, kernel=platform.release(), machine=platform.machine(),
                compiler=subprocess.check_output(['cc', '--version'], text=True).splitlines()[0],
                sources={str(p.relative_to(ROOT)): hashlib.sha256(p.read_bytes()).hexdigest() for p in paths})


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('output', type=Path)
    parser.add_argument('--baseline', default='7cac165')
    parser.add_argument('--trials', type=int, choices=range(1, 10), default=5)
    args = parser.parse_args()
    baseline = subprocess.check_output(['git', 'rev-parse', '--verify', args.baseline + '^{commit}'],
                                       cwd=ROOT, text=True).strip()
    directory = args.output.resolve()
    directory.mkdir(parents=True)
    build(directory, baseline)
    (directory / 'metadata.json').write_text(json.dumps(metadata(baseline), indent=2) + '\n')
    rows = []
    for trial in range(args.trials):
        for scale in range(1, 5):
            for mode in range(3):
                rows.extend(pair(directory, trial, scale, mode))
        (directory / 'raw.json').write_text(json.dumps(rows, indent=2) + '\n')
        print(f'Trial {trial + 1} complete', flush=True)


if __name__ == '__main__':
    main()
