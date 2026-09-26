#!/usr/bin/env python3
"""T575: controlled EVDI/FIFO versus X11 GPU capture through physical USB ACKs.

Requires an explicitly selected UNUSED EVDI card and output. Creates one temporary
test display; always retires it and restores the Blent Android activity.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import selectors
import socket
import struct
import subprocess
import time
import resource
import profile_usb_pipeline as PIPELINE
from profile_usb_wire import Acknowledgements, receipt

PACKAGE = 'com.blent.decoderbench.candidate'
APP = 'io.github.geraldo_netto.blent/com.blent.MainActivity'
CHOICE = dict(name='c2.unisoc.avc.decoder', stream=dict(codec='h264', profile='baseline', level=50, depth=8),
              low_latency=False, operating_rate=60)


def adb(args, *command):
    return subprocess.check_output(['adb', '-s', args.serial, *command], text=True, timeout=15)


def stop(process):
    if process is None:
        return
    if process.poll() is None:
        process.terminate()
    try:
        process.wait(timeout=3)
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait(timeout=2)


def encoder_command(args, variant, policy):
    if variant.startswith('gpu'):
        node = args.same_gpu if variant == 'gpu-same' else args.cross_gpu
        return [str(args.helper), args.output_name, node, '1280', '800', '1', '30', '18', '60000', str(args.frames), 'desktop']
    command = PIPELINE.command(policy, dict(width=1280, height=800, fps=30), args.cross_gpu)
    command[0] = args.ffmpeg
    command[command.index('-i') + 1] = str(args.output / 'capture.fifo')
    position = command.index('-map')
    return command[:position] + ['-frames:v', str(args.frames)] + command[position:]


def pump(args, command, connection, acks, folder):
    before = resource.getrusage(resource.RUSAGE_CHILDREN)
    helper_before = helper_cpu(args.evdi_pid)
    started = time.monotonic_ns()
    with (folder / 'encoder.log').open('w') as log, (folder / 'encoded.h264').open('wb') as encoded:
        process = subprocess.Popen(command, stdout=subprocess.PIPE, stderr=log, stdin=subprocess.DEVNULL,
                                   env=dict(os.environ, BLENT_GPU_TRACE='1'))
        packets = PIPELINE.PacketDelivery(connection, 0, encoded)
        deadline = time.monotonic() + args.frames / 30 + 12
        try:
            with selectors.DefaultSelector() as selector:
                selector.register(process.stdout, selectors.EVENT_READ)
                while selector.get_map():
                    if time.monotonic() > deadline or acks.failure:
                        raise RuntimeError(acks.failure or 'bounded encoder deadline')
                    for _, _ in selector.select(.5):
                        if not packets.read(process.stdout):
                            selector.unregister(process.stdout)
            if process.wait(timeout=2) != 0 or len(packets.rows) != args.frames:
                raise RuntimeError('encoder failure or incomplete encoded frame count')
        finally:
            stop(process)
            process.stdout.close()
    after = resource.getrusage(resource.RUSAGE_CHILDREN)
    cpu = (after.ru_utime + after.ru_stime) - (before.ru_utime + before.ru_stime)
    return dict(packets=packets.rows, encoder_cpu_seconds=cpu,
                evdi_cpu_seconds=helper_cpu(args.evdi_pid) - helper_before,
                elapsed_seconds=(time.monotonic_ns() - started) / 1e9)


def helper_cpu(pid):
    fields = Path(f'/proc/{pid}/stat').read_text().rsplit(')', 1)[1].split()
    return (int(fields[11]) + int(fields[12])) / os.sysconf('SC_CLK_TCK')


def finish(args, connection, acks):
    deadline = time.monotonic() + 2
    while len(acks.rows) < args.frames and time.monotonic() < deadline:
        time.sleep(.01)
    if len({row['sequence'] for row in acks.rows}) != args.frames:
        raise RuntimeError('missing or duplicate physical render ACK')
    # Retain T571's completion-race workaround, plus complete identity coverage.
    time.sleep(.1)
    connection.sendall(struct.pack('!I', 2))
    deadline = time.monotonic() + 5
    while time.monotonic() < deadline:
        result = subprocess.run(['adb', '-s', args.serial, 'shell', 'run-as', PACKAGE,
                                 'cat', 'files/result.json'], capture_output=True, text=True, timeout=5)
        if result.returncode == 0:
            return json.loads(result.stdout)
        time.sleep(.1)
    raise RuntimeError('Android replay completion deadline')


def replay(args, command, folder, connection):
    data = json.dumps(dict(width=1280, height=800, fps=30, mime='video/avc',
                     selection=dict(decoder_protocol=2, decoder_selection=CHOICE))).encode('ascii')
    connection.sendall(struct.pack('!H', len(data)) + data)
    acks = Acknowledgements(connection, receipt(CHOICE))
    acks.thread.start()
    packets = []
    try:
        acks.wait_ready(1)
        observed = pump(args, command, connection, acks, folder)
        packets = observed['packets']
        android = finish(args, connection, acks)
        if not android.get('completed') or acks.failure:
            raise RuntimeError('Android replay did not complete')
        return dict(**observed, acknowledgements=acks.rows, setups=acks.ready, android=android, command=command)
    finally:
        acks.stopping = True
        connection.shutdown(socket.SHUT_RDWR)
        acks.thread.join(timeout=2)
        (folder / 'observation.json').write_text(json.dumps(dict(packets=packets, acks=acks.rows,
                                               error=acks.failure), indent=2) + '\n')


def trial(args, variant, number, policy):
    folder = args.output / f'{number}-{variant}'
    folder.mkdir()
    with socket.socket() as listener:
        listener.bind(('127.0.0.1', 0)); listener.listen(1); listener.settimeout(10)
        port = listener.getsockname()[1]; route = f'tcp:{port}'
        adb(args, 'reverse', '--no-rebind', route, route)
        try:
            adb(args, 'shell', 'run-as', PACKAGE, 'rm', '-f', 'files/result.json')
            launch = adb(args, 'shell', 'am', 'start', '-S', '-W', '-n', PACKAGE + '/com.blent.benchmark.MainActivity',
                         '--ez', 'run', 'true', '--ei', 'usb_port', str(port))
            (folder / 'launch.txt').write_text(launch)
            if 'Status: ok' not in launch:
                raise RuntimeError('replay activity launch failed')
            connection, _ = listener.accept()
            with connection:
                connection.settimeout(10); connection.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
                result = replay(args, encoder_command(args, variant, policy), folder, connection)
        finally:
            adb(args, 'reverse', '--remove', route)
    (folder / 'result.json').write_text(json.dumps(result, indent=2) + '\n')
    # A fresh inode is required after a reader closes partway through a write.
    fifo = args.output / 'capture.fifo'
    fifo.unlink(); os.mkfifo(fifo, 0o600)
    print(number, variant, len(result['acknowledgements']), 'ACKs', flush=True)


def bench_edid(args):
    edid = bytearray(args.edid.read_bytes())
    if len(edid) != 128:
        raise ValueError('test EDID must have one verified base block')
    edid[12:16] = b'T575'
    edid[127] = (-sum(edid[:127])) & 255
    target = args.output / 'benchmark.edid'
    target.write_bytes(edid)
    return target


def wait_output(args):
    deadline = time.monotonic() + 30
    while time.monotonic() < deadline:
        output = subprocess.check_output(['xrandr', '--query'], text=True, timeout=3)
        if any(line.startswith(args.output_name + ' connected') for line in output.splitlines()):
            return
        time.sleep(.2)
    raise RuntimeError('temporary EVDI output did not appear')


def display_run(args, policy):
    fifo = args.output / 'capture.fifo'; os.mkfifo(fifo, 0o600)
    edid = bench_edid(args)
    helper = scene = None
    with (args.output / 'evdi.log').open('w') as log, (args.output / 'scene.log').open('w') as scene_log:
        try:
            helper = subprocess.Popen([args.evdi_helper, '--card', str(args.card), '--edid', str(edid),
                                       '--fps', '30', '--capture-fifo', str(fifo)], stdout=log, stderr=log)
            args.evdi_pid = helper.pid
            wait_output(args)
            subprocess.run(['xrandr', '--output', args.output_name, '--mode', '1280x800', '--pos', f'{args.x}x0'], check=True, timeout=5)
            scene = subprocess.Popen([args.scene, str(args.x), '0', str(args.scene_fps)], stdout=scene_log)
            for number in range(args.trials):
                variants = args.variants
                if number % 2:
                    variants = list(reversed(variants))
                for variant in variants:
                    trial(args, variant, number, policy)
        finally:
            stop(scene)
            subprocess.run(['xrandr', '--output', args.output_name, '--off'], timeout=5, check=False)
            stop(helper)


def run(args):
    status = Path(f'/sys/class/drm/card{args.card}/device/driver').resolve().name
    connected = list(Path('/sys/class/drm').glob(f'card{args.card}-*/status'))
    if status != 'evdi' or any(p.read_text().strip() != 'disconnected' for p in connected):
        raise RuntimeError('selected card must be an unused EVDI device')
    foreground = adb(args, 'shell', 'dumpsys', 'activity', 'activities')
    if not any('topResumedActivity=' in line and APP in line for line in foreground.splitlines()):
        raise RuntimeError('Blent must already be visible')
    args.output.mkdir(mode=0o700)
    provenance(args)
    before = adb(args, 'reverse', '--list')
    policy = PIPELINE.policies(30, 60000, 18)['h264_vaapi_baseline']
    try:
        display_run(args, policy)
    finally:
        adb(args, 'shell', 'am', 'start', '-W', '-n', APP)
        if adb(args, 'reverse', '--list') != before:
            raise RuntimeError('ADB routes changed during isolated replay')


def provenance(args):
    root = Path(__file__).resolve().parents[2]
    sources = list((root / 'host/gpu').glob('*.[ch]'))
    sources += [Path(__file__).resolve(), root / 'scripts/benchmarks/gpu-scene.c']
    copies = args.output / 'sources'; copies.mkdir()
    hashes = {}
    for path in sources:
        relative = path.relative_to(root)
        target = copies / relative; target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(path.read_bytes())
        hashes[str(relative)] = hashlib.sha256(path.read_bytes()).hexdigest()
    binaries = {name: hashlib.sha256(Path(getattr(args, name)).read_bytes()).hexdigest()
                for name in ['helper', 'evdi_helper', 'scene']}
    arguments = {key: str(value) for key, value in vars(args).items()}
    data = dict(arguments=arguments, sources=hashes, binaries=binaries, kernel=os.uname().release,
                ffmpeg=subprocess.check_output([args.ffmpeg, '-version'], text=True).splitlines()[0])
    (args.output / 'metadata.json').write_text(json.dumps(data, indent=2) + '\n')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ['serial', 'helper', 'ffmpeg', 'evdi-helper', 'scene', 'output-name', 'same-gpu', 'cross-gpu']:
        parser.add_argument('--' + name, required=True)
    parser.add_argument('--edid', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--card', type=int, required=True)
    parser.add_argument('--x', type=int, required=True)
    parser.add_argument('--frames', type=int, choices=range(60, 601), default=240)
    parser.add_argument('--trials', type=int, choices=range(1, 10), default=5)
    parser.add_argument('--scene-fps', type=int, choices=range(1, 61), default=29,
                        help='29 decorrelates source updates from the 30 FPS capture cadence')
    parser.add_argument('--variants', nargs='+', choices=['evdi-fifo', 'gpu-same', 'gpu-cross'],
                        default=['evdi-fifo', 'gpu-same'])
    run(parser.parse_args())


if __name__ == '__main__':
    main()
