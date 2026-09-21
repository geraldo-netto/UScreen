#!/usr/bin/env python3
"""T580: compare stock-library EVDI helpers on an explicitly unused X11 output.

No Android, input, power or service actions. Uses the existing T575 scene/GPU
helper. Only the selected temporary EVDI output is enabled and retired.
"""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import resource
import select
import signal
import subprocess
import time


def load_benchmark():
    spec = importlib.util.spec_from_file_location('gpu_bench', Path(__file__).with_name('gpu-capture.py'))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


BENCH = load_benchmark()


def layout(args, *options):
    subprocess.run(['xrandr', '--output', args.output_name, *options], check=True, timeout=5)


def read_frame(fifo, size):
    started = time.monotonic()
    first_byte = None
    pixels = bytearray()
    descriptor = os.open(fifo, os.O_RDONLY | os.O_NONBLOCK)
    try:
        while len(pixels) < size:
            if time.monotonic() - started > 3:
                raise RuntimeError('FIFO resume exceeded three seconds')
            if select.select([descriptor], [], [], .1)[0]:
                chunk = os.read(descriptor, size - len(pixels))
                if chunk:
                    first_byte = first_byte or time.monotonic()
                    pixels.extend(chunk)
        return pixels, dict(first_byte_ms=(first_byte - started) * 1000,
                            full_frame_ms=(time.monotonic() - started) * 1000)
    finally:
        os.close(descriptor)


def check_fallback(args, folder, scene):
    scene.send_signal(signal.SIGSTOP)  # Freeze only our scene; no new damage needed.
    try:
        time.sleep(.4)
        pixels, result = read_frame(folder / 'capture.fifo', 1280 * 800 * 3 // 2)
        # Bottom-right patch is the scene's uniform green (RGB 85,170,51).
        offset = 790 * 1280 + 1270
        expected_y = ((47 * 85 + 157 * 170 + 16 * 51 + 128) >> 8) + 16
        assert pixels[offset] == expected_y, (pixels[offset], expected_y)
        result['sha256'] = hashlib.sha256(pixels).hexdigest()
        result['green_luma'] = pixels[offset]
        return result
    finally:
        scene.send_signal(signal.SIGCONT)


def measure(args, folder, helper):
    command = [args.gpu_helper, str(args.output / 'benchmark.edid'), args.render_node,
               '1280', '800', '1', '30', '18', '60000', str(args.frames), 'desktop']
    before = resource.getrusage(resource.RUSAGE_CHILDREN)
    helper_before = BENCH.helper_cpu(helper.pid)
    start = time.monotonic()
    with (folder / 'gpu.log').open('w') as log:
        subprocess.run(command, stdout=subprocess.DEVNULL, stderr=log,
                       timeout=args.frames / 30 + 10, check=True)
    elapsed = time.monotonic() - start
    after = resource.getrusage(resource.RUSAGE_CHILDREN)
    cpu = BENCH.helper_cpu(helper.pid) - helper_before
    return dict(evdi_cpu_seconds=cpu, elapsed_seconds=elapsed,
                evdi_one_core_percent=100 * cpu / elapsed, gpu_frames=args.frames,
                gpu_cpu_seconds=after.ru_utime + after.ru_stime - before.ru_utime - before.ru_stime)


def trial(args, number, variant):
    folder = args.output / f'{number}-{variant}'
    folder.mkdir()
    fifo = folder / 'capture.fifo'
    os.mkfifo(fifo, 0o600)
    helper = scene = None
    with (folder / 'evdi.log').open('w') as log, (folder / 'scene.log').open('w') as scene_log:
        try:
            helper = subprocess.Popen([getattr(args, variant), '--card', str(args.card), '--edid',
                str(args.output / 'benchmark.edid'), '--fps', '30', '--capture-fifo', str(fifo)],
                stdout=log, stderr=log)
            BENCH.wait_output(args)
            layout(args, '--mode', '1280x800', '--pos', f'{args.x}x0')
            scene = subprocess.Popen([args.scene, str(args.x), '0', '29'], stdout=scene_log)
            time.sleep(2)
            result = measure(args, folder, helper)
            if variant == 'after':
                result['fallback'] = check_fallback(args, folder, scene)
        finally:
            BENCH.stop(scene)
            layout(args, '--off')
            BENCH.stop(helper)
    result.update(pair=number, variant=variant)
    (folder / 'result.json').write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps(result), flush=True)
    return result


def prepare(args):
    driver = Path(f'/sys/class/drm/card{args.card}/device/driver').resolve().name
    connectors = list(Path('/sys/class/drm').glob(f'card{args.card}-*/status'))
    if driver != 'evdi' or not connectors or any(p.read_text().strip() != 'disconnected' for p in connectors):
        raise RuntimeError('selected card must be an unused EVDI device')
    outputs = subprocess.check_output(['xrandr', '--query'], text=True, timeout=5)
    if not any(line.startswith(args.output_name + ' disconnected') for line in outputs.splitlines()):
        raise RuntimeError('selected X11 output must already be disconnected')
    args.output.mkdir(mode=0o700)
    edid = bytearray(args.edid.read_bytes())
    assert len(edid) == 128
    edid[12:16] = b'T580'
    edid[127] = (-sum(edid[:127])) & 255
    (args.output / 'benchmark.edid').write_bytes(edid)
    binaries = {name: hashlib.sha256(Path(getattr(args, name)).read_bytes()).hexdigest()
                for name in ['before', 'after', 'gpu_helper', 'scene']}
    metadata = dict(arguments={k: str(v) for k, v in vars(args).items()}, binaries=binaries,
                    clock_ticks=os.sysconf('SC_CLK_TCK'), kernel=os.uname().release)
    (args.output / 'metadata.json').write_text(json.dumps(metadata, indent=2) + '\n')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ['before', 'after', 'gpu-helper', 'render-node', 'scene', 'output-name']:
        parser.add_argument('--' + name, required=True)
    parser.add_argument('--edid', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--card', type=int, required=True)
    parser.add_argument('--x', type=int, required=True)
    parser.add_argument('--frames', type=int, default=480)
    parser.add_argument('--pairs', type=int, default=3)
    args = parser.parse_args()
    assert 1 <= args.frames <= 1800 and 1 <= args.pairs <= 10
    prepare(args)
    results = []
    for pair in range(args.pairs):
        order = ['before', 'after'] if pair % 2 == 0 else ['after', 'before']
        for variant in order:
            results.append(trial(args, pair, variant))
    (args.output / 'results.json').write_text(json.dumps(results, indent=2) + '\n')


if __name__ == '__main__':
    main()
