#!/usr/bin/env python3
"""T419: RGB error of the matched H.264 controls, including 4:2:0 conversion."""
import argparse
import hashlib
import json
import math
from pathlib import Path
import subprocess
import numpy as np
from codec_artifacts import verify_file


def compare(reference, candidate, frames):
    count, squared, maximum = 0, 0., 0
    with reference.open('rb') as original, candidate.open('rb') as decoded:
        for _ in range(frames):
            left = original.read(1280 * 800 * 3)
            right = decoded.read(1280 * 800 * 3)
            if len(left) != 1280 * 800 * 3 or len(right) != len(left):
                raise ValueError('truncated RGB quality input')
            difference = np.frombuffer(left, np.uint8).astype(np.int16) - np.frombuffer(right, np.uint8)
            squared += np.square(difference, dtype=np.float64).sum()
            maximum = max(maximum, int(np.abs(difference).max()))
            count += difference.size
        if original.read(1) or decoded.read(1):
            raise ValueError('unexpected RGB frame count')
    mse = squared / count
    return dict(rgb_samples=count, rgb_mse=mse, rgb_psnr_db=10 * math.log10(255 * 255 / mse) if mse else None,
                maximum_channel_error=maximum)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('folder', type=Path)
    args = parser.parse_args()
    metadata = json.loads((args.folder / 'metadata.json').read_text())
    rows = []
    for scene in metadata['scenes']:
        stem = f'{scene["scene"]}-{scene["rate"]}'
        verify_file(args.folder / (stem + '.rgb'), scene['rgb_sha256'], scene['count'] * 1280 * 800 * 3)
        verify_file(args.folder / (stem + '.h264'), scene['h264_sha256'], scene['h264_bytes'])
        output = args.folder / (stem + '-decoded.rgb')
        command = ['ffmpeg', '-hide_banner', '-loglevel', 'error', '-i', str(args.folder / (stem + '.h264')),
                   '-vf', 'scale=in_color_matrix=bt709:in_range=limited:out_range=full,format=rgb24',
                   '-f', 'rawvideo', '-y', str(output)]
        subprocess.run(command, check=True, timeout=60)
        row = compare(args.folder / (stem + '.rgb'), output, scene['count'])
        row.update(scene=scene['scene'], command=command, h264_sha256=scene['h264_sha256'],
                   reference_sha256=scene['rgb_sha256'])
        rows.append(row)
        print(scene['scene'], row['rgb_psnr_db'], 'RGB PSNR dB (None means exact)', flush=True)
    result = dict(boundary='Original RGB versus decoded H.264 converted from limited BT.709 to full RGB; includes chroma subsampling',
                  ffmpeg=subprocess.check_output(['ffmpeg', '-version'], text=True),
                  source_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(), scenes=rows)
    (args.folder / 'quality.json').write_text(json.dumps(result, indent=2) + '\n')


if __name__ == '__main__':
    main()
