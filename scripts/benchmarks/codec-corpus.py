#!/usr/bin/env python3
"""T400: synthetic RGB desktop/text/pen pictures converted once to capture NV12."""
import argparse
import hashlib
import json
import math
from pathlib import Path
import subprocess
from PIL import Image, ImageDraw, ImageFont

WIDTH, HEIGHT, FPS = 1280, 800, 60


def desktop(font):
    image = Image.new('RGB', (WIDTH, HEIGHT), '#151b25')
    draw = ImageDraw.Draw(image)
    draw.rectangle((0, 0, WIDTH, 42), fill='#303949')
    draw.text((24, 8), 'UScreen synthetic desktop / codec research', font=font, fill='white')
    lines = ['fn render(frame: &Frame) -> Result<()> {', '    let sequence = frame.sequence();',
             '    queue_pixels(frame.data(), sequence)?;', '    Ok(())', '}',
             '0123456789  abcdefghijklmnopqrstuvwxyz', 'ABCDEFGHIJKLMNOPQRSTUVWXYZ',
             '1px edges | [] {} () <> / \\ + - = @ # %']
    for row in range(28):
        color = ['#dde6ee', '#54dac8', '#ffda75', '#95b8ff'][row % 4]
        draw.text((24, 62 + 24 * row), lines[row % len(lines)], font=font, fill=color)
    draw.rectangle((650, 58, 1256, 770), outline='#707d91', width=1)
    return image


def paint(scene, base, number):
    image = base.copy()
    draw = ImageDraw.Draw(image)
    if scene == 'pen':
        points = [(670 + point * 2, 400 + int(160 * math.sin(point / 17))) for point in range(number + 2)]
        draw.line(points, fill='#63e7ee', width=3)
        x, y = points[-1]
        draw.ellipse((x-6, y-6, x+6, y+6), fill='white')
    elif scene == 'motion':
        for row in range(14):
            for col in range(12):
                x = 650 + ((col * 55 + number * 7) % 606)
                y = 58 + row * 50
                color = ((col*41+number*3) % 256, (row*31+number) % 256, (col*17+row*13) % 256)
                draw.rectangle((x, y, min(x+45, 1255), y+40), fill=color)
    return image


def corpus(scene, args, base):
    target = args.output / (scene + '.nv12')
    command = ['ffmpeg', '-hide_banner', '-loglevel', 'warning', '-f', 'rawvideo', '-pixel_format', 'rgb24',
               '-video_size', f'{WIDTH}x{HEIGHT}', '-framerate', str(FPS), '-i', 'pipe:0',
               '-vf', 'scale=in_range=full:out_range=limited:out_color_matrix=bt709,format=nv12',
               '-frames:v', str(args.frames), '-f', 'rawvideo', '-y', str(target)]
    with (args.output / (scene + '-generation.log')).open('w') as log:
        child = subprocess.Popen(command, stdin=subprocess.PIPE, stderr=log)
        try:
            for number in range(args.frames):
                child.stdin.write(paint(scene, base, number).tobytes())
            child.stdin.close()
            if child.wait(timeout=60) != 0:
                raise RuntimeError(f'corpus conversion failed: {scene}')
        finally:
            if child.poll() is None:
                child.kill()
                child.wait()
    digest = hashlib.file_digest(target.open('rb'), 'sha256').hexdigest()
    return dict(scene=scene, path=target.name, sha256=digest, bytes=target.stat().st_size, command=command)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--font', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--frames', type=int, default=240)
    args = parser.parse_args()
    if not 60 <= args.frames <= 300:
        parser.error('frames must be 60..300')
    args.output.mkdir()
    base = desktop(ImageFont.truetype(str(args.font), 18))
    rows = [corpus(scene, args, base) for scene in ['text', 'pen', 'motion']]
    metadata = dict(width=WIDTH, height=HEIGHT, fps=FPS, frames=args.frames, pixel_format='nv12',
                    font=str(args.font), font_sha256=hashlib.sha256(args.font.read_bytes()).hexdigest(),
                    ffmpeg=subprocess.check_output(['ffmpeg', '-version'], text=True), scenes=rows)
    (args.output / 'metadata.json').write_text(json.dumps(metadata, indent=2) + '\n')
    for row in rows:
        print(row['scene'], row['sha256'], flush=True)


if __name__ == '__main__':
    main()
