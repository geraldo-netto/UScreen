#!/usr/bin/env python3
"""T382: measure installed UScreen; never install, reconfigure or attach EVDI."""
import argparse
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
    valid = [line for line in monitors.splitlines() if position in line and '1280/' in line]
    if code or not valid or '*' in valid[0]:
        raise ValueError('Target must be an existing non-primary 1280x800 monitor')
    return monitors


def metadata(args, monitors):
    code, pid, _ = command(['adb', '-s', args.serial, 'shell', 'pidof', 'com.uscreen'])
    if code or not pid.isdigit():
        raise ValueError('UScreen must already be running on the tablet')
    scripts = Path(__file__).parent
    result = dict(start_utc=time.time(), geometry=args.geometry, monitors=monitors,
                  host_ticks_per_second=os.sysconf('SC_CLK_TCK'), android_ticks_per_second=100,
                  android_page_size=4096, android_pid=int(pid), plan=phases(args.seconds, args.warmup),
                  source_commit=command(['git', 'rev-parse', 'HEAD'])[1],
                  scripts={p.name: hashlib.sha256(p.read_bytes()).hexdigest()
                           for p in scripts.glob('*.py')})
    result['host_kernel'] = command(['uname', '-srvm'])[1]
    result['android_build'] = command(['adb', '-s', args.serial, 'shell', 'getprop', 'ro.build.fingerprint'])[1]
    return result


def start_logs(args, meta):
    host_pattern = re.compile(r'Latency |of which tablet|Encoder: \d|evdi-helper.*(grabs/s|cycle:|capture|Incomplete|Mode:)|FIFO_RESET|Client lagged|Capture manager failed')
    android_pattern = re.compile(r'on-device split:|Control statistics:|Codec configured|Decoder stuck|Decoder took|Output thread:')
    return [filtered_logs(['journalctl', '--user', '-u', 'uscreen', '-f', '-n', '0', '-o', 'json'],
                          args.output / 'host-windows.jsonl', host_pattern, True),
            filtered_logs(['adb', '-s', args.serial, 'logcat', f'--pid={meta["android_pid"]}',
                           '-v', 'epoch', '-T', '1'], args.output / 'android.log', android_pattern)]


def retire_logs(logs):
    for process, thread in logs:
        process.terminate()
        try:
            process.wait(timeout=3)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()
        thread.join(timeout=3)


def main():
    args = arguments()
    monitors = ensure_target(args.geometry)
    args.output.mkdir(parents=True, exist_ok=False)
    meta = metadata(args, monitors)
    (args.output / 'metadata.json').write_text(json.dumps(meta, indent=2) + '\n')
    state, stop = {}, threading.Event()
    logs = start_logs(args, meta)
    extra = [(os.getpid(), 'benchmark-workload-and-observer')]
    for name in ['cinnamon', 'Xorg']:
        extra.extend((int(pid), name) for pid in command(['pgrep', '-x', name])[1].split())
    sampler = Sampler(args.serial, args.output, state, stop, extra)
    thread = threading.Thread(target=sampler.run)
    with (args.output / 'phases.jsonl').open('w') as events:
        def event(value):
            events.write(json.dumps(dict(utc=time.time(), monotonic=time.monotonic(), **value)) + '\n')
            events.flush()
            print(json.dumps(value), flush=True)
        work = Workload(args.geometry, meta['plan'], state, event)
        signal.signal(signal.SIGTERM, lambda *_: work.root.destroy())
        thread.start()
        try:
            work.run()
        finally:
            stop.set()
            thread.join(timeout=45)
            retire_logs(logs)


if __name__ == '__main__':
    main()
