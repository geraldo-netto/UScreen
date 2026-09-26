#!/usr/bin/env python3
"""T600: isolated Tokio worker ramp over production queue/storage modules."""
import argparse
import json
from pathlib import Path
import shutil
import subprocess
import thread_ramp_common as C


def build(folder):
    source = folder / 'src'
    source.mkdir(parents=True)
    manifest = '''[package]
name = "blent-runtime-ramp"
version = "0.0.0"
edition = "2021"
[dependencies]
tokio = { version = "1", features = ["full"] }
libc = "0.2"
serde_json = "1"
bytes = "1"
tracing = "0.1"
[workspace]
'''
    (folder / 'Cargo.toml').write_text(manifest)
    paths = [C.ROOT / 'host/src' / name for name in ['video_queue.rs', 'media_storage.rs']]
    for path in paths:
        shutil.copy2(path, source / path.name)
    shutil.copy2(Path(__file__).with_suffix('.rs'), source / 'main.rs')
    shutil.copy2(C.ROOT / 'Cargo.lock', folder / 'Cargo.lock')
    subprocess.run(['cargo', 'build', '--offline', '--release', '--manifest-path', str(folder / 'Cargo.toml')], check=True)
    return folder / 'target/release/blent-runtime-ramp', paths


def run(args):
    args.output.mkdir(parents=True)
    binary, paths = build(args.output / 'build')
    info = C.metadata([*paths, Path(__file__), Path(__file__).with_suffix('.rs'), Path(C.__file__)])
    info['boundary'] = 'synthetic 64KiB 60Hz production queue/storage to loopback; excludes full daemon and Android'
    C.save(args.output / 'metadata.json', info)
    if args.build_only:
        return
    measure(args.output, binary)


def measure(output, binary):
    rows = []
    for repeat, workers in C.order():
        result = subprocess.check_output([str(binary), str(workers)], text=True, timeout=20)
        row = dict(json.loads(result), repeat=repeat)
        rows.append(row)
        C.save(output / 'results.json', rows)
        print('runtime', repeat, workers, flush=True)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--build-only', action='store_true')
    args = parser.parse_args()
    if (args.output / 'metadata.json').exists():
        measure(args.output, args.output / 'build/target/release/blent-runtime-ramp')
    else:
        run(args)
