#!/usr/bin/env python3
"""T389: build actual raw-input code and replay isolated FIFOs into AVFrame planes."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess

ROOT = Path(__file__).resolve().parents[2]
BASE = '8c3279b'
FILES = ['host/src/encoder_frame.rs', 'host/src/encoder_io.rs', 'host/src/encoder_fifo.rs']


def prepare(folder):
    folder.mkdir(parents=True)
    (folder / 'src').mkdir()
    (folder / 'originals').mkdir()
    for name in FILES:
        data = (ROOT / name).read_bytes()
        (folder / 'originals' / Path(name).name).write_bytes(data)
        (folder / 'src' / Path(name).name).write_bytes(data)
    io = folder / 'src/encoder_io.rs'
    text = io.read_text().split('/// Length of the Annex B prefix')[0]
    text = text[text.index('\n\n') + 2:].replace('#[cfg(feature = "inproc-encoder")]\n', '')
    io.write_text(text)
    source = subprocess.check_output(['git', '-c', f'safe.directory={ROOT}', 'show', f'{BASE}:host/src/encoder.rs'], cwd=ROOT, text=True)
    (folder / 'originals/baseline_encoder.rs').write_text(source)
    copy = source[source.index('fn copy_plane('):source.index('/// Read whole NV12 frames')]
    (folder / 'src/baseline_copy.rs').write_text(copy.replace('fn copy_plane(', 'fn baseline_copy_plane('))
    shutil.copy2(ROOT / 'scripts/benchmarks/raw-input.rs', folder / 'src/main.rs')
    (folder / 'Cargo.toml').write_text('''[package]
name = "blent-raw-input-bench"
version = "0.0.0"
edition = "2021"
[dependencies]
libc = "0.2"
tracing = "0.1"
serde_json = "1"
tempfile = "3"
anyhow = "1"
ffmpeg-next = "9"
''')


def build(folder):
    prepare(folder)
    subprocess.run(['cargo', 'build', '--offline', '--release'], cwd=folder,
                   env=dict(os.environ, CARGO_TARGET_DIR=str(folder / 'target')), check=True)
    binary = folder / 'raw-input'
    shutil.copy2(folder / 'target/release/blent-raw-input-bench', binary)
    return binary


def run(args, binary):
    result = dict(baseline=BASE, platform=platform.platform(), affinity=sorted(os.sched_getaffinity(0)),
                  compiler=subprocess.check_output(['rustc', '--version'], text=True).strip(),
                  sha256=hashlib.sha256(binary.read_bytes()).hexdigest(), trials=[])
    cases = [(w, h, n) for w, h in [(1280, 800), (1920, 1080), (3840, 2160), (1282, 800)] for n in [1, 2, 4]]
    for case, (width, height, sessions) in enumerate(cases):
        for repeat in range(args.trials):
            modes = ['rows', 'contiguous', 'direct']
            if (case + repeat) % 2:
                modes.reverse()
            for mode in modes:
                command = [str(binary), str(width), str(height), str(args.frames), str(sessions), mode]
                row = json.loads(subprocess.check_output(command, text=True, timeout=120))
                row.update(width=width, height=height, sessions=sessions, frames=args.frames, mode=mode, trial=repeat)
                result['trials'].append(row)
                args.output.write_text(json.dumps(result, indent=2) + '\n')
        print(f'T389 raw input: case {case + 1}/{len(cases)}', flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--directory', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--trials', type=int, default=5)
    parser.add_argument('--frames', type=int, default=180)
    parser.add_argument('--build-only', action='store_true')
    parser.add_argument('--reuse', action='store_true')
    args = parser.parse_args()
    if args.trials < 1 or args.frames < 1:
        parser.error('positive trials and frames required')
    binary = args.directory / 'raw-input' if args.reuse else build(args.directory)
    if not args.build_only:
        run(args, binary)


if __name__ == '__main__':
    main()
