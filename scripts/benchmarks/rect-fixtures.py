#!/usr/bin/env python3
"""T419: matched RGB rectangle and VAAPI H.264 local replay fixtures."""
import argparse
import ctypes as C
import hashlib
import importlib.util
import json
from pathlib import Path
import re
import struct
import subprocess
import numpy as np
from PIL import Image, ImageFont
from rect_codecs import RectCodecs


def module(name):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(name + '.py'))
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


def hashing(folder):
    source = Path(__file__).with_name('android-rect') / 'native/rect_hash.c'
    target = folder / 'libfixturehash.so'
    subprocess.run(['cc', '-O3', '-shared', '-fPIC', str(source), '-o', str(target)], check=True)
    library = C.CDLL(str(target.resolve()))
    library.rect_rgb_hash.argtypes = [C.c_void_p, C.c_size_t, C.c_size_t]
    library.rect_rgb_hash.restype = C.c_uint64
    return library.rect_rgb_hash


def picture(corpus, base, photo, scene, number):
    if scene == 'scroll':
        return np.roll(np.asarray(base), -number * 5, axis=0).copy()
    if scene == 'photo':
        frame = base.copy()
        frame.paste(photo, (300 + number % 120, 0))
        return np.asarray(frame)
    return np.asarray(corpus.paint(scene, base, number))


def changed(frame, previous, refresh):
    if refresh:
        return (0, 0, 1280, 800), frame.tobytes()
    mask = frame.reshape(800, 3840) != previous.reshape(800, 3840)
    ys, xs = np.flatnonzero(mask.any(axis=1)), np.flatnonzero(mask.any(axis=0))
    if not len(xs):
        return (0, 0, 0, 0), b''
    x0, x1 = int(xs[0]) // 3, int(xs[-1]) // 3 + 1
    y0, y1 = int(ys[0]), int(ys[-1]) + 1
    return (x0, y0, x1 - x0, y1 - y0), np.ascontiguousarray(frame[y0:y1, x0:x1]).tobytes()


def records(args, scene, rate, corpus, base, photo, codecs, hash_rgb):
    count = 120 if rate == 60 else 20
    previous = np.zeros((800, 1280, 3), dtype=np.uint8)
    rows = []
    files = [(args.output / f'{scene}-{rate}-{codec}.rect').open('wb') for codec in (1, 2)]
    raw = args.output / f'{scene}-{rate}.rgb'
    try:
        for codec, output in enumerate(files, 1):
            output.write(struct.pack('>6I', 0x54523431, codec, 1280, 800, rate, count))
        with raw.open('wb') as rgb:
            for number in range(count):
                frame = picture(corpus, base, photo, scene, number)
                rgb.write(frame.tobytes())
                region, payload = changed(frame, previous, number % rate == 0)
                rows.append(write_update(files, codecs, hash_rgb, region, payload, frame, previous))
                previous = frame
    finally:
        for output in files:
            output.close()
    return dict(scene=scene, rate=rate, count=count, rows=rows, rgb_sha256=sha(raw))


def write_update(files, codecs, hash_rgb, region, payload, frame, previous):
    expected = hash_rgb(frame.tobytes(), 1280 * 800, 3)
    sizes = []
    for codec, output in enumerate(files, 1):
        packed = codecs.encode(codec, payload) if payload else b''
        rebuilt = previous.copy()
        x, y, width, height = region
        if payload:
            rebuilt[y:y+height, x:x+width] = np.frombuffer(codecs.decode(codec, packed, len(payload)), np.uint8).reshape(height, width, 3)
        assert np.array_equal(rebuilt, frame)
        output.write(struct.pack('>5IQ', *region, len(packed), expected) + packed)
        sizes.append(len(packed))
    return dict(region=region, raw=len(payload), packed=sizes, fnv64=str(expected))


def h264(args, row, policy):
    stem = f'{row["scene"]}-{row["rate"]}'
    raw, stream = args.output / (stem + '.rgb'), args.output / (stem + '.h264')
    command = ['ffmpeg', '-nostdin', '-hide_banner', '-loglevel', 'error', '-vaapi_device', '/dev/dri/renderD128',
               '-f', 'rawvideo', '-pix_fmt', 'rgb24', '-s', '1280x800', '-framerate', '60', '-i', str(raw),
               '-vf', 'scale=in_range=full:out_range=limited:out_color_matrix=bt709,format=nv12,hwupload',
               '-c:v', 'h264_vaapi', '-color_range', 'tv', '-colorspace', 'bt709', '-color_primaries', 'bt709',
               '-color_trc', 'bt709']
    command += [value for pair in policy['options'] for value in pair]
    # Local fixtures retain the nominal 60 Hz stream configuration while sparse
    # replay delivers five updates/s. IDR interval matches one replay second.
    command += ['-g', str(row['rate']), '-y', str(stream)]
    subprocess.run(command, check=True)
    packets = json.loads(subprocess.check_output(['ffprobe', '-v', 'error', '-show_packets', '-show_entries',
                        'packet=pos,size', '-of', 'json', str(stream)], text=True))['packets']
    assert len(packets) == row['count']
    data = stream.read_bytes()
    frames = [data[int(p['pos']):int(p['pos'])+int(p['size'])] for p in packets]
    config = module('decoder-fixture').configuration(frames[0])
    video_fixture(args.output / (stem + '.video'), config, frames)
    if row['scene'] == 'text' and row['rate'] == 5:
        idle_control(args.output, config, frames)
    subprocess.run(['ffmpeg', '-v', 'error', '-i', str(stream), '-f', 'null', '-'], check=True,
                   stderr=(args.output / (stem + '-decode.log')).open('w'))
    row['encoder_command'] = command
    row['h264_bytes'] = len(data)
    row['h264_sha256'] = sha(stream)


def video_fixture(path, config, frames):
    with path.open('wb') as output:
        output.write(b'USDB0001' + struct.pack('>4I', 1280, 800, 60, len(frames)))
        for packet in [config, *frames]:
            output.write(struct.pack('>I', len(packet)) + packet)


def idle_control(folder, config, frames):
    selected = frames[::5]
    for frame in selected:
        starts = re.finditer(b'\x00\x00(?:\x00)?\x01', frame)
        assert any(frame[start.end()] & 31 == 5 for start in starts)
    target = folder / 'text-1.video'
    video_fixture(target, config, selected)
    record = dict(source_sha256=sha(folder / 'text-5.video'), selected_picture_indices=list(range(0, len(frames), 5)),
                  all_selected_pictures_are_idr=True, sha256=sha(target), source_cadence=5, control_cadence=1, nominal_fps=60)
    (folder / 'idle-control.json').write_text(json.dumps(record, indent=2) + '\n')


def sha(path):
    with path.open('rb') as data:
        return hashlib.file_digest(data, 'sha256').hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', required=True, type=Path)
    parser.add_argument('--font', required=True, type=Path)
    parser.add_argument('--photo', required=True, type=Path)
    args = parser.parse_args()
    args.output.mkdir(exist_ok=True)
    corpus = module('codec-corpus')
    base = corpus.desktop(ImageFont.truetype(str(args.font), 18))
    photo = Image.open(args.photo).convert('RGB').resize((800, 800), Image.Resampling.LANCZOS)
    codecs, hash_rgb = RectCodecs(), hashing(args.output)
    policies = module('profile_usb_pipeline').policies(60, 20000, 18)
    rows = []
    for scene, rate in [('text', 5), ('pen', 60), ('motion', 60), ('scroll', 60), ('photo', 60)]:
        row = records(args, scene, rate, corpus, base, photo, codecs, hash_rgb)
        h264(args, row, policies['h264_vaapi_baseline'])
        rows.append(row)
        print(scene, 'complete', flush=True)
    meta = dict(scenes=rows, libraries=codecs.versions(), font_sha256=sha(args.font), photo_sha256=sha(args.photo),
                photo_credit="NASA's Earth Observatory, Blue Marble 2002; moving photographic composite, not video footage",
                nominal_stream_fps=60,
                files={p.name: dict(bytes=p.stat().st_size, sha256=sha(p)) for p in args.output.iterdir()
                       if p.suffix in {'.rect', '.video'}})
    (args.output / 'metadata.json').write_text(json.dumps(meta, indent=2) + '\n')


if __name__ == '__main__':
    main()
