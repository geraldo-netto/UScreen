#!/usr/bin/env python3
"""Summarize T405 capacity trials without pooling per-session percentiles."""
import argparse
import csv
import gzip
import json
from pathlib import Path
import statistics
import sys


def values(row):
    channels = row['channels']
    return dict(cpu_ms=row['cpu_ns'] / 1e6, wall_ms=row['wall_ns'] / 1e6,
                mib_per_second=row['size'] * row['frames'] * row['sessions'] / 2**20
                / (row['wall_ns'] / 1e9),
                write_p99_ms=max(c['write_p99_ns'] for c in channels) / 1e6,
                age_p99_ms=max(c['age_p99_ns'] for c in channels) / 1e6,
                pending_mib=(max(c['pending_max_bytes'] for c in channels) / 2**20
                             if row.get('sampled', True) else None),
                writes_per_frame=sum(c['writes'] for c in channels) / row['frames'] / row['sessions'],
                reads_per_frame=sum(c['reads'] for c in channels) / row['frames'] / row['sessions'],
                polls_per_frame=sum(c['polls'] for c in channels) / row['frames'] / row['sessions'],
                voluntary_switches=row['voluntary_switches'], involuntary_switches=row['involuntary_switches'])


def summarize(trials):
    grouped = {}
    for row in trials:
        key = tuple(row[k] for k in ['size', 'sessions', 'mode', 'requested_mib']) + (row.get('sampled', True),)
        grouped.setdefault(key, []).append(row)
    results = []
    for key, group in sorted(grouped.items()):
        capacities = {c['capacity'] for row in group for c in row['channels']}
        assert len(capacities) == 1, 'effective capacities differ within one comparison group'
        measured = [values(row) for row in group]
        result = dict(zip(['size', 'sessions', 'mode', 'requested_mib', 'sampled'], key))
        result.update(actual_mib=capacities.pop() / 2**20, trials=len(group), frames=group[0]['frames'])
        for metric in measured[0]:
            samples = [v[metric] for v in measured]
            if samples[0] is None:
                result.update({metric: None, metric + '_min': None, metric + '_max': None})
            else:
                result.update({metric: statistics.median(samples),
                               metric + '_min': min(samples), metric + '_max': max(samples)})
        results.append(result)
    return results


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('input', type=Path)
    parser.add_argument('--csv', action='store_true')
    args = parser.parse_args()
    opener = gzip.open if args.input.suffix == '.gz' else open
    with opener(args.input, 'rt') as stream:
        result = summarize(json.load(stream)['trials'])
    if args.csv:
        writer = csv.DictWriter(sys.stdout, fieldnames=result[0].keys(), lineterminator="\n")
        writer.writeheader()
        writer.writerows(result)
    else:
        print(json.dumps(result, indent=2))


if __name__ == '__main__':
    main()
