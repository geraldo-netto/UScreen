#!/usr/bin/env python3
"""T382: measure installed UScreen; never install, reconfigure or attach EVDI."""
import argparse
from contextlib import ExitStack
import hashlib
import json
import os
from pathlib import Path
import re
import signal
import subprocess
import threading
import time

from observe import Sampler, command, filtered_logs
from workload import Workload, phases
from android_session import AndroidSessionMonitor


def arguments():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--serial', required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--geometry', default='1280x800+3840+0')
    parser.add_argument('--seconds', type=float, default=300)
    parser.add_argument('--warmup', type=float, default=60)
    return parser.parse_args()


def ensure_target(geometry):
    match = re.fullmatch(r'1280x800\+(\d+)\+(\d+)', geometry)
    if not match:
        raise ValueError('This workload requires a 1280x800 target geometry')
    code, monitors, _ = command(['xrandr', '--listmonitors'])
    position = f'+{match[1]}+{match[2]}'
    target = re.compile(rf'\s1280/\d+x800/\d+{re.escape(position)}(?:\s|$)')
    valid = [line for line in monitors.splitlines() if target.search(line)]
    if code or not valid or '*' in valid[0]:
        raise ValueError('Target must be an existing non-primary 1280x800 monitor')
    return monitors


def metadata(args, monitors):
    code, pid, _ = command(['adb', '-s', args.serial, 'shell', 'pidof', 'io.github.geraldo_netto.uscreen'])
    if code or not pid.isdigit():
        raise ValueError('UScreen must already be running on the tablet')
    scripts = Path(__file__).parent
    result = dict(start_utc=time.time(), geometry=args.geometry, monitors=monitors, visibility_guard_version=1,
                  observation_guard_version=1,
                  host_ticks_per_second=os.sysconf('SC_CLK_TCK'), **android_units(args.serial),
                  android_pid=int(pid), plan=phases(args.seconds, args.warmup),
                  source_commit=command(['git', 'rev-parse', 'HEAD'])[1],
                  scripts={p.name: hashlib.sha256(p.read_bytes()).hexdigest()
                           for p in scripts.glob('*.py')})
    result['host_kernel'] = command(['uname', '-srvm'])[1]
    result['android_build'] = command(['adb', '-s', args.serial, 'shell', 'getprop', 'ro.build.fingerprint'])[1]
    return result


def android_unit(serial, name):
    code, output, _ = command(['adb', '-s', serial, 'shell', 'getconf', name])
    if code or not re.fullmatch(r'[0-9]+', output):
        raise ValueError(f'Cannot verify Android {name}; getconf must return a positive integer')
    value = int(output)
    if value <= 0:
        raise ValueError(f'Invalid Android {name}: must be positive')
    return value


def android_units(serial):
    ticks = android_unit(serial, 'CLK_TCK')
    page_size = android_unit(serial, 'PAGESIZE')
    if page_size & (page_size - 1):
        raise ValueError('Invalid Android PAGESIZE: must be a power of two')
    return dict(android_ticks_per_second=ticks, android_page_size=page_size)


def start_logs(args, meta):
    host_pattern = re.compile(r'Latency |of which tablet|Encoder: \d|evdi-helper.*(grabs/s|cycle:|capture|Incomplete|Mode:)|FIFO_RESET|Client lagged|Capture manager failed')
    android_pattern = re.compile(r'on-device split:|Control statistics:|Codec configured|Decoder stuck|Decoder took|Output thread:')
    logs = []
    try:
        logs.append(filtered_logs(['journalctl', '--user', '-u', 'uscreen', '-f', '-n', '0', '-o', 'json'],
                                  args.output / 'host-windows.jsonl', host_pattern, True))
        logs.append(filtered_logs(['adb', '-s', args.serial, 'logcat', f'--pid={meta["android_pid"]}',
                                   '-v', 'epoch', '-T', '1'], args.output / 'android.log', android_pattern))
    except BaseException:
        retire_logs(logs)
        raise
    return logs


def retire_logs(logs):
    for collector in logs:
        collector.close()


def retire_sampler(sampler, thread, stop):
    stop.set()
    if thread.ident is not None:
        thread.join(timeout=45)
    if not thread.is_alive():
        sampler.file.close()
    else:
        sampler.failure = 'sampler did not stop within its shutdown deadline'


def observation_problem(observers):
    for observer in [observers['monitor'], observers['sampler'], *observers['logs']]:
        problem = observer.problem()
        if problem:
            return problem
    return None


def observer_report(folder, observers):
    errors = [observer.failure for observer in [observers.get('monitor'), observers.get('sampler'), *observers.get('logs', [])]
              if observer is not None and observer.failure]
    if not observers.get('complete'):
        errors.append('workload did not complete')
    receipts = {collector.path.name: collector.receipts for collector in observers.get('logs', [])}
    (folder / 'observer.json').write_text(json.dumps(dict(complete=not errors, errors=errors, log_receipts=receipts), indent=2) + '\n')


def run_workload(args, meta, state, thread, resources, observers):
    events = resources.enter_context((args.output / 'phases.jsonl').open('w'))

    def event(value):
        if value['event'] == 'invalid':
            (args.output / 'invalid.json').write_text(json.dumps(value, indent=2) + '\n')
        if value['event'] == 'complete':
            observers['complete'] = True
        events.write(json.dumps(dict(utc=time.time(), monotonic=time.monotonic(), **value)) + '\n')
        events.flush()
        print(json.dumps(value), flush=True)

    work = Workload(args.geometry, meta['plan'], state, event)
    monitor = AndroidSessionMonitor(args.serial, meta['android_pid'], args.output)
    observers['monitor'] = monitor
    resources.callback(monitor.close)
    work.observation_problem = lambda: observation_problem(observers)
    previous = signal.signal(signal.SIGTERM, lambda *_: work.invalidate('terminated by SIGTERM'))
    resources.callback(signal.signal, signal.SIGTERM, previous)
    monitor.start()
    thread.start()
    try:
        work.run()
    except BaseException as error:
        event(dict(event='invalid', reason=f'workload interrupted: {type(error).__name__}'))
        raise


def main():
    args = arguments()
    monitors = ensure_target(args.geometry)
    args.output.mkdir(parents=True, exist_ok=False)
    meta = metadata(args, monitors)
    (args.output / 'metadata.json').write_text(json.dumps(meta, indent=2) + '\n')
    state, stop = {}, threading.Event()
    with ExitStack() as resources:
        observers = {}
        resources.callback(observer_report, args.output, observers)
        logs = start_logs(args, meta)
        observers['logs'] = logs
        resources.callback(retire_logs, logs)
        extra = [(os.getpid(), 'benchmark-workload-and-observer')]
        for name in ['cinnamon', 'Xorg']:
            extra.extend((int(pid), name) for pid in command(['pgrep', '-x', name])[1].split())
        sampler = Sampler(args.serial, args.output, state, stop, extra,
                          android_page_size=meta['android_page_size'])
        observers['sampler'] = sampler
        thread = threading.Thread(target=sampler.run)
        resources.callback(retire_sampler, sampler, thread, stop)
        run_workload(args, meta, state, thread, resources, observers)


if __name__ == '__main__':
    main()
