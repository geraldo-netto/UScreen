#!/usr/bin/env python3
"""Plot the T382 raw timeline; requires matplotlib, no display interaction."""
import argparse
import json
from pathlib import Path
import re

from summarize import host_cpu, load_lines, visibility_integrity


def series(folder):
    meta = json.loads((folder / 'metadata.json').read_text())
    samples = load_lines(folder, 'samples.jsonl')
    logs = load_lines(folder, 'host-windows.jsonl')
    battery = [row for row in samples if row.get('battery', {}).get('Charge counter')]
    first_charge = int(battery[0]['battery']['Charge counter'])
    charge = [((row['utc'] - meta['start_utc']) / 60,
               (int(row['battery']['Charge counter']) - first_charge) / 1000) for row in battery]
    cpu = [((after['utc'] - meta['start_utc']) / 60,
            host_cpu(before, after, meta['host_ticks_per_second']).get('pipeline_total', float('nan')))
           for before, after in zip(samples, samples[1:])]
    latency = []
    for row in logs:
        match = re.search(r'Latency encode→display: p50 ([\d.]+)ms\s+p95 ([\d.]+)ms', row['message'])
        if match:
            latency.append(((row['utc'] - meta['start_utc']) / 60, *map(float, match.groups())))
    return meta, charge, cpu, latency


def shade(axes, phases, start):
    for phase, end in zip(phases, phases[1:]):
        color = '#b8d8f2' if phase['kind'] == 'motion' else '#eeeeee'
        alpha = 0.5 if phase['measured'] else 0.2
        for axis in axes:
            axis.axvspan((phase['utc'] - start) / 60, (end['utc'] - start) / 60,
                         color=color, alpha=alpha, linewidth=0)


def plot(folder, output):
    metadata = json.loads((folder / 'metadata.json').read_text())
    phases = load_lines(folder, 'phases.jsonl')
    _, _, reasons = visibility_integrity(folder, metadata, phases)
    if reasons:
        raise ValueError('Cannot plot invalid baseline: ' + '; '.join(reasons))
    import matplotlib
    matplotlib.use('Agg')
    import matplotlib.pyplot as plt

    meta, charge, cpu, latency = series(folder)
    fig, axes = plt.subplots(3, 1, figsize=(11, 8), sharex=True, constrained_layout=True)
    shade(axes, phases, meta['start_utc'])
    axes[0].step(*zip(*charge), where='post', color='#8f3c68', linewidth=1.5)
    axes[0].set_ylabel('Battery charge change (mAh)')
    axes[1].plot(*zip(*cpu), color='#25643d', linewidth=1)
    axes[1].set_ylabel('Host pipeline CPU (%)\n100% = one core')
    times, p50, p95 = zip(*latency)
    axes[2].plot(times, p50, label='Window p50', linewidth=1)
    axes[2].plot(times, p95, label='Window p95', linewidth=1)
    axes[2].set_ylabel('Packet ready → ACK (ms)')
    axes[2].set_xlabel('Elapsed minutes; blue = motion, gray = static; pale = warm-up')
    axes[2].legend(loc='upper right')
    for axis in axes:
        axis.grid(axis='y', alpha=0.25)
    fig.suptitle(f'UScreen {meta["source_commit"][:7]} — one USB tablet, H.264 VAAPI 1280×800 / 60 fps target')
    fig.savefig(output, dpi=160)
    plt.close(fig)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('folder', type=Path)
    parser.add_argument('output', type=Path)
    args = parser.parse_args()
    plot(args.folder, args.output)
