#!/usr/bin/env python3
"""Plot T405 slow-reader receipt age, with ranges across three repetitions."""
import argparse
import gzip
import json
from pathlib import Path

import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt


def panel(axis, rows, size, title):
    for sessions, color in [(1, '#1464a5'), (2, '#b55616'), (4, '#258359')]:
        data = sorted((r for r in rows if r['size'] == size and r['sessions'] == sessions
                       and r['mode'] == 'slow'), key=lambda r: r['requested_mib'])
        x = [r['requested_mib'] for r in data]
        y = [r['age_p99_ms'] for r in data]
        error = [[r['age_p99_ms'] - r['age_p99_ms_min'] for r in data],
                 [r['age_p99_ms_max'] - r['age_p99_ms'] for r in data]]
        axis.errorbar(x, y, yerr=error, marker='o', capsize=3, color=color, label=f'Sessions: {sessions}')
    axis.set(title=title, xlabel='Requested pipe capacity (MiB)', ylabel='Worst-session receipt p99 (ms)', ylim=(0, None))
    axis.set_xticks([1, 2, 4, 8, 12, 16, 24, 32])
    axis.grid(alpha=0.2)
    axis.legend(frameon=False)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('summary', type=Path)
    parser.add_argument('output', type=Path)
    args = parser.parse_args()
    opener = gzip.open if args.summary.suffix == '.gz' else open
    with opener(args.summary, 'rt') as stream:
        rows = json.load(stream)
    figure, axes = plt.subplots(1, 2, figsize=(12, 4.7), constrained_layout=True)
    panel(axes[0], rows, 1536000, '1280×800-sized raw frames')
    panel(axes[1], rows, 8205120, '2960×1848-sized raw frames')
    figure.suptitle('More pipe capacity retains older frames under backpressure\n'
                    '120 frames/session · 60 FPS source cap · reader sleeps 20 ms/frame', fontsize=13)
    figure.supxlabel('Medians and min–max across 3 repeats; 12→16 MiB and 24→32 MiB on this kernel', fontsize=10)
    figure.savefig(args.output, dpi=160)
    plt.close(figure)


if __name__ == '__main__':
    main()
