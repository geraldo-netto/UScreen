#!/usr/bin/env python3
"""T383: public C pool/damage replay, no EVDI, encoder or desktop attachment."""
import argparse
import hashlib
import itertools
import json
import os
from pathlib import Path
import platform
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
MODULES = ['conversion.c', 'conversion.h', 'frame_exchange.c', 'frame_exchange.h']
DAMAGE = ['empty', 'sparse', 'overlap', 'full']


def build(folder, reference, scalar=False):
    folder.mkdir()
    sources = {}
    for name in MODULES:
        rel = 'host/evdi/' + name
        data = subprocess.check_output(['git', 'show', f'{reference}:{rel}'], cwd=ROOT) if reference else (ROOT / rel).read_bytes()
        (folder / name).write_bytes(data)
        sources[rel] = hashlib.sha256(data).hexdigest()
    binary = folder / 'conversion'
    extra = ['-fno-tree-vectorize', '-fno-tree-slp-vectorize'] if scalar else []
    if 'last_jobs' in (folder / 'conversion.h').read_text():
        extra.append('-DBLENT_ADAPTIVE_POOL')
    subprocess.run(['cc', *extra, '-std=c11', '-O3', '-Wall', '-Wextra', '-Werror', '-pthread', '-I', str(folder),
                    str(ROOT / 'scripts/benchmarks/conversion.c'), str(folder / 'conversion.c'),
                    str(folder / 'frame_exchange.c'), '-o', str(binary)], check=True)
    return binary, sources


def measure(binary, cpus, sessions, workers, scale, damage, multiplier, samples):
    command = ['taskset', '-c', ','.join(map(str, cpus)), str(binary), str(sessions), str(workers),
               str(scale), str(damage), str(samples), str(multiplier)]
    output = subprocess.run(command, check=True, capture_output=True, text=True, timeout=30)
    result = json.loads(output.stdout)
    result.update(cpus=cpus, sessions=sessions, workers=workers, scale=scale, damage=DAMAGE[damage], samples=samples, multiplier=multiplier)
    return result


def cases(affinity, quick, large, native):
    if native:
        return itertools.product([affinity], [1, 2, 4], [8], [1], [3], [1])
    if large:
        return itertools.product([affinity], [1, 4], [8, 16, 32, 64, 128], [1], [1, 3], [1, 2, 4])
    if quick:
        return itertools.product([affinity], [1, 4], [1, 8], [1, 2, 4], range(4), [1])
    return itertools.product([affinity, affinity[:4]], [1, 2, 4], [1, 2, 4, 8], [1, 2, 3, 4], range(4), [1])


def verify_checksums(trials):
    expected = {}
    for row in trials:
        key = (row['sessions'], row['scale'], row['multiplier'])
        actual = [lane['checksum'] for lane in row['lanes']]
        if key in expected and expected[key] != actual:
            raise RuntimeError(f"pixel mismatch for {key}: {row['variant']}")
        expected[key] = actual


def run(args, temporary):
    variants = {'candidate': build(temporary / 'candidate', None)}
    if args.baseline:
        variants['baseline'] = build(temporary / 'baseline', args.baseline)
    if args.scalar:
        variants['scalar'] = build(temporary / 'scalar', None, True)
    affinity = sorted(os.sched_getaffinity(0))
    result = dict(platform=platform.platform(), compiler=subprocess.check_output(['cc', '--version'], text=True).splitlines()[0],
                  parent=subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip(), baseline=args.baseline,
                  harness_sha256=hashlib.sha256((ROOT / 'scripts/benchmarks/conversion.c').read_bytes()).hexdigest(),
                  sources={name: value[1] for name, value in variants.items()}, trials=[])
    for number, case in enumerate(cases(affinity, args.quick, args.large, args.native)):
        for trial in range(args.trials):
            names = list(variants) if (number + trial) % 2 else list(reversed(variants))
            for name in names:
                row = measure(variants[name][0], *case, args.samples)
                row.update(variant=name, trial=trial)
                result['trials'].append(row)
        if number % 24 == 0:
            print(f'T383 completed case {number + 1}', flush=True)
    verify_checksums(result['trials'])
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--baseline')
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--samples', type=int, default=24)
    parser.add_argument('--trials', type=int, default=3)
    parser.add_argument('--quick', action='store_true')
    parser.add_argument('--large', action='store_true')
    parser.add_argument('--native', action='store_true')
    parser.add_argument('--scalar', action='store_true')
    args = parser.parse_args()
    if not 1 <= args.samples <= 256 or args.trials < 1:
        parser.error('samples must be 1..256 and trials positive')
    with tempfile.TemporaryDirectory(prefix='blent-conversion-') as folder:
        result = run(args, Path(folder))
    args.output.write_text(json.dumps(result, indent=2) + '\n')


if __name__ == '__main__':
    main()
