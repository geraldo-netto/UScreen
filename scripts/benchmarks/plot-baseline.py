#!/usr/bin/env python3
"""Plot the T382 raw timeline; requires matplotlib, no display interaction."""
import argparse
import json
import math
from pathlib import Path
import re

from summarize import PATTERNS, host_cpu, load_lines, visibility_integrity, summarize


def series(folder):
    meta = json.loads((folder / 'metadata.json').read_text())
    samples = load_lines(folder, 'samples.jsonl')
    logs = load_lines(folder, 'host-windows.jsonl')
    battery = [row for row in samples if row.get('battery', {}).get('Charge counter')]
    first_charge = int(battery[0]['battery']['Charge counter']) if battery else 0
    charge = [((row['utc'] - meta['start_utc']) / 60,
               (int(row['battery']['Charge counter']) - first_charge) / 1000) for row in battery]
    cpu = [((after['utc'] - meta['start_utc']) / 60,
            host_cpu(before, after, meta['host_ticks_per_second']).get('pipeline_total', float('nan')))
           for before, after in zip(samples, samples[1:])]
    latency = []
    for row in logs:
        match = re.search(PATTERNS['packet_ready_to_ack_window'], row['message'])
        if match:
            latency.append(((row['utc'] - meta['start_utc']) / 60, *map(float, match.groups()[:2])))
    return meta, charge, cpu, latency


def shade(axes, phases, start):
    for phase, end in zip(phases, phases[1:]):
        color = '#b8d8f2' if phase['kind'] == 'motion' else '#eeeeee'
        alpha = 0.5 if phase['measured'] else 0.2
        for axis in axes:
            axis.axvspan((phase['utc'] - start) / 60, (end['utc'] - start) / 60,
                         color=color, alpha=alpha, linewidth=0)


def missing(axis):
    axis.text(0.5, 0.5, 'No observations', ha='center', va='center', transform=axis.transAxes)


def draw_traces(axes, charge, cpu, latency):
    if charge:
        axes[0].step(*zip(*charge), where='post', color='#8f3c68', linewidth=1.5)
    else:
        missing(axes[0])
    axes[0].set_ylabel('Battery charge change (mAh)')
    if any(math.isfinite(row[1]) for row in cpu):
        axes[1].plot(*zip(*cpu), color='#25643d', linewidth=1)
    else:
        missing(axes[1])
    axes[1].set_ylabel('Host pipeline CPU (%)\n100% = one core')
    if latency:
        times, p50, p95 = zip(*latency)
        axes[2].plot(times, p50, label='Window p50', linewidth=1)
        axes[2].plot(times, p95, label='Window p95', linewidth=1)
        axes[2].legend(loc='upper right')
    else:
        missing(axes[2])
    axes[2].set_ylabel('Packet ready → ACK (ms)')
    axes[2].set_xlabel('Elapsed minutes; blue = motion, gray = static; pale = warm-up')
    for axis in axes:
        axis.grid(axis='y', alpha=0.25)


def plot(folder, output):
    metadata = json.loads((folder / 'metadata.json').read_text())
    phases = load_lines(folder, 'phases.jsonl')
    _, guarded, reasons = visibility_integrity(folder, metadata, phases)
    if metadata.get('observation_guard_version') == 1:
        reasons.extend(summarize(folder)['invalid_reasons'])
    if reasons:
        raise ValueError('Cannot plot invalid baseline: ' + '; '.join(reasons))
    import matplotlib
    matplotlib.use('Agg')
    import matplotlib.pyplot as plt

    meta, charge, cpu, latency = series(folder)
    fig, axes = plt.subplots(3, 1, figsize=(11, 8), sharex=True, constrained_layout=True)
    try:
        shade(axes, phases, meta['start_utc'])
        draw_traces(axes, charge, cpu, latency)
        visibility = 'verified' if guarded else 'unverified'
        fig.suptitle(f'UScreen {meta["source_commit"][:7]} — {meta.get("geometry", "geometry unrecorded")} '
                     f'— visibility {visibility}')
        fig.savefig(output, dpi=160)
    finally:
        plt.close(fig)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('folder', type=Path)
    parser.add_argument('output', type=Path)
    args = parser.parse_args()
    plot(args.folder, args.output)
