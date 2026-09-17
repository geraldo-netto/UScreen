#!/usr/bin/env python3
"""T389: measure a bounded shared-memory prototype and ordinary-page fallbacks."""
import argparse
from concurrent.futures import ThreadPoolExecutor
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess

ROOT = Path(__file__).resolve().parents[2]
VARIANTS = [('fifo', 'aligned'), ('fifo', 'anon'), ('fifo', 'thp'), ('ring', 'anon'), ('ring', 'thp')]


def build(folder):
    folder.mkdir()
    source = folder / 'raw-ring.c'
    shutil.copy2(ROOT / 'scripts/benchmarks/raw-ring.c', source)
    binary = folder / 'raw-ring'
    subprocess.run(['cc', '-O3', '-Wall', '-Wextra', '-Werror', '-std=c11', str(source), '-o', str(binary)], check=True)
    return binary


def replay(binary, size, frames, sessions, transport, allocation):
    command = [str(binary), str(size), str(frames), transport, allocation]
    def one(_):
        return json.loads(subprocess.check_output(command, text=True, timeout=65))
    with ThreadPoolExecutor(max_workers=sessions) as workers:
        return list(workers.map(one, range(sessions)))


def run(args, binary):
    result = dict(platform=platform.platform(), affinity=sorted(os.sched_getaffinity(0)),
                  compiler=subprocess.check_output(['cc', '--version'], text=True),
                  sha256=hashlib.sha256(binary.read_bytes()).hexdigest(), trials=[])
    for path in ['enabled', 'shmem_enabled', 'defrag']:
        result[path] = Path('/sys/kernel/mm/transparent_hugepage', path).read_text().strip()
    for case, (size, sessions) in enumerate([(size, n) for size in [1536000, 12441600] for n in [1, 4]]):
        for repeat in range(args.trials):
            variants = VARIANTS if (case + repeat) % 2 else list(reversed(VARIANTS))
            for transport, allocation in variants:
                streams = replay(binary, size, args.frames, sessions, transport, allocation)
                result['trials'].append(dict(size=size, frames=args.frames, sessions=sessions, trial=repeat,
                                             transport=transport, allocation=allocation, streams=streams))
                args.output.write_text(json.dumps(result, indent=2) + '\n')
        print(f'T389 ring: case {case + 1}/4', flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--directory', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--trials', type=int, default=5)
    parser.add_argument('--frames', type=int, default=300)
    parser.add_argument('--build-only', action='store_true')
    parser.add_argument('--reuse', action='store_true')
    args = parser.parse_args()
    if args.trials < 1 or not 3 <= args.frames <= 10000:
        parser.error('positive trials and 3..10000 frames required')
    binary = args.directory / 'raw-ring' if args.reuse else build(args.directory)
    if not args.build_only:
        run(args, binary)


if __name__ == '__main__':
    main()
