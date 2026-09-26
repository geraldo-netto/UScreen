#!/usr/bin/env python3
"""T607: native ART camera chunk sweep; profile APK only, no sensors."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import statistics
import subprocess
import time

ROOT = Path(__file__).resolve().parents[2]
PACKAGE = 'io.github.geraldo_netto.blent.profile'


def body(text, signature):
    start = text.index('{', text.index(signature)) + 1
    end = text.index('\n    }', start)
    return re.sub(r'\s+', '', re.sub(r'//[^\n]*', '', text[start:end]))


def verify_source():
    production = ROOT / 'android/app/src/main/java/com/blent/CameraWire.kt'
    benchmark = ROOT / 'scripts/benchmarks/android-allocations/CameraChunkProfileActivity.kt'
    expected = body(production.read_text(), 'fun packet(').replace('+8192', '+chunk')
    assert expected == body(benchmark.read_text(), 'fun packet('), 'packet algorithm drift'
    return {str(p.relative_to(ROOT)): hashlib.sha256(p.read_bytes()).hexdigest() for p in (production, benchmark)}


def adb(serial, *args):
    return subprocess.check_output(['adb', '-s', serial, *args], text=True, timeout=20).strip()


def wait_result(serial):
    deadline = time.monotonic() + 180
    while time.monotonic() < deadline:
        result = subprocess.run(['adb', '-s', serial, 'shell', 'run-as', PACKAGE, 'cat', 'files/camera-chunks.json'],
                                capture_output=True, text=True, timeout=10)
        if result.returncode == 0:
            data = json.loads(result.stdout)
            assert 'error' not in data, data
            return data
        time.sleep(1)
    raise TimeoutError('chunk sweep exceeded three minutes')


def counter(row, key, after='settled_after'):
    before, final = row['before'].get(key), row[after].get(key)
    if before is None or final is None:
        return None
    return int(final) - int(before)


def summary(data):
    groups = {}
    for row in data['rows']:
        row['allocated_bytes'] = counter(row, 'art.gc.bytes-allocated')
        row['gc_count'] = counter(row, 'art.gc.gc-count', 'after')
        row['gc_time_ms'] = counter(row, 'art.gc.gc-time', 'after')
        groups.setdefault(row['chunk'], []).append(row)
    keys = ['elapsed_ns', 'cpu_ns', 'allocated_bytes', 'gc_count', 'gc_time_ms', 'write_calls',
            'peak_staging_bytes', 'retained_buffer_bytes', 'wire_bytes']
    return {chunk: {key: statistics.median(r[key] for r in rows) for key in keys}
            for chunk, rows in sorted(groups.items())}


def run(args):
    hashes = verify_source()
    args.output.mkdir(parents=True)
    apk = adb(args.serial, 'shell', 'pm', 'path', PACKAGE).removeprefix('package:')
    metadata = dict(sources=hashes, fingerprint=adb(args.serial, 'shell', 'getprop', 'ro.build.fingerprint'),
                    apk_sha256=adb(args.serial, 'shell', 'sha256sum', apk).split()[0])
    adb(args.serial, 'shell', 'am', 'force-stop', PACKAGE)
    adb(args.serial, 'shell', 'run-as', PACKAGE, 'rm', '-f', 'files/camera-chunks.json')
    try:
        adb(args.serial, 'shell', 'am', 'start', '-W', '-n', PACKAGE + '/com.blent.CameraChunkProfileActivity')
        data = wait_result(args.serial)
        result = dict(metadata=metadata, measurements=data, summary=summary(data))
        (args.output / 'results.json').write_text(json.dumps(result, indent=2) + '\n')
        print(json.dumps(result['summary'], indent=2))
    finally:
        adb(args.serial, 'shell', 'am', 'force-stop', PACKAGE)
        adb(args.serial, 'shell', 'am', 'start', '-W', '-n', 'io.github.geraldo_netto.blent/com.blent.MainActivity')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--serial', required=True)
    parser.add_argument('--output', type=Path, required=True)
    run(parser.parse_args())


if __name__ == '__main__':
    main()
