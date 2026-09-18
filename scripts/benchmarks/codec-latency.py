#!/usr/bin/env python3
"""T400: paced raw-input admission to stock FFmpeg framecrc packet delivery."""
import argparse
import hashlib
import importlib.util
import json
import mmap
import os
import selectors
from pathlib import Path
import subprocess
import time

SPEC = importlib.util.spec_from_file_location('codec_host', Path(__file__).with_name('codec-host.py'))
HOST = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(HOST)


REPLAY_GRACE_SECONDS = 30


def remaining(deadline):
    seconds = deadline - time.monotonic()
    if seconds <= 0:
        raise TimeoutError('encoder replay deadline exceeded')
    return seconds


class PacedInput:
    def __init__(self, stream, raw, meta):
        self.stream, self.raw = stream, raw
        self.frame_bytes = meta['width'] * meta['height'] * 3 // 2
        self.count, self.fps = meta['frames'], meta['fps']
        if self.count <= 0 or self.fps <= 0 or len(raw) != self.frame_bytes * self.count:
            raise ValueError('raw input does not match complete frame metadata')
        self.frames = []
        self.start = time.monotonic_ns()
        self.position = 0
        self.admitted = None

    def delay(self):
        scheduled = self.start + len(self.frames) * 1_000_000_000 // self.fps
        return max(0, (scheduled - time.monotonic_ns()) / 1e9)

    def write(self):
        end = (len(self.frames) + 1) * self.frame_bytes
        count = os.write(self.stream.fileno(), self.raw[self.position:end])
        if count <= 0:
            raise BrokenPipeError('encoder input made no progress')
        self.position += count
        if self.position != end:
            return False
        scheduled = self.start + len(self.frames) * 1_000_000_000 // self.fps
        self.frames.append(dict(index=len(self.frames), scheduled_ns=scheduled,
                                admitted_ns=self.admitted, written_ns=time.monotonic_ns()))
        self.admitted = None
        return True

    def done(self):
        return len(self.frames) == self.count


class PacketOutput:
    def __init__(self):
        self.pending = bytearray()
        self.packets, self.headers = [], []

    def line(self, raw):
        observed = time.monotonic_ns()
        text = raw.decode('ascii').strip()
        if text.startswith('#'):
            self.headers.append(text)
        elif text:
            cells = [cell.strip() for cell in text.split(',')]
            if len(cells) < 6:
                raise ValueError('malformed encoder framecrc line')
            self.packets.append(dict(observed_ns=observed, pts=int(cells[2]), bytes=int(cells[4]), line=text))

    def read(self, stream):
        data = os.read(stream.fileno(), 65536)
        self.pending.extend(data)
        while b'\n' in self.pending:
            end = self.pending.index(b'\n')
            self.line(self.pending[:end])
            del self.pending[:end + 1]
        if len(self.pending) > 65536:
            raise ValueError('encoder framecrc line exceeds limit')
        if not data and self.pending:
            raise ValueError('truncated encoder framecrc line')
        return bool(data)


def pump_input(selector, writer):
    if writer.write():
        selector.unregister(writer.stream)
        if writer.done():
            writer.stream.close()


def pump_io(process, raw, meta, deadline):
    writer = PacedInput(process.stdin, raw, meta)
    output = PacketOutput()
    os.set_blocking(process.stdin.fileno(), False)
    os.set_blocking(process.stdout.fileno(), False)
    with selectors.DefaultSelector() as selector:
        selector.register(process.stdout, selectors.EVENT_READ)
        while not writer.done() or selector.get_map():
            wait = schedule_input(selector, writer, deadline)
            for key, _ in selector.select(wait):
                if key.fileobj is process.stdin:
                    pump_input(selector, writer)
                elif not output.read(process.stdout):
                    selector.unregister(process.stdout)
    return writer.frames, output


def schedule_input(selector, writer, deadline):
    wait = remaining(deadline)
    if not writer.done() and writer.stream not in selector.get_map():
        delay = writer.delay()
        if delay == 0:
            # Keep the original admission boundary before pipe backpressure.
            writer.admitted = time.monotonic_ns()
            selector.register(writer.stream, selectors.EVENT_WRITE)
        else:
            wait = min(wait, delay)
    return wait


def replay(command, source, meta, folder):
    deadline = time.monotonic() + meta['frames'] / meta['fps'] + REPLAY_GRACE_SECONDS
    with (folder / 'ffmpeg.log').open('wb') as log, source.open('rb') as pixels:
        with mmap.mmap(pixels.fileno(), 0, access=mmap.ACCESS_READ) as raw, memoryview(raw) as view:
            process = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=log, bufsize=0)
            try:
                frames, output = pump_io(process, view, meta, deadline)
                status = process.wait(timeout=remaining(deadline))
                if status != 0:
                    raise RuntimeError(f'encoder completion failed: {status}')
            except subprocess.TimeoutExpired as error:
                raise TimeoutError('encoder replay deadline exceeded') from error
            finally:
                if process.poll() is None:
                    process.kill()
                process.wait(timeout=1)
                process.stdin.close()
                process.stdout.close()
    return dict(frames=frames, packets=output.packets, headers=output.headers, returncode=status)


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
