#!/usr/bin/env python3
"""T418 matched conversion/encode/USB/render-callback-ACK replay, not optical latency."""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import socket
import struct
import subprocess
import time
from profile_usb_wire import Acknowledgements, receipt

PACKAGE = 'com.uscreen.decoderbench.candidate'
APP = 'io.github.geraldo_netto.uscreen/com.uscreen.MainActivity'
CHOICE = dict(name='c2.unisoc.avc.decoder', stream=dict(codec='h264', profile='baseline', level=50, depth=8),
              low_latency=False, operating_rate=120)


def adb(args, *command):
    return subprocess.check_output(['adb', '-s', args.serial, *command], text=True, timeout=15)


def one(args, repeat, mode):
    prefix = args.output / f'{repeat}-{mode}'
    metadata = dict(width=1280, height=800, fps=60, mime='video/avc',
                    selection=dict(decoder_protocol=2, decoder_selection=CHOICE))
    with socket.socket() as listener:
        listener.bind(('127.0.0.1', 0))
        listener.listen(1)
        listener.settimeout(10)
        port = listener.getsockname()[1]
        route = f'tcp:{port}'
        adb(args, 'reverse', '--no-rebind', route, route)
        try:
            adb(args, 'shell', 'run-as', PACKAGE, 'rm', '-f', 'files/result.json')
            launch = adb(args, 'shell', 'am', 'start', '-S', '-W', '-n',
                f'{PACKAGE}/com.uscreen.benchmark.MainActivity', '--ez', 'run', 'true', '--ei', 'usb_port', str(port))
            prefix.with_suffix('.launch').write_text(launch)
            if 'Status: ok' not in launch:
                raise RuntimeError('benchmark activity did not launch')
            connection, _ = listener.accept()
            with connection:
                connection.settimeout(10)
                connection.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
                data = json.dumps(metadata, separators=(',', ':')).encode('ascii')
                connection.sendall(struct.pack('!H', len(data)) + data)
                acks = Acknowledgements(connection, receipt(CHOICE))
                acks.thread.start()
                try:
                    acks.wait_ready(1)
                    command = [str(args.directory / 'target/release/uscreen-shared-encode-bench'),
                        str(args.directory / 'producer'), mode, '1280', '800', str(args.frames),
                        str(args.workers), str(prefix.with_suffix('.h264')), str(connection.fileno())]
                    with prefix.with_suffix('.log').open('w') as log:
                        output = subprocess.check_output(command, pass_fds=(connection.fileno(),),
                            stderr=log, text=True, timeout=25)
                    time.sleep(1)
                    host = json.loads(output)
                    android = json.loads(adb(args, 'shell', 'run-as', PACKAGE, 'cat', 'files/result.json'))
                    result = dict(repeat=repeat, mode=mode, host=host, acks=acks.rows, setups=acks.ready,
                                  android=android, error=acks.failure)
                    prefix.with_suffix('.json').write_text(json.dumps(result, indent=2) + '\n')
                    measured = {row['sequence'] + 1 for row in host['frames']}
                    received = {row['sequence'] for row in acks.rows}
                    if not measured.issubset(received):
                        raise RuntimeError('missing ACK for measured frame')
                    if not android.get('completed') or acks.failure:
                        raise RuntimeError(f'Incomplete replay: {android.get("error", acks.failure)}')
                finally:
                    acks.stopping = True
                    connection.shutdown(socket.SHUT_RDWR)
                    acks.thread.join(timeout=2)
        finally:
            adb(args, 'reverse', '--remove', route)
    print(repeat, mode, len(acks.rows), 'ACKs', flush=True)


def validate_bounds(args):
    return 1 <= args.frames <= 600 and 1 <= args.workers <= 128 and 1 <= args.repeats <= 10


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--serial', required=True)
    parser.add_argument('--directory', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--frames', type=int, default=240)
    parser.add_argument('--workers', type=int, default=30)
    parser.add_argument('--repeats', type=int, default=5)
    args = parser.parse_args()
    if not validate_bounds(args):
        parser.error('bounded frame, worker and repeat counts required')
    args.output.mkdir()
    foreground = adb(args, 'shell', 'dumpsys', 'activity', 'activities')
    if not any('topResumedActivity=' in line and APP in line for line in foreground.splitlines()):
        raise RuntimeError('UScreen must already be visible and unlocked')
    before = adb(args, 'reverse', '--list')
    (args.output / 'routes-before.txt').write_text(before)
    try:
        for repeat in range(args.repeats):
            for mode in (['fifo', 'shared'] if repeat % 2 == 0 else ['shared', 'fifo']):
                one(args, repeat, mode)
    finally:
        # No force-stop of UScreen, power key, lock action, ADB reset or display detach.
        adb(args, 'shell', 'am', 'start', '-W', '-n', APP)
        after = adb(args, 'reverse', '--list')
        (args.output / 'routes-after.txt').write_text(after)
        if after != before:
            raise RuntimeError('ADB routes changed during replay')


if __name__ == '__main__':
    main()
