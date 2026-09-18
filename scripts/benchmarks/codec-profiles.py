#!/usr/bin/env python3
"""T400: isolate stock VAAPI H.264 profile/entropy controls and preserve fixtures."""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import shutil
from types import SimpleNamespace


def module(name):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(name + '.py'))
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


HOST, FIXTURE = module('codec-host'), module('codec-fixtures')


def trial(args, meta, scene, profile, coder):
    name = f'{scene["scene"]}-{profile}-{coder}'
    folder = args.output / name
    folder.mkdir()
    encoded = folder / 'encoded.mkv'
    command = HOST.command_for(args, meta, scene, 'h264_vaapi', 18, encoded)
    command[-2:-2] = ['-profile:v', profile, '-coder', coder, '-async_depth', '1']
    result = HOST.encode(command, folder)
    result.update(scene=scene['scene'], profile=profile, coder=coder, quantizer=18, reference_sha256=scene['sha256'])
    if result['returncode'] != 0:
        raise RuntimeError(f'profile encode failed: {folder}')
    result.update(HOST.inspect(encoded, folder, args.corpus / scene['path'], meta))
    raw = folder / 'stream.h264'
    result['fixture_command'] = FIXTURE.elementary(encoded, 'h264', raw)
    frames = FIXTURE.annex_frames(raw)
    fixture = folder / 'stream.bin'
    FIXTURE.write_fixture(fixture, 'video/avc', meta, FIXTURE.config_for(frames[0], 'h264'), frames)
    result.update(encoded_bytes=encoded.stat().st_size, fixture_sha256=hashlib.sha256(fixture.read_bytes()).hexdigest())
    (folder / 'result.json').write_text(json.dumps(result, indent=2) + '\n')
    print(name, result['encoded_bytes'], result['psnr_db'], flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--corpus', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--vaapi-device', default='/dev/dri/renderD128')
    args = parser.parse_args()
    meta = json.loads((args.corpus / 'metadata.json').read_text())
    args.output.mkdir()
    shutil.copy2(args.corpus / 'metadata.json', args.output / 'metadata.json')
    for path in [Path(__file__), Path(HOST.__file__), Path(FIXTURE.__file__)]:
        shutil.copy2(path, args.output / (path.name + '.txt'))
    for scene in meta['scenes']:
        for profile, coder in [('high', 'cabac'), ('high', 'cavlc'), ('constrained_baseline', 'cavlc')]:
            trial(args, meta, scene, profile, coder)


if __name__ == '__main__':
    main()
