"""Stock production encoder policy over a separately measured USB replay route."""
import importlib.util
import json
import mmap
import os
from pathlib import Path
import selectors
import struct
import subprocess
import time
from profile_usb_wire import TeePackets


def module(name):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(name + '.py'))
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


LATENCY = module('codec-latency')
ROOT = Path(__file__).resolve().parents[2]


def policies(fps, bitrate, quality):
    return json.loads(subprocess.check_output([
        'cargo', 'run', '--quiet', '-p', 'blent-config', '--example', 'encoder-options', '--',
        str(fps), str(bitrate), str(quality)], cwd=ROOT, text=True, timeout=60))


def command(profile, meta, render_node):
    encoder = profile['encoder']
    vaapi = encoder.endswith('_vaapi')
    args = ['ffmpeg', '-nostdin', '-hide_banner', '-loglevel', 'error']
    if vaapi:
        args += ['-vaapi_device', render_node]
    args += ['-probesize', '32', '-analyzeduration', '0', '-flags', 'low_delay',
             '-color_primaries', 'bt709', '-color_trc', 'bt709', '-colorspace', 'bt709', '-color_range', 'tv',
             '-f', 'rawvideo', '-pix_fmt', 'nv12', '-s', f'{meta["width"]}x{meta["height"]}',
             '-framerate', str(meta['fps']), '-use_wallclock_as_timestamps', '1', '-i', 'pipe:0']
    timing = "settb=1/1000000,setpts='if(isnan(PREV_OUTPTS),PTS,max(PTS,PREV_OUTPTS+1))'"
    args += ['-vf', timing + (',format=nv12,hwupload' if vaapi else ''), '-enc_time_base', '1:1000000',
             '-c:v', encoder, '-fps_mode', 'passthrough', '-force_key_frames',
             'expr:if(isnan(prev_forced_t),1,gte(t,prev_forced_t+1))']
    args += [value for pair in profile['options'] for value in pair]
    return args + ['-map', '0:v:0', '-f', 'tee',
                   '[f=framecrc:flush_packets=1]pipe:1|[f=data:flush_packets=1]pipe:1']


class PacketDelivery:
    def __init__(self, connection, offset, encoded):
        self.connection, self.offset, self.encoded = connection, offset, encoded
        self.parser, self.rows = TeePackets(), []

    def read(self, stream):
        data = os.read(stream.fileno(), 65536)
        for payload, pts in self.parser.feed(data):
            sequence = self.offset + len(self.rows) + 1
            at = time.monotonic_ns()
            self.connection.sendall(struct.pack('!III', 0, sequence, len(payload)) + payload)
            self.rows.append(dict(sequence=sequence, pts=pts, bytes=len(payload), ready_ns=at,
                                  sent_ns=time.monotonic_ns()))
            self.encoded.write(payload)
        if not data:
            self.parser.finish()
        return bool(data)


def pump_ready(selector, writer, packets, process, wait):
    for key, _ in selector.select(wait):
        if key.fileobj is process.stdin:
            LATENCY.pump_input(selector, writer)
        elif not packets.read(process.stdout):
            selector.unregister(process.stdout)


def pump(process, view, meta, connection, acks, offset, encoded):
    writer = LATENCY.PacedInput(process.stdin, view, meta)
    packets = PacketDelivery(connection, offset, encoded)
    deadline = time.monotonic() + meta['frames'] / meta['fps'] + 8
    os.set_blocking(process.stdin.fileno(), False)
    os.set_blocking(process.stdout.fileno(), False)
    with selectors.DefaultSelector() as selector:
        selector.register(process.stdout, selectors.EVENT_READ)
        while not writer.done() or selector.get_map():
            if acks.failure or acks.closed:
                raise RuntimeError(acks.failure or 'replay closed before encoder completed')
            wait = min(.5, LATENCY.schedule_input(selector, writer, deadline))
            pump_ready(selector, writer, packets, process, wait)
    if process.wait(timeout=LATENCY.remaining(deadline)) != 0:
        raise RuntimeError('stock encoder failed')
    if len(packets.rows) != meta['frames']:
        raise ValueError('encoded frame count differs from raw input')
    return dict(frames=writer.frames, packets=packets.rows, headers=packets.parser.headers)


def phase(args, row, meta, profile, connection, acks, offset, folder):
    paced = dict(meta, fps=row['rate'], frames=row['rate'] * row['seconds'])
    command_line = command(profile, meta, args.vaapi_device)
    source = args.corpus / (row['scene'] + '.nv12')
    size = paced['frames'] * meta['width'] * meta['height'] * 3 // 2
    with source.open('rb') as pixels, (folder / 'ffmpeg.log').open('wb') as log, \
            (folder / 'encoded.h264').open('wb') as encoded, mmap.mmap(pixels.fileno(), size, access=mmap.ACCESS_READ) as raw:
        process = subprocess.Popen(command_line, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=log, bufsize=0)
        try:
            with memoryview(raw) as view:
                result = pump(process, view, paced, connection, acks, offset, encoded)
        finally:
            if process.poll() is None:
                process.kill()
            process.wait(timeout=2)
            process.stdin.close()
            process.stdout.close()
    result.update(command=command_line, rate=row['rate'], frames_expected=paced['frames'])
    return result
