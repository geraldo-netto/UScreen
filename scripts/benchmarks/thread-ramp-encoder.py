#!/usr/bin/env python3
"""T600: libx264 thread ramp, paced raw input through USB decoder render ACKs."""
import argparse
import json
import mmap
import os
from pathlib import Path
import socket
import struct
import subprocess
import time
import profile_usb_pipeline as P
from profile_usb_wire import Acknowledgements, receipt
import thread_ramp_common as C

USB = P.module('shared-usb')
META = dict(width=1280, height=800, fps=60, frames=360)


def connect(args, listener, port, folder):
    USB.adb(args, 'shell', 'run-as', USB.PACKAGE, 'rm', '-f', 'files/result.json')
    launch = USB.adb(args, 'shell', 'am', 'start', '-S', '-W', '-n',
                     f'{USB.PACKAGE}/com.blent.benchmark.MainActivity', '--ez', 'run', 'true', '--ei', 'usb_port', str(port))
    (folder / 'launch.txt').write_text(launch)
    if 'Status: ok' not in launch:
        raise RuntimeError('benchmark activity did not launch')
    connection, _ = listener.accept()
    connection.settimeout(10)
    connection.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
    data = json.dumps(dict(META, mime='video/avc', selection=dict(decoder_protocol=2, decoder_selection=USB.CHOICE))).encode('ascii')
    connection.sendall(struct.pack('!H', len(data)) + data)
    return connection


def encode(args, workers, connection, acks, folder):
    profile = json.loads(args.policies.read_text())['libx264']
    command = P.command(profile, META, '/dev/dri/renderD129')
    command[0] = str(args.ffmpeg)
    command[command.index('error')] = 'info'
    at = command.index('-map')
    command[at:at] = ['-threads:v', str(workers)]
    resources = '{"user_seconds":%U,"system_seconds":%S,"rss_peak_kib":%M,"voluntary_switches":%w,"involuntary_switches":%c}'
    command = ['/usr/bin/time', '-o', str(folder / 'resources.json'), '-f', resources, *command]
    C.save(folder / 'command.json', command)
    with args.source.open('rb') as source, mmap.mmap(source.fileno(), 0, access=mmap.ACCESS_READ) as raw, \
            (folder / 'encoder.log').open('wb') as log, (folder / 'encoded.h264').open('wb') as encoded:
        process = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=log, bufsize=0, start_new_session=True)
        try:
            with memoryview(raw) as view:
                result = P.pump(process, view, META, connection, acks, 0, encoded)
        finally:
            if process.poll() is None:
                os.killpg(process.pid, __import__('signal').SIGKILL)
            process.wait(timeout=5)
            process.stdin.close()
            process.stdout.close()
    result['resources'] = json.loads((folder / 'resources.json').read_text())
    return result


def exchange(args, workers, connection, folder):
    acks = Acknowledgements(connection, receipt(USB.CHOICE))
    acks.thread.start()
    try:
        acks.wait_ready(1)
        result = encode(args, workers, connection, acks, folder)
        time.sleep(.5)
        connection.sendall(struct.pack('!I', 2))
        time.sleep(.5)
        result.update(acknowledgements=acks.rows, setups=acks.ready, error=acks.failure)
        result['android'] = json.loads(USB.adb(args, 'shell', 'run-as', USB.PACKAGE, 'cat', 'files/result.json'))
        C.save(folder / 'result.json', result)
        if acks.failure or not result['android'].get('completed'):
            raise RuntimeError('decoder replay failed; inspect retained result')
        return result
    finally:
        acks.stopping = True
        connection.shutdown(socket.SHUT_RDWR)
        acks.thread.join(timeout=3)


def trial(args, repeat, workers):
    folder = args.output / f'{repeat}-{workers}'
    folder.mkdir()
    with socket.socket() as listener:
        listener.bind(('127.0.0.1', 0))
        listener.listen(1)
        listener.settimeout(10)
        port = listener.getsockname()[1]
        route = f'tcp:{port}'
        USB.adb(args, 'reverse', '--no-rebind', route, route)
        try:
            with connect(args, listener, port, folder) as connection:
                result = exchange(args, workers, connection, folder)
        finally:
            USB.adb(args, 'reverse', '--remove', route)
    print('encoder', repeat, workers, len(result['acknowledgements']), 'ACKs', flush=True)


def run(args):
    assert args.source.stat().st_size == META['width'] * META['height'] * 3 // 2 * META['frames']
    foreground = USB.adb(args, 'shell', 'dumpsys', 'activity', 'activities')
    assert any('topResumedActivity=' in line and USB.APP in line for line in foreground.splitlines())
    args.output.mkdir(parents=True)
    paths = [Path(__file__), Path(C.__file__), Path(P.__file__), args.source, args.policies, args.ffmpeg]
    info = C.metadata(paths)
    info.update(workload=META, warmup_frames=60, cooldown_frames=60,
                ffmpeg=subprocess.check_output([str(args.ffmpeg), '-version'], text=True),
                boundary='raw-input admission to USB render callback ACK; no capture/compositor/optical latency; only encoder threads overridden')
    C.save(args.output / 'metadata.json', info)
    before = USB.adb(args, 'reverse', '--list')
    (args.output / 'routes-before.txt').write_text(before)
    try:
        for repeat, workers in C.order():
            trial(args, repeat, workers)
    finally:
        USB.adb(args, 'shell', 'am', 'start', '-W', '-n', USB.APP)
        after = USB.adb(args, 'reverse', '--list')
        (args.output / 'routes-after.txt').write_text(after)
        assert before == after, 'replay changed existing ADB routes'


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--serial', required=True)
    for name in ['source', 'policies', 'ffmpeg', 'output']:
        parser.add_argument('--' + name, type=Path, required=True)
    run(parser.parse_args())
