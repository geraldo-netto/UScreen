#!/usr/bin/env python3
"""Run separate baseline/candidate decoder APKs; never replace the UScreen APK."""
import argparse
from datetime import datetime, timezone
import hashlib
import itertools
import json
from pathlib import Path
import subprocess
import time

VARIANTS = [('baseline', 'legacy'), ('candidate', 'legacy'), ('candidate', 'sync-normal'),
            ('candidate', 'callback-legacy'), ('candidate', 'callback-supported'),
            ('candidate', 'callback-supported1'), ('candidate', 'callback-unhinted')]
INPUT_VARIANTS = [('candidate', 'socket-heap'), ('candidate', 'socket-direct')]


def adb(serial, *command, **kwargs):
    return subprocess.run(['adb', '-s', serial, *command], timeout=30, check=True, **kwargs)


def capture(serial, *command):
    return adb(serial, *command, capture_output=True, text=True).stdout


def transfer(args, package, scene):
    source = getattr(args, scene)
    expected = hashlib.sha256(source.read_bytes()).hexdigest()
    with source.open('rb') as data:
        adb(args.serial, 'shell', '-T', 'run-as', package, 'tee', 'files/stream.bin', stdin=data, stdout=subprocess.DEVNULL)
    actual = capture(args.serial, 'shell', 'run-as', package, 'sha256sum', 'files/stream.bin').split()[0]
    if actual != expected:
        raise RuntimeError('fixture transfer hash mismatch')


def wait_result(args, package):
    deadline = time.monotonic() + args.seconds + args.warmup + 20
    while time.monotonic() < deadline:
        result = subprocess.run(['adb', '-s', args.serial, 'exec-out', 'run-as', package, 'cat', 'files/result.json'],
                                capture_output=True, text=True, timeout=10)
        if result.returncode == 0:
            try:
                return json.loads(result.stdout)
            except json.JSONDecodeError:
                pass  # Writer can still be finishing this one small result.
        time.sleep(1)
    raise TimeoutError('decoder activity did not finish')


def logs(args, package, folder):
    pid = capture(args.serial, 'shell', 'pidof', package).strip()
    if pid.isdigit():
        (folder / 'android.log').write_text(capture(args.serial, 'logcat', '-d', '--pid', pid, '-v', 'threadtime', '-s',
                                                   'UScreenDecoderBench:I', 'AndroidRuntime:E'))


def trial(args, variant, profile, scene, rate, number):
    folder = args.output / f'{scene}-{rate}-{number}-{variant}-{profile}'
    folder.mkdir()
    package = f'com.uscreen.decoderbench.{variant}'
    transfer(args, package, scene)
    adb(args.serial, 'shell', 'run-as', package, 'rm', '-f', 'files/result.json', capture_output=True)
    (folder / 'battery-before.txt').write_text(capture(args.serial, 'shell', 'dumpsys', 'battery'))
    started = datetime.now(timezone.utc).isoformat()
    command = capture(args.serial, 'shell', 'am', 'start', '-S', '-W', '-n', f'{package}/com.uscreen.benchmark.MainActivity',
                      '--ez', 'run', 'true', '--es', 'profile', profile, '--ei', 'rate', str(rate),
                      '--ei', 'seconds', str(args.seconds), '--ei', 'warmup', str(args.warmup))
    (folder / 'launch.txt').write_text(command)
    if 'Status: ok' not in command:
        raise RuntimeError(f'activity launch failed: {command}')
    result = wait_result(args, package)
    result.update(variant=variant, scene=scene, trial=number, host_started_utc=started,
                  host_finished_utc=datetime.now(timezone.utc).isoformat())
    (folder / 'result.json').write_text(json.dumps(result, indent=2) + '\n')
    (folder / 'battery-after.txt').write_text(capture(args.serial, 'shell', 'dumpsys', 'battery'))
    logs(args, package, folder)
    if not result.get('completed'):
        raise RuntimeError(f'incomplete decoder trial; inspect {folder}')
    print(f'{scene} {rate}fps #{number}: {variant}/{profile}: {result["stats"]["rendered"]}/{result["sent"]} callbacks', flush=True)


def run(args):
    args.output.mkdir()
    metadata = dict(serial=args.serial, seconds=args.seconds, warmup=args.warmup, trials=args.trials, variants=args.variants,
                    adb=subprocess.check_output(['adb', 'version'], text=True),
                    fingerprint=capture(args.serial, 'shell', 'getprop', 'ro.build.fingerprint').strip())
    (args.output / 'metadata.json').write_text(json.dumps(metadata, indent=2) + '\n')
    for number in range(args.trials):
        for case, (scene, rate) in enumerate(itertools.product(['motion', 'static'], [60, 5])):
            order = args.variants if (number + case) % 2 else list(reversed(args.variants))
            for variant, profile in order:
                trial(args, variant, profile, scene, rate, number)
    # Complete runs return to the previously authorized UScreen session. An
    # interrupted/backgrounded trial aborts above and does not steal focus back.
    capture(args.serial, 'shell', 'am', 'start', '-n', 'com.uscreen/.MainActivity')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--serial', required=True)
    parser.add_argument('--motion', type=Path, required=True)
    parser.add_argument('--static', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--seconds', type=int, default=30)
    parser.add_argument('--warmup', type=int, default=5)
    parser.add_argument('--trials', type=int, default=3)
    parser.add_argument('--variant', action='append', choices=[f'{variant}/{profile}' for variant, profile in VARIANTS + INPUT_VARIANTS])
    args = parser.parse_args()
    if not 1 <= args.seconds <= 600 or not 0 <= args.warmup <= 60 or args.trials < 1:
        parser.error('invalid replay duration/trial count')
    args.variants = [tuple(item.split('/')) for item in args.variant] if args.variant else VARIANTS
    run(args)


if __name__ == '__main__':
    main()
