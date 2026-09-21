#!/usr/bin/env python3
"""T575: associate decoded scene identities with physical Android render ACKs."""
import argparse
import json
from pathlib import Path
import statistics
import subprocess


def barcode(frame):
    if len(frame) != 384 * 16:
        raise ValueError('truncated barcode image')
    pixels = [frame[8 * 384 + bit * 16 + 8] for bit in range(24)]
    if any(64 <= value <= 192 for value in pixels):
        raise ValueError('corrupt or miscropped barcode; reject timing sample')
    return sum((value > 128) << bit for bit, value in enumerate(pixels))


def decoded_ids(ffmpeg, path, expected):
    raw = subprocess.check_output([ffmpeg, '-v', 'error', '-i', str(path), '-vf',
        'crop=384:16:0:0,format=gray', '-fps_mode', 'passthrough', '-f', 'rawvideo', 'pipe:1'], timeout=30)
    size = 384 * 16
    if len(raw) != expected * size:
        raise ValueError('decoded frame count differs from encoded packet count')
    return [barcode(raw[index:index + size]) for index in range(0, len(raw), size)]


def distribution(values):
    values = sorted(values)
    if not values:
        raise ValueError('empty latency distribution')
    return dict(samples=len(values), p50_ms=statistics.median(values),
                p95_ms=values[int((len(values) - 1) * .95)], max_ms=values[-1])


def analyze(result, identities, scene, warmup):
    packets = result['packets']; acknowledgements = result['acknowledgements']
    acks = {row['sequence']: row['acknowledged_ns'] for row in acknowledgements}
    expected = set(range(1, len(packets) + 1))
    if len(acks) != len(acknowledgements) or set(acks) != expected:
        raise ValueError('missing, duplicate or unrelated ACK')
    first = {}; packet_age = []
    for index in range(warmup, len(packets)):
        code = identities[index]
        if code not in scene:
            raise ValueError('decoded source identity absent from scene trace')
        received = acks[index + 1]
        first.setdefault(code, (received - scene[code][0]) / 1e6)
        packet_age.append((received - packets[index]['ready_ns']) / 1e6)
    if any(value < 0 for value in first.values()):
        raise ValueError('ACK precedes source update')
    coverage = len(first) / (max(first) - min(first) + 1)
    if coverage < .95:
        raise ValueError('less than 95% of scene updates delivered')
    return dict(source_update_to_ack=distribution(first.values()), packet_to_ack=distribution(packet_age),
                unique_updates=len(first), scene_update_coverage=coverage, complete_acks=len(acks))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('directory', type=Path)
    parser.add_argument('--ffmpeg', required=True)
    parser.add_argument('--warmup', type=int, default=30)
    args = parser.parse_args()
    scene = {int(a): (int(b), int(c)) for a, b, c in
             (line.split() for line in (args.directory / 'scene.log').read_text().splitlines())}
    rows = []
    for path in sorted(args.directory.glob('*-*/result.json')):
        result = json.loads(path.read_text())
        identities = decoded_ids(args.ffmpeg, path.parent / 'encoded.h264', len(result['packets']))
        observed = analyze(result, identities, scene, args.warmup)
        observed['trial'] = path.parent.name
        observed['decoded_scene_ids'] = identities
        rows.append(observed)
        print(path.parent.name, json.dumps(observed['source_update_to_ack']), flush=True)
    (args.directory / 'summary.json').write_text(json.dumps(rows, indent=2) + '\n')


if __name__ == '__main__':
    main()
