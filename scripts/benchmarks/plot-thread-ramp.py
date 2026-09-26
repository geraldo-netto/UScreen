#!/usr/bin/env python3
"""T600: median and observed min/max of three trials, not confidence intervals."""
import argparse
import json
import statistics
from pathlib import Path
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import thread_ramp_common as C


def panel(axis, rows, metric, title, unit, factor=1):
    groups = [[row[metric] * factor for row in rows if row['workers'] == n] for n in C.LEVELS]
    medians = [statistics.median(group) for group in groups]
    lower = [median - min(group) for median, group in zip(medians, groups)]
    upper = [max(group) - median for median, group in zip(medians, groups)]
    axis.errorbar(range(8), medians, yerr=[lower, upper], marker='o', capsize=4, color='#126782')
    axis.set(title=title, ylabel=unit, xlabel='Requested threads', xticks=range(8), xticklabels=C.LEVELS)
    axis.grid(alpha=.22)
    axis.set_ylim(bottom=0)


def main(args):
    data = json.loads(args.input.read_text())['trials']
    fig, axes = plt.subplots(2, 2, figsize=(11, 7), constrained_layout=True)
    panel(axes[0, 0], data['encoder'], 'cpu_seconds', 'Encoder CPU per 360-frame run', 'CPU seconds')
    panel(axes[0, 1], data['encoder'], 'ack_ms_p95', 'USB render-ACK latency, p95', 'Milliseconds')
    conversion = [row for row in data['conversion_1280'] if row['damage'] == 'full']
    panel(axes[1, 0], conversion, 'convert_p50_us', '1280×800 conversion, p50', 'Microseconds')
    panel(axes[1, 1], data['runtime'], 'loopback_us_p95', 'Isolated runtime loopback latency, p95', 'Microseconds')
    fig.suptitle('Blent thread ramp · three shuffled passes\nBars show observed trial ranges; each pool varied separately', fontsize=14)
    fig.supxlabel('Conversion dispatch caps at 4 jobs; x264 slice workers cap at 12 for 800-pixel height.\nRuntime replay excludes the full daemon and tablet; ACK timing is not optical latency.', fontsize=9)
    fig.savefig(args.output.with_suffix('.png'), dpi=160)
    svg = args.output.with_suffix('.svg')
    fig.savefig(svg)
    svg.write_text('\n'.join(line.rstrip() for line in svg.read_text().splitlines()) + '\n')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--input', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    main(parser.parse_args())
