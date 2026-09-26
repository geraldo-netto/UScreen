#!/usr/bin/env python3
"""T600: exact production conversion pool; isolated CPU replay, no device capture."""
import argparse
import importlib.util
from pathlib import Path
import subprocess
import thread_ramp_common as C


def build(folder, dimensions):
    spec = importlib.util.spec_from_file_location('conversion', Path(__file__).with_name('conversion.py'))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    # Keep the normal harness and production source byte-for-byte.
    binary, sources = module.build(folder, None, dimensions=dimensions)
    return module, binary, sources


def run(args):
    args.output.mkdir(parents=True)
    module, binary, sources = build(args.output / 'build', (args.width, args.height))
    results = []
    info = C.metadata([Path(__file__), Path(C.__file__), Path(module.__file__),
                       C.ROOT / 'scripts/benchmarks/conversion.c'])
    info.update(production_sources=sources, width=args.width, height=args.height,
                boundary='scale-1 isolated unpaced conversion; no encode/USB/render', samples=256)
    C.save(args.output / 'metadata.json', info)
    for repeat, workers in C.order():
        for damage in [1, 3]:
            row = module.measure(binary, info['affinity'], 1, workers, 1, damage, 1, 256)
            row.update(repeat=repeat)
            results.append(row)
        C.save(args.output / 'results.json', results)
        print('conversion', repeat, workers, flush=True)
    module.verify_checksums([dict(row, variant='production') for row in results])


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--width', type=int, default=1280)
    parser.add_argument('--height', type=int, default=800)
    run(parser.parse_args())
