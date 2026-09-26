#!/usr/bin/env python3
"""T607: own and retire the ADB route for a byte-checked encoded replay."""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import signal
import subprocess
import time

ROOT = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location('chunks', ROOT / 'camera-chunks.py')
CHUNKS = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CHUNKS)
PACKAGE = CHUNKS.PACKAGE


def port_file(path, process):
    deadline = time.monotonic() + 10
    while not path.exists():
        assert process.poll() is None, 'receiver failed before readiness'
        if time.monotonic() >= deadline:
            raise TimeoutError('receiver not ready')
        time.sleep(.05)
    return int(path.read_text())


def cleanup(serial, remote, local, process):
    if remote:
        mappings = CHUNKS.adb(serial, 'reverse', '--list').splitlines()
        if any(row.split()[1:] == [remote, local] for row in mappings):
            CHUNKS.adb(serial, 'reverse', '--remove', remote)
    if process.poll() is None:
        os.killpg(process.pid, signal.SIGTERM)
        process.wait(timeout=5)
    CHUNKS.adb(serial, 'shell', 'am', 'force-stop', PACKAGE)
    CHUNKS.adb(serial, 'shell', 'rm', '-f', '/data/local/tmp/blent-t607-replay.bin')
    CHUNKS.adb(serial, 'shell', 'am', 'start', '-W', '-n', 'io.github.geraldo_netto.blent/com.blent.MainActivity')


def run(args):
    args.output.mkdir(parents=True)
    CHUNKS.verify_source()
    command = ['python3', str(ROOT / 'camera-socket.py'), '--fixture', str(args.fixture), '--output', str(args.output)]
    if args.trace:
        command = ['strace', '-c', '-e', 'trace=read,write,recvfrom,sendto', '-o', str(args.output / 'syscalls.txt')] + command
    process = subprocess.Popen(command, start_new_session=True)
    remote, local = None, None
    try:
        local = 'tcp:' + str(port_file(args.output / 'port', process))
        remote = 'tcp:' + CHUNKS.adb(args.serial, 'reverse', 'tcp:0', local)
        CHUNKS.adb(args.serial, 'push', str(args.fixture), '/data/local/tmp/blent-t607-replay.bin')
        CHUNKS.adb(args.serial, 'shell', 'run-as', PACKAGE, 'cp', '/data/local/tmp/blent-t607-replay.bin', 'files/camera-replay.bin')
        CHUNKS.adb(args.serial, 'shell', 'am', 'force-stop', PACKAGE)
        CHUNKS.adb(args.serial, 'shell', 'run-as', PACKAGE, 'rm', '-f', 'files/camera-chunks.json')
        CHUNKS.adb(args.serial, 'shell', 'am', 'start', '-W', '-n', PACKAGE + '/com.blent.CameraChunkProfileActivity', '--ei', 'port', remote[4:])
        data = CHUNKS.wait_result(args.serial)
        assert process.wait(timeout=10) == 0
        (args.output / 'android.json').write_text(json.dumps(data, indent=2) + '\n')
        apk = CHUNKS.adb(args.serial, 'shell', 'pm', 'path', PACKAGE).removeprefix('package:')
        metadata = dict(traced=args.trace, sources=CHUNKS.verify_source(),
                        apk_sha256=CHUNKS.adb(args.serial, 'shell', 'sha256sum', apk).split()[0])
        (args.output / 'metadata.json').write_text(json.dumps(metadata, indent=2) + '\n')
        print('completed 40 byte-exact replay trials', flush=True)
    finally:
        cleanup(args.serial, remote, local, process)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--serial', required=True)
    parser.add_argument('--fixture', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--trace', action='store_true')
    run(parser.parse_args())


if __name__ == '__main__':
    main()
