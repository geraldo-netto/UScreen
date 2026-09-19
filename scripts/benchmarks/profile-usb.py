#!/usr/bin/env python3
"""T479: raw NV12 submission → encoder → USB → decoder callback → host ACK.

Separate replay APK/route; does not attach a capture display or alter preferences.
"""
import argparse
import hashlib
import json
from pathlib import Path
import socket
import struct
import subprocess
import time
from types import SimpleNamespace
import profile_usb_pipeline as PIPELINE
from profile_usb_wire import Acknowledgements, receipt

PLAN = PIPELINE.module('decoder-plan')
DEVICE = PLAN.DEVICE
PACKAGE = 'com.uscreen.decoderbench.candidate'


def foreground(serial):
    state = DEVICE.capture(serial, 'shell', 'dumpsys', 'activity', 'activities')
    top = [line for line in state.splitlines() if 'topResumedActivity=' in line]
    if len(top) != 1 or 'com.uscreen/.MainActivity' not in top[0]:
        raise RuntimeError('tablet is not available in the authorized UScreen foreground')


def validate(rows, meta):
    scenes = {scene['scene'] for scene in meta['scenes']}
    for row in rows:
        if row['scene'] not in scenes or row['encoder'] not in ['libx264', 'h264_vaapi', 'h264_vaapi_baseline']:
            raise ValueError('unsupported isolated USB trial')
        if row['rate'] not in [1, 5, 60] or not 2 <= row['seconds'] <= 12:
            raise ValueError('unbounded replay workload')
        if row['rate'] * row['seconds'] > meta['frames']:
            raise ValueError('replay exceeds verified corpus')


def connect(args, listener, port, metadata, folder):
    DEVICE.adb(args.serial, 'shell', 'run-as', PACKAGE, 'rm', '-f', 'files/result.json', capture_output=True)
    output = DEVICE.capture(args.serial, 'shell', 'am', 'start', '-S', '-W', '-n',
                            f'{PACKAGE}/com.uscreen.benchmark.MainActivity', '--ez', 'run', 'true',
                            '--ei', 'usb_port', str(port))
    (folder / 'launch.txt').write_text(output)
    if 'Status: ok' not in output:
        raise RuntimeError('replay Activity did not launch')
    connection, _ = listener.accept()
    connection.settimeout(4)
    connection.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
    data = json.dumps(metadata, separators=(',', ':')).encode('ascii')
    connection.sendall(struct.pack('!H', len(data)) + data)
    return connection


def stream(args, row, meta, policy, connection, folder):
    acks = Acknowledgements(connection, receipt(row['selection']['decoder_selection']))
    acks.thread.start()
    phases = []
    try:
        for number in range(2):
            acks.wait_ready(number + 1)
            phase_folder = folder / f'phase-{number}'
            phase_folder.mkdir()
            observed = PIPELINE.phase(args, row, meta, policy, connection, acks,
                                      number * row['rate'] * row['seconds'], phase_folder)
            phases.append(observed)
            # Allow outstanding callbacks before a controlled decoder restart.
            time.sleep(.3)
            connection.sendall(struct.pack('!I', 1 if number == 0 else 2))
        replay = DEVICE.wait_result(SimpleNamespace(serial=args.serial, seconds=5, warmup=0), PACKAGE)
        if not replay.get('completed') or acks.failure:
            raise RuntimeError(f'incomplete replay: {replay.get("error", acks.failure)}')
        return dict(phases=phases, acknowledgements=acks.rows, setups=acks.ready, android=replay)
    finally:
        acks.stopping = True
        connection.close()
        acks.thread.join(timeout=5)
        evidence = dict(phases=phases, acknowledgements=acks.rows, setups=acks.ready, error=acks.failure)
        (folder / 'session-observation.json').write_text(json.dumps(evidence, indent=2) + '\n')


def trial(args, row, meta, policy, number):
    foreground(args.serial)
    folder = args.output / f'trial-{number:03d}'
    folder.mkdir()
    (folder / 'request.json').write_text(json.dumps(row, indent=2) + '\n')
    before = DEVICE.capture(args.serial, 'shell', 'dumpsys', 'battery')
    (folder / 'battery-before.txt').write_text(before)
    metadata = dict(width=meta['width'], height=meta['height'], fps=meta['fps'], mime='video/avc', selection=row['selection'])
    with socket.socket() as listener:
        listener.bind(('127.0.0.1', 0))
        listener.listen(1)
        listener.settimeout(5)
        port = listener.getsockname()[1]
        route = f'tcp:{port}'
        if route in DEVICE.capture(args.serial, 'reverse', '--list').split():
            raise RuntimeError('benchmark route already exists')
        DEVICE.adb(args.serial, 'reverse', '--no-rebind', route, route, capture_output=True)
        try:
            connection = connect(args, listener, port, metadata, folder)
            result = stream(args, row, meta, policy, connection, folder)
        finally:
            DEVICE.adb(args.serial, 'reverse', '--remove', route, capture_output=True)
    (folder / 'result.json').write_text(json.dumps(result, indent=2) + '\n')
    (folder / 'battery-after.txt').write_text(DEVICE.capture(args.serial, 'shell', 'dumpsys', 'battery'))
    (folder / 'thermal-after.txt').write_text(DEVICE.capture(args.serial, 'shell', 'dumpsys', 'thermalservice'))
    DEVICE.logs(args, PACKAGE, folder)
    print(number, row['scene'], row['rate'], row['encoder'], len(acks_rows(result)), 'ACKs', flush=True)


def acks_rows(result):
    rows = result['acknowledgements']
    expected = sum(len(phase['frames']) for phase in result['phases'])
    sequences = [row['sequence'] for row in rows]
    if len(set(sequences)) != len(rows) or any(seq < 1 or seq > expected for seq in sequences):
        raise ValueError('duplicate or unrelated ACK')
    if len(rows) < expected * .9:
        raise ValueError('insufficient rendered frames')
    return rows


def run(args):
    meta = json.loads((args.corpus / 'metadata.json').read_text())
    rows = json.loads(args.plan.read_text())
    validate(rows, meta)
    args.output.mkdir()
    provenance = json.loads(args.provenance.read_text())
    PLAN.verify_apk(args.serial, provenance)
    policies = PIPELINE.policies(meta['fps'], 20000, 18)
    for scene in meta['scenes']:
        PIPELINE.LATENCY.HOST.ARTIFACTS.raw(args.corpus, meta, scene)
    sources = [Path(__file__), Path(PIPELINE.__file__), Path(__file__).with_name('profile_usb_wire.py')]
    source_hashes = {path.name: hashlib.sha256(path.read_bytes()).hexdigest() for path in sources}
    archive_sources(args.output, sources)
    metadata = dict(corpus=meta, plan=rows, provenance=provenance, policies=policies, scripts=source_hashes,
                    ffmpeg=subprocess.check_output(['ffmpeg', '-version'], text=True),
                    boundary='host raw-write admission to Android callback ACK received on host; no capture/compositor/optical timing')
    (args.output / 'metadata.json').write_text(json.dumps(metadata, indent=2) + '\n')
    for number, row in enumerate(rows):
        trial(args, row, meta, policies[row['encoder']], number)


def archive_sources(output, sources):
    folder = output / 'sources'
    folder.mkdir()
    for source in sources:
        (folder / source.name).write_bytes(source.read_bytes())


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--serial', required=True)
    parser.add_argument('--corpus', type=Path, required=True)
    parser.add_argument('--plan', type=Path, required=True)
    parser.add_argument('--provenance', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--vaapi-device', default='/dev/dri/renderD128')
    run(parser.parse_args())


if __name__ == '__main__':
    main()
