#!/usr/bin/env python3
"""T399: skip whole raw pictures before encoding; quantify held-frame quality."""
import argparse
import hashlib
import json
import math
import mmap
from pathlib import Path
import subprocess
import time


def select_raw(source, output, frame_bytes, frames, step):
    with source.open('rb') as file, mmap.mmap(file.fileno(), 0, access=mmap.ACCESS_READ) as raw, output.open('xb') as target:
        for index in range(0, frames, step):
            target.write(raw[index * frame_bytes:(index + 1) * frame_bytes])


def held_quality(source, decoded, frame_bytes, frames, step):
    import numpy as np
    reference = np.memmap(source, mode='r', dtype=np.uint8, shape=(frames, frame_bytes))
    received = np.memmap(decoded, mode='r', dtype=np.uint8, shape=(frames // step, frame_bytes))
    squared_sum = 0
    for index in range(frames):
        difference = reference[index].astype(np.int32) - received[index // step].astype(np.int32)
        squared_sum += int((difference * difference).sum(dtype=np.int64))
    mse = squared_sum / (frames * frame_bytes)
    return dict(mse=mse, psnr_db=None if mse == 0 else 10 * math.log10(255 ** 2 / mse))


def trial(args, meta, scene, rate):
    folder = args.output / f'{scene["scene"]}-{rate}'
    folder.mkdir()
    frame_bytes = meta['width'] * meta['height'] * 3 // 2
    step = meta['fps'] // rate
    source = args.corpus / scene['path']
    raw, encoded, decoded = folder / 'selected.nv12', folder / 'encoded.mkv', folder / 'decoded.nv12'
    select_raw(source, raw, frame_bytes, meta['frames'], step)
    command = ['ffmpeg', '-nostdin', '-hide_banner', '-loglevel', 'warning', '-f', 'rawvideo', '-pixel_format', 'nv12',
        '-video_size', f'{meta["width"]}x{meta["height"]}', '-framerate', str(rate), '-i', str(raw),
        '-c:v', 'libx264', '-preset', 'ultrafast', '-tune', 'zerolatency', '-crf', '18', '-threads', '8',
        '-g', '60', '-bf', '0', '-x264-params', 'scenecut=0',
        '-force_key_frames', 'expr:if(isnan(prev_forced_t),1,gte(t,prev_forced_t+1))', str(encoded)]
    start = time.monotonic()
    result = subprocess.run(command, check=True, capture_output=True, timeout=120)
    elapsed = time.monotonic() - start
    (folder / 'encode.log').write_bytes(result.stderr)
    subprocess.run(['ffmpeg', '-nostdin', '-v', 'error', '-i', str(encoded), '-pix_fmt', 'nv12', '-f', 'rawvideo', str(decoded)],
                   check=True, capture_output=True, timeout=120)
    expected = meta['frames'] // step
    if decoded.stat().st_size != expected * frame_bytes:
        raise ValueError('decoded frame count does not match admitted whole pictures')
    probe = json.loads(subprocess.check_output(['ffprobe', '-v', 'error', '-show_packets', '-show_entries',
                       'packet=pts_time,flags', '-of', 'json', str(encoded)], text=True))['packets']
    row = dict(scene=scene['scene'], fps=rate, source_sha256=scene['sha256'], command=command,
               admitted_frames=expected, omitted_raw_frames=meta['frames'] - expected, encoded_bytes=encoded.stat().st_size,
               encoded_sha256=hashlib.sha256(encoded.read_bytes()).hexdigest(), encode_wall_seconds=elapsed,
               keyframe_times=[p['pts_time'] for p in probe if p['flags'].startswith('K')],
               held_picture_quality=held_quality(source, decoded, frame_bytes, meta['frames'], step))
    (folder / 'result.json').write_text(json.dumps(row, indent=2) + '\n')
    raw.unlink(); decoded.unlink()
    print(scene['scene'], rate, row['encoded_bytes'], row['held_picture_quality'], flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--corpus', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    meta = json.loads((args.corpus / 'metadata.json').read_text())
    if meta['fps'] != 60 or meta['frames'] % 60:
        parser.error('requires a complete 60-FPS corpus')
    args.output.mkdir()
    (args.output / 'metadata.json').write_text(json.dumps(dict(corpus=meta,
        ffmpeg=subprocess.check_output(['ffmpeg', '-version'], text=True),
        boundary='offline whole-NV12-picture admission; decoded picture held until next admitted source frame'), indent=2) + '\n')
    for scene in meta['scenes']:
        for rate in [60, 30, 15, 5]:
            trial(args, meta, scene, rate)


if __name__ == '__main__':
    main()
