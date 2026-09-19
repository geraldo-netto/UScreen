#!/usr/bin/env python3
"""T419 short, foreground-guarded local rectangle/hardware-video comparisons."""
import argparse
import base64
import hashlib
import json
from pathlib import Path
import subprocess
import time
import observe
from rect_surface import PresentationSampler

PACKAGE = 'com.uscreen.rectbench'
SERVICES = ['io.github.geraldo_netto.uscreen', 'media.unisoc.codec2', 'surfaceflinger', 'android.hardware.graphics.composer@2.4-service']


def adb(args, *command, **kwargs):
    return subprocess.run(['adb', '-s', args.serial, *command], check=True, timeout=40, **kwargs)


def capture(args, *command):
    return adb(args, *command, capture_output=True, text=True).stdout


def foreground(args):
    lines = capture(args, 'shell', 'dumpsys', 'activity', 'activities').splitlines()
    top = [line for line in lines if 'topResumedActivity=' in line]
    if len(top) != 1 or 'io.github.geraldo_netto.uscreen/com.uscreen.MainActivity' not in top[0]:
        raise RuntimeError('UScreen is not foreground; do not interrupt another app')


def upload(args, source, target):
    with source.open('rb') as data:
        adb(args, 'shell', '-T', 'run-as', PACKAGE, 'tee', 'files/' + target,
            stdin=data, stdout=subprocess.DEVNULL)
    actual = capture(args, 'shell', 'run-as', PACKAGE, 'sha256sum', 'files/' + target).split()[0]
    assert actual == hashlib.sha256(source.read_bytes()).hexdigest(), 'fixture transfer mismatch'


def process_ids(args):
    rows = capture(args, 'shell', 'ps', '-A', '-o', 'PID,NAME').splitlines()[1:]
    names = {line.split()[1]: int(line.split()[0]) for line in rows}
    return {name: names[name] for name in [PACKAGE, *SERVICES]}


def sample(args, pids):
    rows = {}
    for name, pid in pids.items():
        command = ['cat', f'/proc/{pid}/stat']
        if name in (PACKAGE, 'io.github.geraldo_netto.uscreen'):
            command = ['run-as', name, *command]
        begin = time.monotonic_ns()
        raw = capture(args, 'shell', *command)
        end = time.monotonic_ns()
        rows[name] = dict(raw=raw, at_ns=(begin + end) // 2, cost_ns=end - begin,
                          **observe.process_stat(raw, args.page_size))
    return rows


def memory(args, folder, number, pids):
    for name, pid in pids.items():
        raw = capture(args, 'shell', 'dumpsys', 'meminfo', '--local', str(pid))
        (folder / f'memory-{number}-{name}.txt').write_text(raw)


def result(args):
    reply = subprocess.run(['adb', '-s', args.serial, 'exec-out', 'run-as', PACKAGE,
                            'cat', 'files/result.json'], capture_output=True, text=True, timeout=10)
    if reply.returncode:
        return None
    try:
        return json.loads(reply.stdout)
    except json.JSONDecodeError:
        return None


def wait(args, folder):
    pids = process_ids(args)
    deadline = time.monotonic() + args.seconds + args.warmup + args.sample_period + 12
    count = 0
    with (folder / 'resources.jsonl').open('w') as output:
        while time.monotonic() < deadline:
            completed = result(args)
            if completed is not None:
                return completed
            output.write(json.dumps(sample(args, pids)) + '\n'); output.flush()
            if count in (2, 7):
                memory(args, folder, count, pids)
            count += 1
            time.sleep(args.sample_period)
    raise TimeoutError('replay did not finish within its bounded window')


def trial(args, scene, rate, codec, number):
    foreground(args)
    folder = args.output / f'{number:03d}-{scene}-{rate}-{codec}'
    folder.mkdir()
    mode = 'video' if codec == 0 else 'rect'
    fixture = args.fixtures / (f'{scene}-{rate}.video' if codec == 0 else f'{scene}-{rate}-{codec}.rect')
    upload(args, fixture, 'stream.bin' if codec == 0 else 'rect.bin')
    foreground(args)
    adb(args, 'shell', 'run-as', PACKAGE, 'rm', '-f', 'files/result.json', capture_output=True)
    (folder / 'battery-before.txt').write_text(capture(args, 'shell', 'dumpsys', 'battery'))
    selection = dict(decoder_protocol=2, decoder_selection=dict(name='c2.unisoc.avc.decoder',
                     stream=dict(codec='h264', profile='constrained-baseline', level=40, depth=8),
                     low_latency=False, operating_rate=120))
    encoded = base64.b64encode(json.dumps(selection).encode()).decode()
    command = ['shell', 'am', 'start', *([] if args.keep_process else ['-S']), '-W', '-n', PACKAGE + '/com.uscreen.benchmark.MainActivity',
               '--ez', 'run', 'true', '--ez', 'rect', str(codec != 0).lower(), '--ez', 'verify', str(args.verify).lower(),
               '--ez', 'mapped', str(args.mapped).lower(),
               '--ei', 'seconds', str(args.seconds), '--ei', 'warmup', str(args.warmup), '--ei', 'rate', str(rate),
               '--es', 'selection', encoded]
    launch = capture(args, *command)
    (folder / 'launch.txt').write_text(launch)
    assert 'Status: ok' in launch
    observed = observed_trial(args, folder)
    observed.update(scene=scene, case=codec, fixture_sha256=hashlib.sha256(fixture.read_bytes()).hexdigest())
    (folder / 'result.json').write_text(json.dumps(observed, indent=2) + '\n')
    (folder / 'battery-after.txt').write_text(capture(args, 'shell', 'dumpsys', 'battery'))
    (folder / 'thermal-after.txt').write_text(capture(args, 'shell', 'dumpsys', 'thermalservice'))
    if args.keep_process:
        memory(args, folder, 'retired', process_ids(args))
    if not observed.get('completed'):
        raise RuntimeError(f'incomplete {mode} replay; inspect {folder}')
    if observed.get('presentation_error'):
        raise RuntimeError('presentation collection failed: ' + observed['presentation_error'])
    print(number, scene, mode, codec, observed.get('count', observed.get('stats', {}).get('rendered')), flush=True)


def observed_trial(args, folder):
    sampler = PresentationSampler(args.serial, PACKAGE, folder) if args.presentation else None
    if sampler:
        sampler.thread.start()
    try:
        observed = wait(args, folder)
    finally:
        if sampler:
            sampler.close()
    if sampler:
        observed['presentation_error'] = sampler.error
    return observed


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--serial', required=True)
    parser.add_argument('--fixtures', required=True, type=Path)
    parser.add_argument('--provenance', required=True, type=Path)
    parser.add_argument('--output', required=True, type=Path)
    parser.add_argument('--seconds', type=int, choices=range(1, 601), default=20)
    parser.add_argument('--sample-period', type=int, choices=[2, 10, 30], default=2)
    parser.add_argument('--warmup', type=int, choices=range(0, 6), default=4)
    parser.add_argument('--scenes', nargs='+', choices=['text', 'pen', 'motion', 'scroll', 'photo'], default=['text', 'pen', 'motion', 'scroll', 'photo'])
    parser.add_argument('--codecs', nargs='+', type=int, choices=[0, 1, 2], default=[0, 1, 2])
    parser.add_argument('--repeats', type=int, choices=[1, 2, 3], default=3)
    parser.add_argument('--verify', action='store_true')
    parser.add_argument('--mapped', action='store_true', help='map the complete local fixture; default uses a bounded read buffer')
    parser.add_argument('--keep-process', action='store_true', help='exercise repeated Activity/resource teardown without force-stopping this test APK')
    parser.add_argument('--text-rate', type=int, choices=[1, 5], default=5, help='1 requires a separate H.264-only idle control fixture')
    parser.add_argument('--presentation', action='store_true', help='collect the replay layer presentation ring in a separate matched cohort')
    args = parser.parse_args()
    if args.text_rate == 1 and args.codecs != [0]:
        parser.error('one-update/s control requires --codecs 0')
    run(args)


def run(args):
    foreground(args)
    provenance = json.loads(args.provenance.read_text())
    apk = capture(args, 'shell', 'pm', 'path', PACKAGE).strip().removeprefix('package:')
    assert capture(args, 'shell', 'sha256sum', apk).split()[0] == provenance['apk_sha256']
    args.output.mkdir()
    args.page_size = int(capture(args, 'shell', 'getconf', 'PAGESIZE'))
    meta = dict(provenance=provenance, page_size=args.page_size, clock_ticks=int(capture(args, 'shell', 'getconf', 'CLK_TCK')),
                sample_period=args.sample_period,
                seconds=args.seconds, warmup=args.warmup, verification=args.verify, mapped=args.mapped,
                keep_process=args.keep_process,
                text_rate=args.text_rate,
                presentation=args.presentation,
                fingerprint=capture(args, 'shell', 'getprop', 'ro.build.fingerprint').strip())
    (args.output / 'metadata.json').write_text(json.dumps(meta, indent=2) + '\n')
    adb(args, 'shell', 'run-as', PACKAGE, 'mkdir', '-p', 'files', capture_output=True)
    number = 0
    for repeat in range(args.repeats):
        for scene in args.scenes:
            for codec in (args.codecs if repeat % 2 == 0 else reversed(args.codecs)):
                trial(args, scene, args.text_rate if scene == 'text' else 60, codec, number)
                number += 1


if __name__ == '__main__':
    main()
