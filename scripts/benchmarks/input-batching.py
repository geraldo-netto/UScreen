#!/usr/bin/env python3
"""T406: compare the recorded immediate emitter with the current batch writer."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
BASE = ROOT / 'docs/benchmarks/2026-09-17-input-batching/baseline-event-writer.rs.txt'
MANIFEST = '''[package]
name = "uscreen-input-batching-replay"
version = "0.0.0"
edition = "2021"
[features]
baseline = []
[dependencies]
libc = "0.2"
serde_json = "1"
'''


def build(root, variant):
    folder = root / variant
    (folder / 'src').mkdir(parents=True)
    source = BASE if variant == 'baseline' else ROOT / 'host/src/input/event_writer.rs'
    shutil.copy2(source, folder / 'src/event_writer.rs')
    shutil.copy2(ROOT / 'scripts/benchmarks/input-batching.rs', folder / 'src/main.rs')
    shutil.copy2(ROOT / 'host/tests/fixtures/t406-input-events.json', folder / 'src/events.json')
    (folder / 'Cargo.toml').write_text(MANIFEST)
    command = ['cargo', 'build', '--offline', '--release']
    if variant == 'baseline':
        command += ['--features', 'baseline']
    subprocess.run(command, cwd=folder, env=dict(os.environ, CARGO_TARGET_DIR=str(folder / 'target')), check=True)
    binary = folder / 'target/release/uscreen-input-batching-replay'
    return binary, hashlib.sha256(source.read_bytes()).hexdigest()


def run(root, rounds):
    builds = {variant: build(root, variant) for variant in ['baseline', 'candidate']}
    result = dict(source_sha256={variant: data[1] for variant, data in builds.items()},
                  compiler=subprocess.check_output(['rustc', '--version'], text=True).strip(), trials=[])
    for sessions in [1, 2, 4]:
        expected = None
        for variant, (binary, _) in builds.items():
            row = json.loads(subprocess.check_output([str(binary), str(sessions), str(rounds)], text=True))
            signatures = [[tuple(device[1:]) for device in lane] for lane in row['lanes']]
            if expected is not None and signatures != expected:
                raise RuntimeError('native-byte stream or count changed')
            expected = signatures
            row.update(variant=variant, binary_sha256=hashlib.sha256(binary.read_bytes()).hexdigest())
            result['trials'].append(row)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--rounds', type=int, default=10000)
    args = parser.parse_args()
    if not 1 <= args.rounds <= 100000:
        parser.error('rounds must be in 1..100000')
    with tempfile.TemporaryDirectory(prefix='uscreen-input-batching-') as directory:
        result = run(Path(directory), args.rounds)
    args.output.write_text(json.dumps(result, indent=2) + '\n')


if __name__ == '__main__':
    main()
