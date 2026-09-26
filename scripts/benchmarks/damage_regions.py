#!/usr/bin/env python3
"""T554: paired one-session replay against the pre-region conversion modules."""
import argparse
import hashlib
import json
from pathlib import Path
import platform
import subprocess

from conversion_sources import build_flags, copy_sources

ROOT = Path(__file__).resolve().parents[2]


def build(directory, baseline):
    old = directory / 'baseline'
    old.mkdir()
    copy_sources(ROOT, old, baseline)
    variants = [('row', old, ['-DROW_BASELINE']), ('span', ROOT / 'host/evdi', [])]
    for variant, sources, flags in variants:
        subprocess.run(['cc', '-std=c11', '-O3', '-pthread', '-Wall', '-Wextra', '-Werror',
                        *flags, *build_flags(sources), '-I', str(sources), str(ROOT / 'scripts/benchmarks/damage_regions.c'),
                        str(sources / 'conversion.c'), str(sources / 'frame_exchange.c'),
                        '-o', str(directory / variant)], check=True)


def run_pair(directory, trial, scale, case):
    rows = []
    order = ['row', 'span'] if trial % 2 == 0 else ['span', 'row']
    for variant in order:
        raw = subprocess.check_output([str(directory / variant), str(scale), str(case)],
                                      stderr=subprocess.DEVNULL)
        row = json.loads(raw)
        row.update(variant=variant, trial=trial)
        rows.append(row)
    if rows[0]['checksum'] != rows[1]['checksum']:
        raise ValueError(f'Frame checksum mismatch: {rows}')
    return rows


def metadata(baseline):
    sources = list((ROOT / 'host/evdi').glob('*.[ch]'))
    sources += [Path(__file__).resolve(), ROOT / 'scripts/benchmarks/damage_regions.c']
    return dict(baseline=subprocess.check_output(['git', 'rev-parse', baseline], cwd=ROOT, text=True).strip(),
                kernel=platform.release(), machine=platform.machine(),
                compiler=subprocess.check_output(['cc', '--version'], text=True).splitlines()[0],
                source_sha256={str(path.relative_to(ROOT)): hashlib.sha256(path.read_bytes()).hexdigest()
                               for path in sources})


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('output', type=Path, help='new directory for binaries, baseline sources and raw results')
    parser.add_argument('--baseline', default='d1de64a', help='pre-T554 revision')
    parser.add_argument('--trials', type=int, choices=range(1, 10), default=5)
    args = parser.parse_args()
    directory = args.output.resolve()
    directory.mkdir(parents=True)
    build(directory, args.baseline)
    (directory / 'metadata.json').write_text(json.dumps(metadata(args.baseline), indent=2) + '\n')
    rows = []
    for trial in range(args.trials):
        for scale in range(1, 5):
            for case in range(5):
                rows.extend(run_pair(directory, trial, scale, case))
        (directory / 'raw.json').write_text(json.dumps(rows, indent=2) + '\n')
        print(f'Trial {trial + 1} complete', flush=True)


if __name__ == '__main__':
    main()
