#!/usr/bin/env python3
"""T400: paired VAAPI async-depth latency at one, two and four live encoders."""
import argparse
from concurrent.futures import ThreadPoolExecutor
import importlib.util
import json
from pathlib import Path
from types import SimpleNamespace

SPEC = importlib.util.spec_from_file_location('codec_latency', Path(__file__).with_name('codec-latency.py'))
LATENCY = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(LATENCY)


def run_group(args, meta, scene, selection, depth, clients, number):
    output = args.output / f'depth{depth}-clients{clients}-round{number}'
    output.mkdir()
    trial_args = SimpleNamespace(corpus=args.corpus, output=output, async_depth=depth, vaapi_device=args.vaapi_device)
    with ThreadPoolExecutor(max_workers=clients) as pool:
        tasks = [pool.submit(LATENCY.trial, trial_args, meta, scene, selection, index) for index in range(clients)]
        for task in tasks:
            task.result()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--corpus', type=Path, required=True)
    parser.add_argument('--selection', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--vaapi-device', default='/dev/dri/renderD128')
    args = parser.parse_args()
    meta = json.loads((args.corpus / 'metadata.json').read_text())
    scene = next(row for row in meta['scenes'] if row['scene'] == 'motion')
    selection = next(row for row in json.loads(args.selection.read_text())['selections']
                     if row['scene'] == 'motion' and row['encoder'] == 'h264_vaapi')
    args.output.mkdir()
    (args.output / 'metadata.json').write_text(json.dumps(dict(corpus=meta, selected=selection,
        clients=[1, 2, 4], depths=[1, 2], rounds=3, boundary='independent paced stock-FFmpeg processes; one GPU'), indent=2) + '\n')
    for number in range(3):
        for clients in [1, 2, 4]:
            for depth in ([1, 2] if number % 2 == 0 else [2, 1]):
                run_group(args, meta, scene, selection, depth, clients, number)


if __name__ == '__main__':
    main()
