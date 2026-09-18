#!/usr/bin/env python3
"""T400: preserve selected encoded access units in versioned decoder fixtures."""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import re
import struct
import subprocess

MIMES = dict(h264='video/avc', hevc='video/hevc', vp9='video/x-vnd.on2.vp9', av1='video/av01')

SPEC = importlib.util.spec_from_file_location('codec_artifacts', Path(__file__).with_name('codec_artifacts.py'))
ARTIFACTS = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(ARTIFACTS)


def elementary(source, codec, target):
    command = ['ffmpeg', '-nostdin', '-hide_banner', '-loglevel', 'warning', '-i', str(source), '-map', '0:v:0', '-c:v', 'copy']
    if codec in ['h264', 'hevc']:
        command += ['-bsf:v', f'{codec}_mp4toannexb,{codec}_metadata=aud=insert', '-f', codec]
    else:
        command += ['-f', 'ivf']
    subprocess.run(command + [str(target)], check=True, capture_output=True)
    return command + [str(target)]


def annex_frames(path):
    probe = subprocess.check_output(['ffprobe', '-v', 'error', '-show_packets', '-show_entries',
                                     'packet=pos,size,flags', '-of', 'json', str(path)], text=True)
    packets = json.loads(probe)['packets']
    data = path.read_bytes()
    frames = [data[int(p['pos']):int(p['pos']) + int(p['size'])] for p in packets]
    if not packets[0]['flags'].startswith('K'):
        raise ValueError('fixture must begin at a random-access frame')
    return frames


def ivf_frames(path):
    data = path.read_bytes()
    if data[:4] != b'DKIF' or struct.unpack_from('<H', data, 6)[0] != 32:
        raise ValueError('invalid IVF header')
    offset, frames = 32, []
    while offset < len(data):
        size, _ = struct.unpack_from('<IQ', data, offset)
        offset += 12
        end = offset + size
        if size == 0 or end > len(data):
            raise ValueError('truncated IVF frame')
        frames.append(data[offset:end])
        offset = end
    return frames


def config_for(frame, codec):
    if codec not in ['h264', 'hevc']:
        return b''  # VP9/AV1 initialization is present in the first keyframe.
    starts = list(re.finditer(b'\x00\x00(?:\x00)?\x01', frame))
    required = [7, 8] if codec == 'h264' else [32, 33, 34]
    chunks = {}
    for index, start in enumerate(starts):
        nal = frame[start.end()]
        kind = nal & 31 if codec == 'h264' else (nal >> 1) & 63
        end = starts[index + 1].start() if index + 1 < len(starts) else len(frame)
        if kind in required:
            chunks[kind] = frame[start.start():end]
    if set(chunks) != set(required):
        raise ValueError(f'missing parameter sets: {set(required) - set(chunks)}')
    return b''.join(chunks[kind] for kind in required)


def write_fixture(path, mime, meta, config, frames):
    if len(frames) != meta['frames']:
        raise ValueError('encoded frame count differs from quality corpus')
    name = mime.encode('ascii')
    with path.open('xb') as output:
        output.write(b'USDB0002' + struct.pack('>4I', meta['width'], meta['height'], meta['fps'], len(frames)))
        output.write(struct.pack('>H', len(name)) + name)
        for packet in [config, *frames]:
            if len(packet) > 8 * 1024 * 1024:
                raise ValueError('codec sample exceeds fixture bound')
            output.write(struct.pack('>I', len(packet)) + packet)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--selection', type=Path, required=True)
    parser.add_argument('--corpus', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    args.output.mkdir()
    meta = json.loads((args.corpus / 'metadata.json').read_text())
    results = []
    for selected in json.loads(args.selection.read_text())['selections']:
        row, source = ARTIFACTS.selection(selected, args.corpus, meta)
        codec = row['stream']['codec_name']
        base = args.output / f'{row["scene"]}-{row["encoder"]}'
        raw = base.with_suffix('.' + (codec if codec in ['h264', 'hevc'] else 'ivf'))
        command = elementary(source, codec, raw)
        ARTIFACTS.verify_file(source, row['sha256'], row['encoded_bytes'])
        frames = annex_frames(raw) if codec in ['h264', 'hevc'] else ivf_frames(raw)
        fixture = base.with_suffix('.bin')
        config = config_for(frames[0], codec)
        write_fixture(fixture, MIMES[codec], meta, config, frames)
        results.append(dict(scene=row['scene'], encoder=row['encoder'], codec=codec, mime=MIMES[codec],
            fixture=fixture.name, fixture_sha256=hashlib.sha256(fixture.read_bytes()).hexdigest(),
            source_sha256=hashlib.sha256(source.read_bytes()).hexdigest(), command=command,
            quality=row['quality'], config_bytes=len(config), frames=len(frames), wire_payload_bytes=sum(map(len, frames))))
    (args.output / 'metadata.json').write_text(json.dumps(dict(corpus=meta, fixtures=results), indent=2) + '\n')


if __name__ == '__main__':
    main()
