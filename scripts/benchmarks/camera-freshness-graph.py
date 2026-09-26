#!/usr/bin/env python3
"""Plot measured host-decoded frame arrival, never inferred display latency."""
import json
from pathlib import Path
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt

ROOT = Path(__file__).resolve().parents[2]
DATA = ROOT / 'docs/reviews/artifacts/2026-09-26-camera/freshness/t618'


def main():
    fig, axes = plt.subplots(2, 1, figsize=(9, 6), sharex=True, layout='constrained')
    for filename, label, color in [('native-clean-proxy.json', 'Clean USB route', '#2673b8'),
                                   ('native-stall.json', 'One 250 ms feedback stall', '#b85f26')]:
        sample = json.loads((DATA / filename).read_text())
        times = [value / 1000 for value in sample['decoded_ms']]
        axes[0].step(times, range(1, len(times) + 1), where='post', label=label, color=color)
        gaps = [1000 * (b - a) for a, b in zip(times, times[1:])]
        axes[1].plot(times[1:], gaps, color=color, alpha=.8)
    axes[0].set(ylabel='Decoded frames')
    axes[0].legend(loc='upper left')
    axes[1].set(xlabel='Seconds after host invitation', ylabel='Gap between decoded frames (ms)')
    for axis in axes:
        axis.axvline(2.007, color='#777', linestyle=':', linewidth=1)
        axis.grid(alpha=.2)
    fig.suptitle('Real tablet camera: recovery after stalled feedback\n'
                 '1280×720 · 30 FPS · 150 ms admission budget\n'
                 'Host decoder output; no V4L2 or presentation measurement', fontsize=11)
    for extension in ('png', 'svg'):
        output = DATA / f'native-recovery.{extension}'
        fig.savefig(output, dpi=150)
        if extension == 'svg':
            output.write_text('\n'.join(line.rstrip() for line in output.read_text().splitlines()) + '\n')
    plt.close(fig)


if __name__ == '__main__':
    main()
