#!/usr/bin/env python3
"""Generate the bounded, sensor-free H264 fixture shared by T607 and T608."""
import argparse
import hashlib
import json
from pathlib import Path
import struct
import subprocess


def generate(output, bitrate=3000):
    output.mkdir(parents=True)
    encoded = output / 'source.h264'
    command = ['ffmpeg', '-v', 'error', '-f', 'lavfi', '-i', 'testsrc2=size=1280x720:rate=30',
               '-frames:v', '300', '-c:v', 'libx264', '-threads', '1', '-profile:v', 'baseline',
               '-tune', 'zerolatency', '-preset', 'ultrafast', '-b:v', f'{bitrate}k', '-maxrate', f'{bitrate}k',
               '-bufsize', f'{bitrate}k', '-g', '30', '-keyint_min', '30', '-sc_threshold', '0',
               '-x264-params', 'aud=1', '-f', 'h264', str(encoded)]
    subprocess.run(command, check=True, timeout=60)
    probe = subprocess.check_output(['ffprobe', '-v', 'error', '-show_packets', '-show_entries',
                                    'packet=pos,size,flags', '-of', 'json', str(encoded)], timeout=20)
    (output / 'packets.json').write_bytes(probe)
    data = encoded.read_bytes()
    packets = json.loads(probe)['packets']
    write_framed(output / 'camera-replay.bin', packets, data)
    metadata = dict(command=command, ffmpeg=subprocess.check_output(['ffmpeg', '-version'], text=True),
                    sha256=hashlib.sha256(data).hexdigest(), frames=len(packets), width=1280, height=720, fps=30, bitrate_kbps=bitrate)
    (output / 'fixture.json').write_text(json.dumps(metadata, indent=2) + '\n')


def write_framed(path, packets, data):
    with path.open('wb') as target:
        target.write(struct.pack('!I', len(packets)))
        for row in packets:
            size, pos = int(row['size']), int(row['pos'])
            assert 0 < size <= 2 * 1024 * 1024
            target.write(struct.pack('!I', size))
            target.write(data[pos:pos+size])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--bitrate', type=int, choices=[1000, 3000], default=3000)
    args = parser.parse_args()
    generate(args.output, args.bitrate)


if __name__ == '__main__':
    main()
