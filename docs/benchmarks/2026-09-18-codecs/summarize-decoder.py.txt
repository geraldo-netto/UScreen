#!/usr/bin/env python3
"""T386: preserve trial boundaries and distinguish render reports from callbacks."""
import argparse
from collections import defaultdict
import json
import math
from pathlib import Path
import statistics


def percentile(values, fraction):
    ordered = sorted(values)
    return ordered[math.floor(fraction * (len(ordered) - 1))] if ordered else None


def distribution(values):
    present = [v for v in values if v is not None]
    if not present:
        return None
    return dict(median=statistics.median(present), minimum=min(present), maximum=max(present), count=len(present))


def switches(row):
    before = {t['tid']: t for t in row['before']['threads']}
    after = {t['tid']: t for t in row['after']['threads']}
    names = ['voluntary_ctxt_switches', 'nonvoluntary_ctxt_switches']
    total = sum(int(after[tid][key]) - int(before[tid][key]) for tid in before.keys() & after.keys() for key in names)
    return dict(surviving_thread_switches=total, new_threads=len(after.keys() - before.keys()),
                retired_threads=len(before.keys() - after.keys()))


def trace_metrics(row):
    result = {}
    for name, start, end in [('feed_to_release', 1, 2), ('feed_to_reported_render', 1, 4),
                             ('render_to_notification', 4, 3), ('feed_to_ack', 1, 5)]:
        values = [(r[end] - r[start]) / 1e6 for r in row.get('trace', []) if r[end] and r[start]]
        result[name + '_count'] = len(values)
        result[name + '_negative'] = sum(v < 0 for v in values)
        for suffix, fraction in [('p50_ms', .5), ('p95_ms', .95), ('p99_ms', .99)]:
            result[name + '_' + suffix] = percentile(values, fraction)
    return result


def metrics(row):
    before, after = row['before'], row['after']
    seconds = (after['elapsed_ns'] - before['elapsed_ns']) / 1e9
    values = [value / 1000 for value in row['stats']['arrival_to_callback_us']]
    result = dict(seconds=seconds, cpu_percent_one_core=(after['process_cpu_ms'] - before['process_cpu_ms']) / seconds / 10,
                  rendered=row['stats']['rendered'], sent=row['sent'], invalidations=row['stats']['invalidations'],
                  duplicates=row['stats']['duplicates'])
    for suffix, fraction in [('p50_ms', .5), ('p95_ms', .95), ('p99_ms', .99)]:
        result['arrival_to_callback_' + suffix] = percentile(values, fraction)
    for key in ['input_calls', 'output_calls', 'input_ns', 'output_ns']:
        result[key + '_per_second'] = (row['dequeues_after'][key] - row['dequeues_before'][key]) / seconds
    for key in ['bytes-allocated', 'gc-count', 'gc-time']:
        name = 'art.gc.' + key
        result['art_' + key] = int(after['runtime'][name]) - int(before['runtime'][name])
    result.update(switches(row))
    result.update(trace_metrics(row))
    return result


def summarize(folder, allow_incomplete=False):
    indexed = defaultdict(list)
    rows = []
    rejected = []
    for path in sorted(folder.glob('*/result.json')):
        raw = json.loads(path.read_text())
        if not raw.get('completed'):
            if not allow_incomplete:
                raise ValueError(f'incomplete trial: {path}')
            rejected.append(dict(path=str(path.relative_to(folder)), reason='incomplete', sent=raw.get('sent'),
                                 invalidations=raw.get('stats', {}).get('invalidations')))
            continue
        row = dict(path=str(path.relative_to(folder)), scene=raw['scene'], fps=raw['send_fps'],
                   variant=raw['variant'], profile=raw['profile'], trial=raw['trial'], metrics=metrics(raw))
        rows.append(row)
        indexed[row['scene'], row['fps'], row['variant'], row['profile']].append(row)
    groups = []
    for key, trials in sorted(indexed.items()):
        group = dict(zip(['scene', 'fps', 'variant', 'profile'], key), trials=len(trials))
        group['metrics'] = {name: distribution([r['metrics'][name] for r in trials]) for name in trials[0]['metrics']}
        groups.append(group)
    return dict(trials=rows, groups=groups, rejected=rejected)


def table(summary):
    lines = ['| Scene / FPS | Profile | Trials | CPU % of one core | Callback p50 ms | Callback p99 ms | Output dequeues/s |',
             '| --- | --- | ---: | ---: | ---: | ---: | ---: |']
    for group in summary['groups']:
        values = group['metrics']
        columns = [values[key]['median'] for key in ['cpu_percent_one_core', 'arrival_to_callback_p50_ms',
                   'arrival_to_callback_p99_ms', 'output_calls_per_second']]
        cells = ' | '.join(f'{number:.2f}' for number in columns)
        lines.append(f'| {group["scene"]} / {group["fps"]} | {group["variant"]}/{group["profile"]} | {group["trials"]} | {cells} |')
    return '\n'.join(lines) + '\n'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('folder', type=Path)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--allow-incomplete', action='store_true', help='record rejected trials separately, never pool them')
    args = parser.parse_args()
    result = summarize(args.folder, args.allow_incomplete)
    args.output.mkdir(exist_ok=True)
    (args.output / 'summary.json').write_text(json.dumps(result, indent=2) + '\n')
    (args.output / 'table.md').write_text(table(result))


if __name__ == '__main__':
    main()
