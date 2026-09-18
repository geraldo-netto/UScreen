#!/usr/bin/env python3
"""T400: stock encoder throughput/quality on pinned capture-format NV12 files."""
import argparse
import hashlib
import json
import math
import mmap
from pathlib import Path
import resource
import subprocess
import time

ENCODERS = ['h264_vaapi', 'hevc_vaapi', 'libx264', 'libx265', 'libvpx-vp9', 'libaom-av1']


def codec_options(encoder, quality):
    if encoder.endswith('_vaapi'):
        return ['-vf', 'format=nv12,hwupload', '-rc_mode', 'CQP', '-qp', str(quality), '-idr_interval', '0']
    if encoder == 'libx264':
        return ['-preset', 'ultrafast', '-tune', 'zerolatency', '-crf', str(quality),
                '-x264-params', 'scenecut=0:repeat-headers=1:aud=1']
    if encoder == 'libx265':
        return ['-preset', 'ultrafast', '-tune', 'zerolatency', '-crf', str(quality),
                '-x265-params', 'pools=8:frame-threads=1:scenecut=0:repeat-headers=1:log-level=error']
    if encoder == 'libvpx-vp9':
        return ['-deadline', 'realtime', '-cpu-used', '8', '-lag-in-frames', '0', '-row-mt', '1',
                '-auto-alt-ref', '0', '-tune-content', 'screen', '-b:v', '0', '-crf', str(quality)]
    if encoder == 'libaom-av1':
        return ['-usage', 'realtime', '-cpu-used', '8', '-lag-in-frames', '0', '-row-mt', '1',
                '-tiles', '2x1', '-b:v', '0', '-crf', str(quality)]
    raise ValueError(encoder)


def command_for(args, meta, scene, encoder, quality, output):
    command = ['ffmpeg', '-nostdin', '-hide_banner', '-loglevel', 'warning']
    if encoder.endswith('_vaapi'):
        command += ['-vaapi_device', args.vaapi_device]
    command += ['-f', 'rawvideo', '-pixel_format', meta['pixel_format'], '-video_size', f'{meta["width"]}x{meta["height"]}',
                '-framerate', str(meta['fps']), '-i', str(args.corpus / scene['path']), '-frames:v', str(meta['frames']),
                '-an', '-c:v', encoder, '-threads', '8', '-g', str(meta['fps']), '-bf', '0', '-flags', '+global_header',
                '-color_range', 'tv', '-colorspace', 'bt709', '-color_primaries', 'bt709', '-color_trc', 'bt709']
    return command + codec_options(encoder, quality) + ['-y', str(output)]


def warm(path):
    with path.open('rb') as source:
        with mmap.mmap(source.fileno(), 0, access=mmap.ACCESS_READ) as pixels:
            return sum(pixels[index] for index in range(0, len(pixels), 4096))


def encode(command, folder):
    usage = resource.getrusage(resource.RUSAGE_CHILDREN)
    started = time.monotonic()
    result = subprocess.run(command, capture_output=True, text=True, timeout=180)
    elapsed = time.monotonic() - started
    after = resource.getrusage(resource.RUSAGE_CHILDREN)
    (folder / 'encode.log').write_text(result.stderr)
    return dict(command=command, returncode=result.returncode, wall_seconds=elapsed,
                user_seconds=after.ru_utime - usage.ru_utime, system_seconds=after.ru_stime - usage.ru_stime)


def quality(reference, decoded, meta):
    import numpy as np
    width, height, frames = meta['width'], meta['height'], meta['frames']
    frame_bytes = width * height * 3 // 2
    expected = frame_bytes * frames
    if decoded.stat().st_size != expected:
        raise ValueError(f'decoded byte count {decoded.stat().st_size} != {expected}')
    source = np.memmap(reference, dtype=np.uint8, mode='r', shape=(frames, frame_bytes))
    target = np.memmap(decoded, dtype=np.uint8, mode='r', shape=(frames, frame_bytes))
    errors = dict(all=0, luma=0, text_crop=0)
    for number in range(frames):
        difference = np.subtract(source[number], target[number], dtype=np.int16).astype(np.int32)
        squared = difference * difference
        errors['all'] += int(squared.sum(dtype=np.int64))
        luma = squared[:width * height].reshape(height, width)
        errors['luma'] += int(luma.sum(dtype=np.int64))
        errors['text_crop'] += int(luma[58:HEIGHT_CROP, 20:630].sum(dtype=np.int64))
    samples = dict(all=expected, luma=frames * width * height, text_crop=frames * (HEIGHT_CROP - 58) * 610)
    mse = {key: value / samples[key] for key, value in errors.items()}
    return dict(mse=mse, psnr_db={key: None if value == 0 else 10 * math.log10(255 ** 2 / value) for key, value in mse.items()},
                decoded_frames=frames, zero_mse_means_lossless=True)


HEIGHT_CROP = 750


def inspect(output, folder, source, meta):
    decoded = folder / 'decoded.nv12'
    command = ['ffmpeg', '-nostdin', '-hide_banner', '-loglevel', 'error',
               '-xerror', '-err_detect', 'explode', '-i', str(output),
               '-pix_fmt', 'nv12', '-f', 'rawvideo', '-y', str(decoded)]
    try:
        result = subprocess.run(command, capture_output=True, text=True, timeout=180)
        (folder / 'decode.log').write_text(result.stderr)
        result.check_returncode()
        # T461: some decoder errors still return zero with concealed or truncated
        # output. Error-level diagnostics invalidate the quality measurement.
        if result.stderr.strip():
            raise ValueError(f'decoder errors; see {folder / "decode.log"}')
        measured = quality(source, decoded, meta)
        measured['decode_command'] = command
    finally:
        decoded.unlink(missing_ok=True)
    probe = subprocess.check_output(['ffprobe', '-v', 'error', '-select_streams', 'v:0', '-show_streams', '-of', 'json', str(output)], text=True)
    measured['stream'] = json.loads(probe)['streams'][0]
    return measured


def trial(args, meta, scene, encoder, quantizer):
    folder = args.output / f'{scene["scene"]}-{encoder}-q{quantizer}'
    folder.mkdir()
    output = folder / 'encoded.mkv'
    source = args.corpus / scene['path']
    warm(source)
    result = encode(command_for(args, meta, scene, encoder, quantizer, output), folder)
    result.update(scene=scene['scene'], encoder=encoder, quality=quantizer, reference_sha256=scene['sha256'])
    if result['returncode'] == 0:
        result.update(inspect(output, folder, source, meta))
        result.update(encoded_bytes=output.stat().st_size, sha256=hashlib.sha256(output.read_bytes()).hexdigest(),
                      mbps=output.stat().st_size * 8 * meta['fps'] / meta['frames'] / 1e6,
                      fps_including_startup=meta['frames'] / result['wall_seconds'])
    (folder / 'result.json').write_text(json.dumps(result, indent=2) + '\n')
    print(scene['scene'], encoder, quantizer, result['returncode'], round(result['wall_seconds'], 3), result.get('psnr_db'), flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--corpus', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--encoder', choices=ENCODERS, action='append')
    parser.add_argument('--quality', type=int, action='append')
    parser.add_argument('--vaapi-device', default='/dev/dri/renderD128', help='Explicit lab device; not a production default')
    args = parser.parse_args()
    meta = json.loads((args.corpus / 'metadata.json').read_text())
    if (meta['width'], meta['height'], meta['pixel_format']) != (1280, 800, 'nv12'):
        parser.error('quality crop is defined for the 1280x800 NV12 corpus')
    args.output.mkdir()
    (args.output / 'metadata.json').write_text(json.dumps(dict(corpus=meta, encoders=args.encoder or ENCODERS,
        quantizers=args.quality or [18, 26, 34], ffmpeg=subprocess.check_output(['ffmpeg', '-version'], text=True)), indent=2) + '\n')
    for scene in meta['scenes']:
        for encoder in args.encoder or ENCODERS:
            for quantizer in args.quality or [18, 26, 34]:
                trial(args, meta, scene, encoder, quantizer)


if __name__ == '__main__':
    main()
