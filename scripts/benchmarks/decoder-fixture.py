#!/usr/bin/env python3
"""Generate a stock-FFmpeg Annex-B fixture for the separate Android decoder replay."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import struct
import subprocess


def configuration(packet):
    starts = list(re.finditer(b'\x00\x00(?:\x00)?\x01', packet))
    chunks = []
    for index, start in enumerate(starts):
        end = starts[index + 1].start() if index + 1 < len(starts) else len(packet)
        if packet[start.end()] & 31 in [7, 8]:
            chunks.append(packet[start.start():end])
    assert len(chunks) == 2, 'expected one SPS and PPS'
    return b''.join(chunks)


def generate(args):
    stream = args.output.with_suffix('.h264')
    video = ('testsrc2=size=1280x800:rate=60' if args.scene == 'motion'
             else 'color=c=0x203040:size=1280x800:rate=60,drawgrid=width=80:height=50:thickness=1:color=white@0.5')
    command = ['ffmpeg', '-hide_banner', '-loglevel', 'warning', '-f', 'lavfi', '-i', video, '-frames:v', '360',
               '-c:v', 'libx264', '-preset', 'veryfast', '-tune', 'zerolatency', '-crf', '18', '-g', '60', '-bf', '0',
               '-x264-params', 'aud=1:repeat-headers=1', '-pix_fmt', 'yuv420p', '-f', 'h264', str(stream)]
    subprocess.run(command, check=True)
    packets = json.loads(subprocess.check_output(['ffprobe', '-v', 'error', '-show_packets', '-show_entries',
                                                  'packet=pos,size', '-of', 'json', str(stream)], text=True))['packets']
    data = stream.read_bytes()
    frames = [data[int(row['pos']):int(row['pos']) + int(row['size'])] for row in packets]
    config = configuration(frames[0])
    with args.output.open('xb') as output:
        output.write(b'USDB0001' + struct.pack('>4I', 1280, 800, 60, len(frames)))
        for packet in [config, *frames]:
            assert 0 < len(packet) <= 8 * 1024 * 1024
            output.write(struct.pack('>I', len(packet)) + packet)
    metadata = dict(command=command, ffmpeg=subprocess.check_output(['ffmpeg', '-version'], text=True), scene=args.scene,
                    fixture_sha256=hashlib.sha256(args.output.read_bytes()).hexdigest(), stream_sha256=hashlib.sha256(data).hexdigest(), frames=len(frames))
    args.output.with_suffix('.json').write_text(json.dumps(metadata, indent=2) + '\n')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--scene', choices=['motion', 'static'], required=True)
    args = parser.parse_args()
    generate(args)


if __name__ == '__main__':
    main()
