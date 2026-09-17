#!/usr/bin/env python3
"""Summarize T389 repeated trials without treating transport FPS as display FPS."""
import argparse
from collections import defaultdict
import json
import math
from pathlib import Path
import statistics


def percentile(values, fraction):
    ordered = sorted(values)
    return ordered[math.floor(fraction * (len(ordered) - 1))]


def metrics(row):
    streams = row['streams']
    frames = row['frames'] * row['sessions']
    ages = [n / 1e6 for stream in streams for n in stream['age_ns']]
    return dict(fps=frames * 1e9 / max(s['ns'] for s in streams),
                cpu_ms_per_frame=sum(s['cpu_ns'] for s in streams) / frames / 1e6,
                p50_ms=percentile(ages, .5), p95_ms=percentile(ages, .95), p99_ms=percentile(ages, .99),
                reads_per_frame=sum(s['reads'] for s in streams) / frames)


def distribution(values):
    return dict(median=statistics.median(values), minimum=min(values), maximum=max(values))


def groups(path, fields):
    source = json.loads(path.read_text())
    groups = defaultdict(list)
    for row in source['trials']:
        groups[tuple(row[field] for field in fields)].append(row)
    result = []
    for key, rows in sorted(groups.items()):
        measured = [metrics(row) for row in rows]
        group = dict(zip(fields, key), trials=len(rows))
        group['metrics'] = {name: distribution([m[name] for m in measured]) for name in measured[0]}
        if 'touch_ns' in rows[0]['streams'][0]:
            group['allocation_name'] = group.pop('allocation')
            group['allocation'] = {name: distribution([stream[name] for row in rows for stream in row['streams']])
                                   for name in ['touch_ns', 'touch_minor_faults', 'touch_major_faults', 'huge_kb', 'advice_accepted']}
        result.append(group)
    return result


def med(group, field):
    return group['metrics'][field]['median']


def raw_table(groups):
    rows = ['| Geometry | Streams | Rows FPS | Direct FPS | Rows p99 ms | Direct p99 ms |',
            '| --- | ---: | ---: | ---: | ---: | ---: |']
    indexed = {(g['width'], g['height'], g['sessions'], g['mode']): g for g in groups}
    for key, direct in sorted(indexed.items()):
        width, height, sessions, mode = key
        if mode != 'direct':
            continue
        baseline = indexed[width, height, sessions, 'rows']
        rows.append(f'| {width}×{height} | {sessions} | {med(baseline, "fps"):.1f} | {med(direct, "fps"):.1f} | '
                    f'{med(baseline, "p99_ms"):.3f} | {med(direct, "p99_ms"):.3f} |')
    return '\n'.join(rows)


def ring_table(groups):
    rows = ['| Frame bytes | Streams | Transport / allocation | FPS | p99 ms | Actual huge KiB | Touch faults |',
            '| ---: | ---: | --- | ---: | ---: | ---: | ---: |']
    for group in groups:
        allocation = group['allocation']
        rows.append(f'| {group["size"]:,} | {group["sessions"]} | {group["transport"]} / {group["allocation_name"]} | '
                    f'{med(group, "fps"):.1f} | {med(group, "p99_ms"):.3f} | '
                    f'{allocation["huge_kb"]["median"]:.0f} | {allocation["touch_minor_faults"]["median"]:.0f} |')
    return '\n'.join(rows)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--raw', type=Path, required=True)
    parser.add_argument('--ring', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    raw = groups(args.raw, ['width', 'height', 'sessions', 'mode'])
    ring = groups(args.ring, ['size', 'sessions', 'transport', 'allocation'])
    args.output.mkdir(exist_ok=True)
    (args.output / 'summary.json').write_text(json.dumps(dict(raw=raw, ring=ring), indent=2) + '\n')
    (args.output / 'tables.md').write_text(raw_table(raw) + '\n\n' + ring_table(ring) + '\n')


if __name__ == '__main__':
    main()
