#!/usr/bin/env python3
"""T405: compare readiness I/O and ordinary io_uring on local pipes/TCP."""
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
SOURCE = ROOT / 'scripts/benchmarks/io-uring.c'


def build(binary, args):
    dependency = ['-I', str(args.include), str(args.library)] if args.library else subprocess.check_output(
        ['pkg-config', '--cflags', '--libs', 'liburing'], text=True).split()
    command = ['cc', '-O3', '-Wall', '-Wextra', '-Werror', '-pthread', str(SOURCE), *dependency, '-o', str(binary)]
    subprocess.run(command, check=True)
    return command


def measure(binary, cpus, backend, transport, sessions, paced):
    size = 65536 if transport == 'tcp' else 1536000
    frames = 120 if paced else 256
    command = ['taskset', '-c', ','.join(map(str, cpus)), str(binary), str(int(backend == 'uring')),
               str(int(transport == 'tcp')), str(sessions), str(size), str(frames), str(paced)]
    result = subprocess.run(command, check=True, capture_output=True, text=True, timeout=30)
    row = json.loads(result.stdout)
    row.update(backend=backend, transport=transport, sessions=sessions, paced=bool(paced),
               size=size, frames=frames)
    return row


def run(args, binary):
    command = build(binary, args)
    cpus = sorted(os.sched_getaffinity(0))
    result = dict(platform=platform.platform(), build=command, cpus=cpus,
                  source_sha256=hashlib.sha256(SOURCE.read_bytes()).hexdigest(), trials=[])
    cases = itertools.product(['pipe', 'tcp'], [1, 2, 4], [0, 1])
    for index, case in enumerate(cases):
        for trial in range(args.trials):
            backends = ['poll', 'uring'] if (index + trial) % 2 else ['uring', 'poll']
            for backend in backends:
                row = measure(binary, cpus, backend, *case)
                row['trial'] = trial
                result['trials'].append(row)
        print(f'T405 completed case {index + 1}/12', flush=True)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--include', type=Path)
    parser.add_argument('--library', type=Path)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--trials', type=int, default=3)
    args = parser.parse_args()
    if bool(args.include) != bool(args.library) or args.trials < 1:
        parser.error('include/library must be supplied together; trials must be positive')
    with tempfile.TemporaryDirectory(prefix='uscreen-uring-') as directory:
        result = run(args, Path(directory) / 'transport')
    args.output.write_text(json.dumps(result, indent=2) + '\n')


if __name__ == '__main__':
    main()
