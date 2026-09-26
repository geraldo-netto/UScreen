#!/usr/bin/env python3
"""T589: bounded ART packet-storage workloads in the separate Blent profile APK."""
import argparse
import json
from pathlib import Path
import subprocess
import time

PACKAGE = 'io.github.geraldo_netto.blent.profile'


def adb(serial, *args):
    return subprocess.check_output(['adb', '-s', serial, *args], text=True, timeout=20)


def difference(before, after, key):
    # Missing, histogram-shaped or reset counters are unknown, never zero.
    left, right = before.get(key, ''), after.get(key, '')
    if not str(left).isdigit() or not str(right).isdigit():
        return None
    return int(right) - int(left) if int(right) >= int(left) else None


def summarize(row):
    before, after = row['before'], row['after']
    settled = row['settled_after']
    return dict(name=row['name'], packets=row['packets'], elapsed_ns=row['elapsed_ns'],
        storage_replacements=row.get('storage_replacements'), retained_capacity=row.get('retained_capacity'),
        allocated_bytes=difference(before, settled, 'art.gc.bytes-allocated'),
        workload_gc_count=difference(before, after, 'art.gc.gc-count'),
        workload_gc_time_ms=difference(before, after, 'art.gc.gc-time'),
        workload_blocking_gc_time_ms=difference(before, after, 'art.gc.blocking-gc-time'),
        boundary_gc_count=difference(after, settled, 'art.gc.gc-count'))


def await_result(serial):
    deadline = time.monotonic() + 60
    while time.monotonic() < deadline:
        result = subprocess.run(['adb', '-s', serial, 'shell', 'run-as', PACKAGE,
                                 'cat', 'files/allocations.json'], capture_output=True, text=True, timeout=10)
        if result.returncode == 0:
            data = json.loads(result.stdout)
            if 'error' in data:
                raise RuntimeError(data['error'])
            return data
        time.sleep(.5)
    raise TimeoutError('allocation workload did not complete within 60 seconds')


def run(args):
    args.output.mkdir()
    apk = adb(args.serial, 'shell', 'pm', 'path', PACKAGE).strip().removeprefix('package:')
    metadata = dict(fingerprint=adb(args.serial, 'shell', 'getprop', 'ro.build.fingerprint').strip(),
                    apk=adb(args.serial, 'shell', 'sha256sum', apk).split()[0], trials=args.trials)
    (args.output / 'metadata.json').write_text(json.dumps(metadata, indent=2) + '\n')
    rows = []
    for trial in range(args.trials):
        adb(args.serial, 'shell', 'am', 'force-stop', PACKAGE)
        adb(args.serial, 'shell', 'run-as', PACKAGE, 'rm', '-f', 'files/allocations.json')
        adb(args.serial, 'shell', 'am', 'start', '-W', '-n', PACKAGE + '/com.blent.AllocationProfileActivity')
        data = await_result(args.serial)
        (args.output / f'{trial}.json').write_text(json.dumps(data, indent=2) + '\n')
        pid = adb(args.serial, 'shell', 'pidof', PACKAGE).strip()
        (args.output / f'{trial}-logcat.txt').write_text(adb(args.serial, 'logcat', '-d', '--pid=' + pid))
        rows.append(dict(trial=trial, source=data['source'], workloads=[summarize(row) for row in data['workloads']]))
        print('trial', trial, 'complete', flush=True)
    (args.output / 'summary.json').write_text(json.dumps(rows, indent=2) + '\n')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--serial', required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--trials', type=int, choices=range(1, 6), default=3)
    run(parser.parse_args())


if __name__ == '__main__':
    main()
