"""T600: shared metadata and ordering for bounded, single-session thread ramps."""
import hashlib
import json
import os
from pathlib import Path
import platform
import random
import subprocess

ROOT = Path(__file__).resolve().parents[2]
LEVELS = [1, 2, 4, 8, 12, 16, 24, 32]


def save(path, value):
    path.write_text(json.dumps(value, indent=2) + '\n')


def order(repeats=3):
    for repeat in range(repeats):
        levels = LEVELS.copy()
        random.Random(600 + repeat).shuffle(levels)
        for workers in levels:
            yield repeat, workers


def identity(paths):
    return {str(p): hashlib.sha256(p.read_bytes()).hexdigest() for p in paths}


def metadata(paths):
    return dict(revision=subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip(),
                platform=platform.platform(), affinity=sorted(os.sched_getaffinity(0)),
                sources=identity(paths), levels=LEVELS, ordering='seeded shuffle, seeds 600+repeat')


def percentile(values, fraction):
    ordered = sorted(values)
    return ordered[max(0, __import__('math').ceil(len(ordered) * fraction) - 1)]
