#!/usr/bin/env python3
"""T400: paced raw-input admission to stock FFmpeg framecrc packet delivery."""
import argparse
import hashlib
import importlib.util
import json
import mmap
from pathlib import Path
import subprocess
import threading
import time

SPEC = importlib.util.spec_from_file_location('codec_host', Path(__file__).with_name('codec-host.py'))
HOST = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(HOST)


def collect_packets(stream, packets, headers):
    for line in stream:
        observed = time.monotonic_ns()
        text = line.decode('ascii').strip()
        if text.startswith('#'):
            headers.append(text)
        elif text:
            cells = [cell.strip() for cell in text.split(',')]
            packets.append(dict(observed_ns=observed, pts=int(cells[2]), bytes=int(cells[4]), line=text))


def write_frames(stream, raw, meta):
    frame_bytes = meta['width'] * meta['height'] * 3 // 2
    frames = []
    start = time.monotonic_ns()
    with memoryview(raw) as view:
        for index in range(meta['frames']):
            deadline = start + index * 1_000_000_000 // meta['fps']
            delay = deadline - time.monotonic_ns()
            if delay > 0:
                time.sleep(delay / 1e9)
            admitted = time.monotonic_ns()
            position, end = index * frame_bytes, (index + 1) * frame_bytes
            while position < end:
                position += stream.write(view[position:end])
            frames.append(dict(index=index, scheduled_ns=deadline, admitted_ns=admitted, written_ns=time.monotonic_ns()))
    return frames


def replay(command, source, meta, folder):
    packets, headers = [], []
    with (folder / 'ffmpeg.log').open('wb') as log, source.open('rb') as pixels:
        with mmap.mmap(pixels.fileno(), 0, access=mmap.ACCESS_READ) as raw:
            process = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=log, bufsize=0)
            reader = threading.Thread(target=collect_packets, args=(process.stdout, packets, headers))
            reader.start()
            try:
                frames = write_frames(process.stdin, raw, meta)
                process.stdin.close()
                status = process.wait(timeout=30)
                reader.join(timeout=5)
                if reader.is_alive() or status != 0:
                    raise RuntimeError(f'encoder completion failed: {status}')
            finally:
                if process.poll() is None:
                    process.kill()
                    process.wait()
                reader.join(timeout=5)
                process.stdout.close()
    return dict(frames=frames, packets=packets, headers=headers, returncode=status)


def trial(args, meta, scene, selected, number):
    encoder = selected['encoder']
    quality = selected['selected_quantizer']
    folder = args.output / f'{scene["scene"]}-{encoder}-{number}'
    folder.mkdir()
    command = HOST.command_for(args, meta, scene, encoder, quality, Path('unused'))
    command[command.index('-i') + 1] = 'pipe:0'
    command = command[:-2] + ['-flush_packets', '1', '-f', 'framecrc', 'pipe:1']
    if args.async_depth is not None and encoder.endswith('_vaapi'):
        command[-1:-1] = ['-async_depth', str(args.async_depth)]
    source = args.corpus / scene['path']
    HOST.warm(source)
    result = replay(command, source, meta, folder)
    result.update(command=command, scene=scene['scene'], encoder=encoder, quality=quality, trial=number,
                  reference_sha256=scene['sha256'], warmup_frames=meta['fps'])
    expected = list(range(meta['frames']))
    if [packet['pts'] for packet in result['packets']] != expected:
        raise ValueError('framecrc timestamps/count/order do not match input frame indices')
    result['latency_ms'] = [(packet['observed_ns'] - frame['admitted_ns']) / 1e6
                           for frame, packet in zip(result['frames'], result['packets'])]
    (folder / 'result.json').write_text(json.dumps(result, indent=2) + '\n')
    print(scene['scene'], encoder, number, 'complete', flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--corpus', type=Path, required=True)
    parser.add_argument('--selection', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--trials', type=int, default=3)
    parser.add_argument('--encoder', action='append', choices=HOST.ENCODERS)
    parser.add_argument('--async-depth', type=int, choices=[1, 2, 4])
    parser.add_argument('--vaapi-device', default='/dev/dri/renderD128')
    args = parser.parse_args()
    meta = json.loads((args.corpus / 'metadata.json').read_text())
    selected = json.loads(args.selection.read_text())['selections']
    if args.encoder:
        selected = [item for item in selected if item['encoder'] in args.encoder]
    args.output.mkdir()
    (args.output / 'metadata.json').write_text(json.dumps(dict(corpus=meta, selection_sha256=
        hashlib.sha256(args.selection.read_bytes()).hexdigest(), ffmpeg=subprocess.check_output(['ffmpeg', '-version'], text=True),
        boundary='scheduled in-memory NV12 write start to stdout framecrc packet line; includes pipe/muxer/checksum costs',
        trials=args.trials, encoders=args.encoder, async_depth=args.async_depth), indent=2) + '\n')
    scenes = {scene['scene']: scene for scene in meta['scenes']}
    for number in range(args.trials):
        order = selected if number % 2 == 0 else list(reversed(selected))
        for item in order:
            if item['selected_quantizer'] is None:
                raise ValueError(f'No quality match for {item["encoder"]}')
            trial(args, meta, scenes[item['scene']], item, number)


if __name__ == '__main__':
    main()
