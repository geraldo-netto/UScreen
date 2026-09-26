#!/usr/bin/env python3
"""T405: paired check of FIONREAD sampling overhead on the pipe-capacity result."""
import argparse
import hashlib
import itertools
import json
import os
from pathlib import Path
import platform
import random
import subprocess
import tempfile

SOURCE = Path(__file__).resolve().with_name('pipe-capacity.c')


def run(args, binary):
    command = ['cc', '-O3', '-Wall', '-Wextra', '-Werror', '-pthread', str(SOURCE), '-o', str(binary)]
    subprocess.run(command, check=True)
    result = dict(platform=platform.platform(), cpus=sorted(os.sched_getaffinity(0)), build=command,
                  source_sha256=hashlib.sha256(SOURCE.read_bytes()).hexdigest(), seed=405390,
                  controller_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(), trials=[])
    randomizer = random.Random(result['seed'])
    cases = list(itertools.product([1536000, 8205120], [1, 4]))
    for index, (size, sessions) in enumerate(cases):
        for trial in range(args.trials):
            comparisons = list(itertools.product([1, 2, 4, 8, 12, 16, 24, 32], [0, 1]))
            randomizer.shuffle(comparisons)
            for capacity, sample in comparisons:
                command = [str(binary), str(capacity), str(sessions), str(size), '120', '1', '0', str(sample)]
                output = subprocess.run(command, check=True, capture_output=True, text=True, timeout=30)
                row = json.loads(output.stdout)
                row.update(requested_mib=capacity, size=size, sessions=sessions, mode='paced', frames=120,
                           sampled=bool(sample), trial=trial)
                result['trials'].append(row)
            args.output.write_text(json.dumps(result, indent=2) + '\n')
            print(f'T405 control: case {index + 1}/{len(cases)}, trial {trial + 1}/{args.trials}', flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--trials', type=int, default=3)
    args = parser.parse_args()
    if args.trials < 1:
        parser.error('trials must be positive')
    with tempfile.TemporaryDirectory(prefix='blent-pipe-control-') as directory:
        run(args, Path(directory) / 'transport')


if __name__ == '__main__':
    main()
