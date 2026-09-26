#!/usr/bin/env python3
"""T404: actual Rust timing lookup and report-storage replay, no media device."""
import argparse
import hashlib
import itertools
import json
import os
from pathlib import Path
import shutil
import subprocess

ROOT = Path(__file__).resolve().parents[2]
BASE = '0533724'
MANIFEST = '''[package]
name = "blent-timing-replay"
version = "0.0.0"
edition = "2021"
[dependencies]
libc = "0.2"
serde_json = "1"
tracing = "0.1"
'''


def build(folder, variant):
    (folder / 'src').mkdir(parents=True)
    text = (subprocess.check_output(['git', 'show', f'{BASE}:host/src/latency.rs'], cwd=ROOT, text=True)
            if variant == 'baseline' else (ROOT / 'host/src/latency.rs').read_text())
    (folder / 'original.rs.txt').write_text(text)
    lookup = ('state.sent.iter().position(|(candidate, _)| *candidate == sequence)'
              if variant == 'baseline' else 'state.position(sequence)')
    text += f'\nfn probe_position(state: &Inner, sequence: u32) -> Option<usize> {{ {lookup} }}\n'
    text += (ROOT / 'scripts/benchmarks/timing-rust.rs').read_text()
    (folder / 'src/latency.rs').write_text(text)
    shutil.copy2(ROOT / 'scripts/benchmarks/timing-rust-main.rs', folder / 'src/main.rs')
    (folder / 'Cargo.toml').write_text(MANIFEST)
    subprocess.run(['cargo', 'build', '--offline', '--release'], cwd=folder,
                   env=dict(os.environ, CARGO_TARGET_DIR=str(folder / 'target')), check=True)
    binary = folder / 'timing'
    shutil.copy2(folder / 'target/release/blent-timing-replay', binary)
    return binary


def cases(count):
    yield from ((sessions, mode, count) for sessions, mode in itertools.product([1, 2, 4], ['first', 'latest', 'missing', 'sparse']))
    yield from ((sessions, 'reports', 100) for sessions in [1, 2, 4])


def run(args, binaries):
    result = dict(baseline=BASE, compiler=subprocess.check_output(['rustc', '--version'], text=True).strip(),
                  sources={name: hashlib.sha256((binary.parent / 'original.rs.txt').read_bytes()).hexdigest() for name, binary in binaries.items()}, trials=[])
    checksums = {}
    for number, (sessions, mode, count) in enumerate(cases(args.count)):
        for trial in range(args.trials):
            order = list(binaries) if (number + trial) % 2 else list(reversed(binaries))
            for variant in order:
                row = json.loads(subprocess.check_output([str(binaries[variant]), str(sessions), mode, str(count)], text=True))
                signature = [lane['checksum'] for lane in row.get('lanes', [])]
                key = (sessions, mode)
                if key in checksums and checksums[key] != signature:
                    raise RuntimeError(f'lookup result mismatch: {key}')
                checksums[key] = signature
                row.update(variant=variant, sessions=sessions, mode=mode, count=count, trial=trial)
                result['trials'].append(row)
        args.output.write_text(json.dumps(result, indent=2) + '\n')
        print(f'T404 Rust: case {number + 1}/15', flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--directory', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--count', type=int, default=2000000)
    parser.add_argument('--trials', type=int, default=5)
    parser.add_argument('--build-only', action='store_true')
    parser.add_argument('--reuse', action='store_true')
    args = parser.parse_args()
    if args.count < 1 or args.trials < 1:
        parser.error('count and trials must be positive')
    binaries = {name: args.directory / name / 'timing' for name in ['baseline', 'candidate']}
    if not args.reuse:
        binaries = {name: build(args.directory / name, name) for name in binaries}
    if not args.build_only:
        run(args, binaries)


if __name__ == '__main__':
    main()
