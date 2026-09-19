#!/usr/bin/env python3
"""T419 standalone plot of observed net battery flow, not USB input power."""
import argparse
import json
from pathlib import Path
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt

LABELS = {('text', 0, 5): 'Static H.264, 5/s', ('text', 0, 1): 'Static H.264, 1/s',
          ('text', 2, 5): 'Static Zstd, 1 draw/s', ('pen', 0, 60): 'Pen H.264, 60/s',
          ('pen', 2, 60): 'Pen Zstd, 60/s'}


def save_svg(figure, path):
    figure.savefig(path, metadata={'Date': None})
    # Matplotlib emits trailing spaces in SVG paths; keep committed docs clean.
    path.write_text('\n'.join(line.rstrip() for line in path.read_text().splitlines()) + '\n')
    plt.close(figure)


def comparison(groups, path):
    ordered = sorted(groups, key=lambda row: list(LABELS).index((row['scene'], row['codec'], row['rate'])))
    figure, axes = plt.subplots(figsize=(9, 4.6), layout='constrained')
    for index, row in enumerate(ordered):
        values = row['per_trial_mean_current_ma']
        axes.plot(values, [index] * len(values), color='#777777', linewidth=1)
        for repeat, value in enumerate(values):
            axes.scatter(value, index, s=65, color=['#225ea8', '#d95f0e'][repeat],
                         label=f'Round {repeat + 1}' if index == 0 else None, zorder=3)
    axes.axvline(0, color='#555555', linewidth=1)
    axes.set_yticks(range(len(ordered)), [LABELS[(r['scene'], r['codec'], r['rate'])] for r in ordered])
    axes.invert_yaxis()
    axes.set_xlabel('Mean signed battery current (mA): positive = charging, negative = discharging')
    axes.set_title('T419 local replay with USB connected\nTwo reversed-order rounds; first 30 measured seconds excluded')
    axes.grid(axis='x', alpha=.2)
    axes.legend(loc='best')
    save_svg(figure, path)


def timeline(trials, path):
    start = min(row['samples'][0]['ts'] for row in trials)
    first_charge = trials[0]['samples'][0]['batt.charge_uah']
    figure, axes = plt.subplots(2, 1, figsize=(11, 5.5), sharex=True, layout='constrained')
    for number, trial in enumerate(trials):
        samples = trial['samples']
        times = [(row['ts'] - start) / 60e9 for row in samples]
        color = '#225ea8' if trial['codec'] == 0 else '#d95f0e'
        axes[0].plot(times, [row['batt.current_ua'] / 1000 for row in samples], color=color, alpha=.6, linewidth=.8)
        axes[1].step(times, [(row['batt.charge_uah'] - first_charge) / 1000 for row in samples],
                     where='post', color=color, linewidth=1)
        axes[0].text((times[0] + times[-1]) / 2, 1.02, str(number + 1),
                     transform=axes[0].get_xaxis_transform(), ha='center', fontsize=9)
    axes[0].axhline(0, color='#555555', linewidth=1)
    axes[0].set_ylabel('Net current (mA)')
    axes[1].set_ylabel('Gauge change (mAh)')
    axes[1].set_xlabel('Minutes since first trace sample; numbers identify the ten phases')
    figure.suptitle('Raw battery observations, including startup/retirement\nBlue: H.264; orange: Zstd. Gaps are not interpolated.')
    for axis in axes:
        axis.grid(alpha=.2)
    save_svg(figure, path)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('summary', type=Path)
    parser.add_argument('--output', required=True, type=Path)
    args = parser.parse_args()
    result = json.loads(args.summary.read_text())
    if result['completed_phases'] != 10 or any(group['trials'] != 2 for group in result['groups']):
        raise ValueError('require the complete balanced matrix before plotting its comparison')
    args.output.mkdir(exist_ok=True, parents=True)
    comparison(result['groups'], args.output / 'battery-current.svg')
    timeline(result['trials'], args.output / 'battery-timeline.svg')


if __name__ == '__main__':
    main()
