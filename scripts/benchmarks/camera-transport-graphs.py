#!/usr/bin/env python3
"""T608 measurement plots plus historical, already-implemented T594/T613 gains."""
import argparse
import gzip
import json
from pathlib import Path
import statistics
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import numpy as np

ROOT = Path(__file__).resolve().parents[2]
COLORS = {'tcp': '#008775', 'udp': '#9b5fb5'}


def save(fig, root, name):
    svg = root / (name + '.svg')
    fig.savefig(svg, bbox_inches='tight')
    svg.write_text('\n'.join(line.rstrip() for line in svg.read_text().splitlines()) + '\n')
    fig.savefig(root / (name + '.png'), dpi=160, bbox_inches='tight')
    plt.close(fig)


def median(rows, key):
    values = [r['quality'][key] for r in rows if r['quality'][key] is not None]
    return statistics.median(values) if values else float('nan')


def subset(data, protocol, scenario):
    return [r for r in data['rows'] if r['protocol'] == protocol and r['scenario'] == scenario]


def transport(data, output):
    scenarios = ['clean', 'loss-jitter', 'pressure']
    fig, axes = plt.subplots(1, 2, figsize=(12, 5), constrained_layout=True)
    for offset, protocol in enumerate(['tcp', 'udp']):
        groups = [subset(data, protocol, name) for name in scenarios]
        x = np.arange(3) + (offset-.5)*.32
        p95 = [median(g, 'age_p95_ms') for g in groups]
        axes[0].bar(x, p95, width=.3, color=COLORS[protocol], label=protocol.upper())
        axes[0].scatter(x, [median(g, 'age_p99_ms') for g in groups], color='#172b4d', marker='_', s=100)
        axes[1].bar(x, [median(g, 'within_150ms')/3 for g in groups], width=.3, color=COLORS[protocol])
    axes[0].set(yscale='log', ylabel='Frame age at receiver (ms, log scale)', title='Latency of surviving decodable frames\nBars: p95; dark marks: p99')
    axes[0].legend()
    axes[0].text(2.16, .6, 'UDP: no\ndecodable\nframes', ha='center', fontsize=9)
    axes[1].set(ylabel='Percent of all generated frames', ylim=(0, 108), title='Decodable frames arriving within 150 ms\nBoth transports fail the 2 Mb/s case at 3 Mb/s source')
    for ax in axes:
        ax.set_xticks(range(3), ['Clean', 'Loss + jitter', '2 Mb/s pressure'])
        ax.grid(axis='y', alpha=.2)
        ax.spines[['top', 'right']].set_visible(False)
    fig.suptitle('T608 · Real TCP / authenticated datagram probe · 1280×720, 30 FPS, 3 Mb/s\nThree trials in private network namespaces; prototype UDP is not WebRTC/SRTP')
    save(fig, output, 'transport-tradeoffs')


def budget(data, low, output):
    selections = [(data, 'tcp'), (data, 'udp'), (low, 'tcp'), (low, 'udp')]
    groups = [subset(d, p, 'pressure') for d, p in selections]
    labels = ['TCP\n3 Mb/s', 'UDP\n3 Mb/s', 'TCP\n1 Mb/s', 'UDP\n1 Mb/s']
    fig, axes = plt.subplots(1, 2, figsize=(11, 5), constrained_layout=True)
    colors = [COLORS[p] for _, p in selections]
    axes[0].bar(labels, [median(g, 'within_150ms')/3 for g in groups], color=colors)
    axes[0].set(ylabel='Percent of generated frames', ylim=(0, 108), title='Frames decoded intact within 150 ms')
    axes[1].bar(labels, [median(g, 'age_p95_ms') for g in groups], color=colors)
    axes[1].set(yscale='log', ylabel='Receiver frame age p95 (ms)', title='Match source rate to available bandwidth')
    axes[1].text(1, 50, 'No decodable\nframes', ha='center', fontsize=9)
    for ax in axes:
        ax.grid(axis='y', alpha=.2)
        ax.spines[['top', 'right']].set_visible(False)
    fig.suptitle('T608 · Same 2 Mb/s impaired route; lower source bitrate fixes overload\nExisting camera bitrate setting; lower bitrate has a separate image-quality cost')
    save(fig, output, 'transport-bitrate')


def historical(output):
    root = ROOT / 'docs/reviews/artifacts/2026-09-26-performance/t594'
    workloads = []
    for name in ['camera-baseline', 'camera-segmented']:
        data = json.loads((root / name / 'summary.json').read_text())
        workloads.append([r for trial in data for r in trial['workloads'] if r['name'] == 'camera-packets'])
    summary = ROOT / 'docs/reviews/artifacts/2026-09-26-followup/t600/steady30/summary.json.gz'
    host = json.loads(gzip.decompress(summary.read_bytes()))
    cpu = [statistics.median(r['total_cpu_seconds'] for r in host if r['trial'].endswith(name)) for name in ['baseline', 'encoder1']]
    panels = [([statistics.median(r['allocated_bytes'] for r in w)/1024 for w in workloads], 'T594 · Camera allocation', 'KiB / 1,200 synthetic packets', ['Whole payload', '8 KiB staging']),
              ([statistics.median(r['elapsed_ns'] for r in w)/1e6 for w in workloads], 'T594 · Packet workload time', 'ms / 1,200 synthetic packets', ['Whole payload', '8 KiB staging']),
              (cpu, 'T600 → T613 · Host CPU', 'CPU seconds / 20 s scene', ['Auto workers', '1 encoder worker'])]
    fig, axes = plt.subplots(1, 3, figsize=(14, 4.8), constrained_layout=True)
    for ax, (values, title, unit, labels) in zip(axes, panels):
        bars = ax.bar(labels, values, color=['#718096', '#008775'])
        ax.bar_label(bars, labels=[f'{v:,.2f}' for v in values], padding=3)
        ax.set(title=title, ylabel=unit)
        ax.text(.97, .93, f'{100*(1-values[1]/values[0]):.2f}% less', ha='right', transform=ax.transAxes, fontsize=14)
        ax.grid(axis='y', alpha=.2)
        ax.spines[['top', 'right']].set_visible(False)
    fig.suptitle('Previously implemented improvements · Historical matched measurements\nCamera packet tests and 1280×800 host scene are separate workloads; gains are not additive')
    save(fig, output, 'implemented-improvements')


def read(path):
    data = path.read_bytes()
    return json.loads(gzip.decompress(data) if path.suffix == '.gz' else data)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--results', type=Path, required=True)
    parser.add_argument('--lowrate', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    data, low = read(args.results), read(args.lowrate)
    transport(data, args.output)
    budget(data, low, args.output)
    historical(args.output)


if __name__ == '__main__':
    main()
