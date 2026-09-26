#!/usr/bin/env python3
"""Plot retained T607 measurements; separate microbenchmark from real USB replay."""
import argparse
import json
from pathlib import Path
import statistics
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt

COLORS = ['#718096', '#718096', '#718096', '#718096', '#008775', '#497cb6', '#497cb6', '#497cb6', '#cc7044']


def save(fig, output, name):
    svg = output / (name + '.svg')
    fig.savefig(svg, bbox_inches='tight')
    svg.write_text('\n'.join(line.rstrip() for line in svg.read_text().splitlines()) + '\n')
    fig.savefig(output / (name + '.png'), dpi=160, bbox_inches='tight')
    plt.close(fig)


def bars(ax, labels, values, title, ylabel, colors):
    ax.bar(labels, values, color=colors)
    ax.set(title=title, ylabel=ylabel, xlabel='Application chunk size')
    ax.grid(axis='y', alpha=.2)
    ax.spines[['top', 'right']].set_visible(False)


def synthetic(data, output):
    rows = data['summary']
    labels = ['512 B', '1 KiB', '2 KiB', '4 KiB', '8 KiB', '12 KiB', '16 KiB', '32 KiB', '64 KiB']
    fig, axes = plt.subplots(1, 3, figsize=(16, 4.8), constrained_layout=True)
    keys = [('cpu_ns', 1e6, 'Writer CPU — lower is better', 'ms / 1,200 packets'),
            ('allocated_bytes', 1024, 'ART allocation — log scale', 'KiB / 1,200 packets'),
            ('peak_staging_bytes', 1024, 'Peak logical staging', 'KiB')]
    for ax, (key, scale, title, unit) in zip(axes, keys):
        bars(ax, labels, [r[key]/scale for r in rows.values()], title, unit, COLORS)
        ax.tick_params(axis='x', labelrotation=45)
    axes[1].set_yscale('log')
    fig.suptitle('T607 · Synthetic sink on tablet · 8 interleaved repeats · Okio 3.6.0\nGreen = current 8 KiB; no camera/codec/USB cost in this measurement')
    save(fig, output, 'chunk-local')


def network(data, output):
    rows = data['rows']
    chunks = [8192, 12288, 16384, 32768, 65536]
    labels = [str(c//1024) + ' KiB' for c in chunks]
    fig, axes = plt.subplots(1, 2, figsize=(11, 4.8), constrained_layout=True)
    for ax, (key, scale, title, unit) in zip(axes, [
            ('mbps', 1, 'ADB throughput — higher is better', 'Mb/s'),
            ('cpu_ns', 1e6, 'Android writer CPU — lower is better', 'ms / 3,000 encoded packets')]):
        values = [[r[key]/scale for r in rows if r['chunk'] == c] for c in chunks]
        bars(ax, labels, [statistics.median(v) for v in values], title, unit, COLORS[4:])
        for index, samples in enumerate(values):
            ax.scatter([index] * len(samples), samples, color='#172b4d', s=14, zorder=3)
    fig.suptitle('T607 · Real tablet → USB/ADB → host · Untraced interleaved runs\n120,000 / 120,000 packets byte-exact; larger chunks show no clear throughput gain')
    save(fig, output, 'chunk-adb')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--synthetic', type=Path, required=True)
    parser.add_argument('--network', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    synthetic(json.loads(args.synthetic.read_text()), args.output)
    network(json.loads(args.network.read_text()), args.output)


if __name__ == '__main__':
    main()
