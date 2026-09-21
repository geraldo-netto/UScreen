#!/usr/bin/env python3
"""T575 native correctness checks on an explicitly selected UNUSED EVDI output.

Never selects a device automatically. No Android, pointer, power or service
actions. The temporary display is retired even when a native assertion fails.
"""
import argparse
import fcntl
import importlib.util
import os
from pathlib import Path
import signal
import subprocess
import time
from profile_usb_wire import TeePackets


def module():
    spec = importlib.util.spec_from_file_location('gpu_bench', Path(__file__).with_name('gpu-capture.py'))
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


BENCH = module()


def command(args, connector=None, scale=1, frames=6, mode='desktop', device=None):
    return [args.helper, connector or str(args.output / 'benchmark.edid'), device or args.same_gpu,
            str(1280 // scale // 2 * 2), str(800 // scale // 2 * 2), str(scale), '30', '18',
            '60000', str(frames), mode]


def run(command_line):
    result = subprocess.run(command_line, capture_output=True, timeout=8)
    assert result.returncode == 0, result.stderr.decode()
    parser = TeePackets()
    packets = parser.feed(result.stdout)
    parser.finish()
    assert len(packets) == 6, len(packets)
    return result.stdout, b''.join(payload for payload, _ in packets)


def patterns(args):
    for scale in range(1, 5):
        tee, encoded = run(command(args, connector='synthetic', scale=scale, mode='pattern'))
        raw = subprocess.check_output([args.ffmpeg, '-v', 'error', '-f', 'h264', '-i', 'pipe:0',
            '-frames:v', '1', '-f', 'rawvideo', '-pix_fmt', 'rgb24', 'pipe:1'], input=encoded, timeout=5)
        width, height = 1280 // scale // 2 * 2, 800 // scale // 2 * 2
        assert len(raw) == width * height * 3
        for x, y, color in [(8, 8, (221, 51, 85)), (width - 8, height - 8, (85, 170, 51))]:
            actual = raw[(y * width + x) * 3:(y * width + x) * 3 + 3]
            assert all(abs(a - b) <= 3 for a, b in zip(actual, color)), (scale, list(actual), color)
        (args.output / f'pattern-{scale}.tee').write_bytes(tee)
    print('T575: patterned colors/scales 1..4 pass', flush=True)


def rejected(args):
    result = subprocess.run(command(args, connector='synthetic', mode='pattern', device=args.cross_gpu), capture_output=True, timeout=8)
    assert result.returncode == 2 and not result.stdout
    assert b'same render node' in result.stderr
    print('T575: cross-device rejected before stream header', flush=True)


def blocked_output(args):
    with subprocess.Popen(command(args, connector='synthetic', mode='pattern', frames=0), stdout=subprocess.PIPE,
                          stderr=subprocess.DEVNULL) as child:
        fcntl.fcntl(child.stdout, fcntl.F_SETPIPE_SZ, 4096)
        try:
            assert child.wait(timeout=8) == -signal.SIGALRM
        finally:
            BENCH.stop(child)
    print('T575: blocked output terminates within native deadline', flush=True)


def layout(args, *options):
    subprocess.run(['xrandr', '--output', args.output_name, *options], check=True, timeout=5)


def restore(args):
    layout(args, '--mode', '1280x800', '--rotate', 'normal', '--scale', '1x1', '--pos', f'{args.x}x0')


def transition(args, label, options, expected):
    with subprocess.Popen(command(args, frames=0), stdout=subprocess.PIPE,
                          stderr=subprocess.PIPE) as child:
        try:
            # Startup is bounded by the native alarm; the header follows admission.
            assert child.stdout.readline().startswith(b'#software:')
            layout(args, *options)
            _, error = child.communicate(timeout=4)
            assert child.returncode == 2 and expected in error, (label, child.returncode, error)
        finally:
            BENCH.stop(child)
            restore(args)
    run(command(args))  # Same output can start a fresh adapter after retirement.
    print('T575:', label, 'rejection and fresh capture pass', flush=True)


def desktop(args):
    fifo = args.output / 'capture.fifo'; os.mkfifo(fifo, 0o600)
    edid = BENCH.bench_edid(args)
    helper = scene = None
    with (args.output / 'evdi.log').open('w') as log:
        try:
            helper = subprocess.Popen([args.evdi_helper, '--card', str(args.card), '--edid', str(edid),
                                       '--fps', '30', '--capture-fifo', str(fifo)], stdout=log, stderr=log)
            BENCH.wait_output(args); restore(args)
            scene = subprocess.Popen([args.scene, str(args.x), '0', '29'], stdout=subprocess.DEVNULL)
            time.sleep(.1)
            run(command(args))
            transition(args, 'move', ['--pos', f'{args.x + 16}x0'], b'position')
            transition(args, 'rotation', ['--rotate', 'left'], b'unrotated')
            transition(args, 'transform', ['--scale', '1.25x1.25'], b'untransformed')
            transition(args, 'disconnect', ['--off'], b'active capture output')
        finally:
            BENCH.stop(scene)
            layout(args, '--off')
            BENCH.stop(helper)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ['helper', 'ffmpeg', 'evdi-helper', 'scene', 'output-name', 'same-gpu', 'cross-gpu']:
        parser.add_argument('--' + name, required=True)
    parser.add_argument('--edid', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--card', type=int, required=True)
    parser.add_argument('--x', type=int, required=True)
    args = parser.parse_args()
    driver = Path(f'/sys/class/drm/card{args.card}/device/driver').resolve().name
    connectors = list(Path('/sys/class/drm').glob(f'card{args.card}-*/status'))
    assert driver == 'evdi' and connectors and all(p.read_text().strip() == 'disconnected' for p in connectors)
    args.output.mkdir(mode=0o700)
    patterns(args); rejected(args); blocked_output(args); desktop(args)


if __name__ == '__main__':
    main()
