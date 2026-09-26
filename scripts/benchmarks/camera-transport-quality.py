#!/usr/bin/env python3
"""T608: independently decode surviving reference chains and quantify frame loss."""
import argparse
import importlib.util
import json
import math
from pathlib import Path
import subprocess
import numpy as np

ROOT = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location('wire', ROOT / 'camera-socket.py')
WIRE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(WIRE)
PIXELS = 1280 * 720


def decode(encoded, output):
    command = ['ffmpeg', '-v', 'error', '-threads', '1', '-i', str(encoded), '-an',
               '-threads', '1', '-pix_fmt', 'gray', '-f', 'rawvideo', '-y', str(output)]
    result = subprocess.run(command, capture_output=True, timeout=30)
    assert result.returncode == 0, result.stderr.decode()
    assert not result.stderr, result.stderr.decode()
    assert output.stat().st_size % PIXELS == 0
    return np.memmap(output, mode='r', dtype=np.uint8).reshape(-1, PIXELS)


def psnr(mse):
    return None if mse == 0 else 10 * math.log10(255**2 / mse)


def loss_quality(reference, decoded, sequences):
    selected = dict(zip(sequences, decoded))
    previous = np.zeros(PIXELS, dtype=np.uint8)
    squared = 0.0
    for sequence, frame in enumerate(reference):
        previous = selected.get(sequence, previous)
        delta = frame.astype(np.float32) - previous
        squared += float(np.mean(delta * delta))
    return psnr(squared / len(reference))


def gaps(sequences, count):
    # Includes the initial and trailing unavailable interval, not just recovered gaps.
    return max(b-a-1 for a, b in zip([-1]+sequences, sequences+[count])) * 1000/30


def assess(row, packets, reference, work):
    sequences = [p['sequence'] for p in row['arrivals']]
    encoded, pixels = work / 'candidate.h264', work / 'candidate.gray'
    encoded.write_bytes(b''.join(packets[n] for n in sequences))
    decoded = decode(encoded, pixels) if sequences else np.empty((0, PIXELS), dtype=np.uint8)
    assert len(decoded) == len(sequences), 'lost/corrupted decoder output'
    assert all(np.array_equal(frame, reference[n]) for frame, n in zip(decoded, sequences))
    ages = [p['age_ms'] for p in row['arrivals']]
    quality = dict(decoded_frames=len(decoded), pixel_exact_frames=len(decoded),
                   hold_last_psnr_db=loss_quality(reference, decoded, sequences),
                   longest_missing_ms=gaps(sequences, len(packets)),
                   age_p95_ms=float(np.percentile(ages, 95)) if ages else None,
                   age_p99_ms=float(np.percentile(ages, 99)) if ages else None,
                   within_150ms=sum(p['age_ms'] <= 150 for p in row['arrivals']))
    if pixels.exists():
        pixels.unlink()
    return quality


def run(args):
    args.work.mkdir(parents=True)
    packets = WIRE.fixture(args.fixture)
    reference = decode(args.fixture.with_name('source.h264'), args.work / 'reference.gray')
    assert len(reference) == len(packets)
    data = json.loads(args.results.read_text())
    for row in data['rows']:
        row['quality'] = assess(row, packets, reference, args.work)
    args.output.write_text(json.dumps(data, indent=2) + '\n')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--fixture', type=Path, required=True)
    parser.add_argument('--results', type=Path, required=True)
    parser.add_argument('--work', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    run(parser.parse_args())


if __name__ == '__main__':
    main()
