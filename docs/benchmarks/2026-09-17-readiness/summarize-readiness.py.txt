#!/usr/bin/env python3
"""Summarize T405 trial medians/ranges, retaining worst-lane nearest-rank p99."""
import argparse
import collections
import json
import math
from pathlib import Path
import statistics


def p99(values):
    return sorted(values)[math.ceil(len(values) * 0.99) - 1]


def metrics(row):
    ages = [p99(reader['ages_ns']) / 1e6 for reader in row['readers'] if reader['ages_ns']]
    stop = row['cancel_ns']
    return dict(cpu_ms=row['cpu_ns'] / 1e6,
                reads=sum(reader['reads'] for reader in row['readers']),
                writes=sum(writer[0] for writer in row['writers_write_poll']),
                polls=sum(writer[1] for writer in row['writers_write_poll']),
                voluntary_switches=row['voluntary_switches'],
                receipt_p99_ms=max(ages) if ages else None,
                cancel_ms=(max(reader['ended_ns'] for reader in row['readers']) - stop) / 1e6 if stop else None)


def summarize(rows):
    values = [metrics(row) for row in rows]
    result = dict(trials=len(rows), capacities=sorted({cap for row in rows for cap in row['capacities']}))
    for name in values[0]:
        samples = [row[name] for row in values if row[name] is not None]
        if samples:
            result[name] = dict(median=statistics.median(samples), minimum=min(samples), maximum=max(samples))
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source', type=Path)
    parser.add_argument('output', type=Path)
    args = parser.parse_args()
    source = json.loads(args.source.read_text())
    groups = collections.defaultdict(list)
    keys = ['variant', 'size', 'sessions', 'frames', 'mode']
    for row in source['trials']:
        groups[tuple(row[key] for key in keys)].append(row)
    results = [dict(zip(keys, key), **summarize(rows)) for key, rows in sorted(groups.items())]
    args.output.write_text(json.dumps(results, indent=2) + '\n')


if __name__ == '__main__':
    main()
