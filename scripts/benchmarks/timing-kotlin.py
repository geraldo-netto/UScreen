#!/usr/bin/env python3
"""T404: host-JVM timing bookkeeping replay; no MediaCodec or Android power claim."""
import argparse
import hashlib
import itertools
import json
import os
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'scripts/complexity'))
import kotlin  # Reuse the pinned/hash-checked compiler bootstrap.
SOURCE = 'android/app/src/main/java/com/uscreen/VideoTiming.kt'
BASE = '0533724'


def prepare(folder, variant):
    folder.mkdir(parents=True)
    text = (subprocess.check_output(['git', 'show', f'{BASE}:{SOURCE}'], cwd=ROOT, text=True)
            if variant == 'original' else (ROOT / SOURCE).read_text())
    if variant == 'scan':
        text = text.replace('            lookup[seq and (ARRIVAL_RING - 1)] = i\n', '')
        text = text.replace('        val cached = lookup[seq and (ARRIVAL_RING - 1)]\n', '')
        text = text.replace('        if (cached >= 0 && valid[cached] && arrivalSeq[cached] == seq) return cached\n', '')
    (folder / 'VideoTiming.kt').write_text(text)
    (folder / 'Log.kt').write_text('package android.util\nobject Log { @JvmStatic fun i(tag: String, message: String): Int = 0 }\n')
    (folder / 'VideoReceiver.kt').write_text('package com.uscreen\nclass VideoReceiver { companion object { const val ARRIVAL_RING = 64; const val TAG = "test" } }\n')
    (folder / 'Timing.kt').write_bytes((ROOT / 'scripts/benchmarks/timing.kt').read_bytes())


def build(folder, variant, classpath):
    prepare(folder, variant)
    jar = folder / 'timing.jar'
    command = ['java', '-cp', classpath, 'org.jetbrains.kotlin.cli.jvm.K2JVMCompiler', '-no-stdlib',
               '-no-reflect', '-cp', classpath, '-d', str(jar), *map(str, sorted(folder.glob('*.kt'))) ]
    subprocess.run(command, check=True)
    return jar


def run(args, jars, classpath):
    result = dict(baseline=BASE, java=subprocess.check_output(['java', '-version'], stderr=subprocess.STDOUT, text=True).strip(),
                  kotlin='1.9.20', count=args.count,
                  sources={name: hashlib.sha256((jar.parent / 'VideoTiming.kt').read_bytes()).hexdigest() for name, jar in jars.items()}, trials=[])
    checksums = {}
    for number, (sessions, mode) in enumerate(itertools.product([1, 2, 4], ['latest', 'delay8', 'delay32', 'missing', 'sparse'])):
        for trial in range(args.trials):
            order = list(jars) if (number + trial) % 2 else list(reversed(jars))
            for variant in order:
                command = ['java', '-cp', f'{jars[variant]}{os.pathsep}{classpath}', 'com.uscreen.TimingKt', str(sessions), str(args.count), mode]
                output = subprocess.check_output(command, text=True, timeout=60)
                lanes = [list(map(int, line.split('\t'))) for line in output.splitlines()]
                signature = [row[3] for row in lanes]
                key = (sessions, mode)
                if key in checksums and checksums[key] != signature:
                    raise RuntimeError(f'timing result mismatch: {key}: {variant}')
                checksums[key] = signature
                result['trials'].append(dict(variant=variant, sessions=sessions, mode=mode, trial=trial, lanes=lanes))
            args.output.write_text(json.dumps(result, indent=2) + '\n')
        print(f'T404 JVM: case {number + 1}/15', flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--directory', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--trials', type=int, default=5)
    parser.add_argument('--count', type=int, default=200000)
    parser.add_argument('--build-only', action='store_true')
    parser.add_argument('--reuse', action='store_true')
    args = parser.parse_args()
    if args.count < 1 or args.trials < 1:
        parser.error('count and trials must be positive')
    classpath = os.pathsep.join(kotlin.artifact(kotlin.cache_dir(), key, digest) for key, digest in kotlin.ARTIFACTS.items())
    jars = {name: args.directory / name / 'timing.jar' for name in ['original', 'scan', 'candidate']}
    if not args.reuse:
        jars = {name: build(args.directory / name, name, classpath) for name in jars}
    if not args.build_only:
        run(args, jars, classpath)


if __name__ == '__main__':
    main()
