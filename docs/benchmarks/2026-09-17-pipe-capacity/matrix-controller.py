#!/usr/bin/env python3
"""T405/T389: pipe-capacity ramp; run in an isolated CAP_SYS_RESOURCE container."""
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

ROOT = Path(__file__).resolve().parents[2]
SOURCE = ROOT / 'scripts/benchmarks/pipe-capacity.c'
CAPACITIES = [1, 2, 4, 8, 12, 16, 24, 32]


def measure(binary, capacity, size, sessions, mode, frames):
    command = [str(binary), str(capacity), str(sessions), str(size), str(frames),
               str(int(mode != 'peak')), str(20 if mode == 'slow' else 0)]
    completed = subprocess.run(command, capture_output=True, text=True, timeout=30)
    if completed.returncode:
        raise RuntimeError(f'{command}: {completed.returncode}: {completed.stderr}')
    row = json.loads(completed.stdout)
    row.update(requested_mib=capacity, size=size, sessions=sessions, mode=mode, frames=frames)
    return row


def metadata(command):
    sysctls = ['pipe-max-size', 'pipe-user-pages-soft', 'pipe-user-pages-hard']
    return dict(platform=platform.platform(), cpus=sorted(os.sched_getaffinity(0)), build=command,
                compiler=subprocess.check_output(['cc', '--version'], text=True).splitlines()[0],
                source_sha256=hashlib.sha256(SOURCE.read_bytes()).hexdigest(),
                controller_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
                sysctls={name: Path('/proc/sys/fs', name).read_text().strip() for name in sysctls},
                seed=405389, trials=[])


def run(args, binary):
    command = ['cc', '-O3', '-Wall', '-Wextra', '-Werror', '-pthread', str(SOURCE), '-o', str(binary)]
    subprocess.run(command, check=True)
    result = metadata(command)
    randomizer = random.Random(result['seed'])
    cases = list(itertools.product([1536000, 8205120], [1, 2, 4], ['peak', 'paced', 'slow']))
    for index, (size, sessions, mode) in enumerate(cases):
        for trial in range(args.trials):
            capacities = CAPACITIES.copy()
            randomizer.shuffle(capacities)
            for capacity in capacities:
                row = measure(binary, capacity, size, sessions, mode, args.frames)
                row['trial'] = trial
                result['trials'].append(row)
            args.output.write_text(json.dumps(result, indent=2) + '\n')
            print(f'T405 pipe ramp: case {index + 1}/{len(cases)}, trial {trial + 1}/{args.trials}', flush=True)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--trials', type=int, default=3)
    parser.add_argument('--frames', type=int, default=120)
    args = parser.parse_args()
    if args.trials < 1 or not 1 <= args.frames <= 4096:
        parser.error('trials must be positive; frames must be in 1..4096')
    with tempfile.TemporaryDirectory(prefix='uscreen-pipe-') as directory:
        run(args, Path(directory) / 'transport')


if __name__ == '__main__':
    main()
