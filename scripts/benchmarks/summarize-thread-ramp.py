#!/usr/bin/env python3
"""T600: per-trial percentiles, then medians across repeats; retain every trial."""
import argparse
import json
from pathlib import Path
import re
import statistics
import thread_ramp_common as C


def distribution(rows, name, values):
    for label, quantile in [('p50', .5), ('p95', .95), ('p99', .99)]:
        rows[name + '_' + label] = C.percentile(values, quantile)


def conversion(folder):
    rows = []
    for data in json.loads((folder / 'results.json').read_text()):
        lane = data['lanes'][0]
        row = {key: data[key] for key in ['workers', 'repeat', 'damage', 'cpu_us', 'rss_peak_kib', 'voluntary_switches', 'involuntary_switches']}
        row.update(convert_p50_us=lane['convert_p50_ns'] / 1000,
                   convert_p99_us=lane['convert_p99_ns'] / 1000, effective_jobs=lane['last_jobs'])
        rows.append(row)
    return rows


def encoder_trial(path):
    data = json.loads(path.read_text())
    repeat, workers = map(int, path.parent.name.split('-'))
    acks = {r['sequence']: r['acknowledged_ns'] for r in data['acknowledgements']}
    assert len(acks) == len(data['acknowledgements']), 'duplicate ACK'
    assert all(1 <= index <= 360 for index in acks), 'unrelated ACK'
    row = dict(repeat=repeat, workers=workers, **data['resources'])
    with path.with_name('encoded.h264').open('rb') as encoded:
        parameters = dict(re.findall(rb'\b(threads|lookahead_threads|sliced_threads|slices)=(\d+)', encoded.read(16384)))
    row['effective_encoder_threads'] = int(parameters[b'threads'])
    row.update(cpu_seconds=row['user_seconds'] + row['system_seconds'],
               frames=len(data['packets']), acks=len(acks), encoded_bytes=sum(p['bytes'] for p in data['packets']))
    encode, end_to_end, scheduled = [], [], []
    for index in range(60, 300):
        frame, packet = data['frames'][index], data['packets'][index]
        assert packet['sequence'] == index + 1
        encode.append((packet['ready_ns'] - frame['admitted_ns']) / 1e6)
        if index + 1 in acks:
            end_to_end.append((acks[index + 1] - frame['admitted_ns']) / 1e6)
            scheduled.append((acks[index + 1] - frame['scheduled_ns']) / 1e6)
    row['measured_missing_acks'] = 240 - len(end_to_end)
    distribution(row, 'encode_ms', encode)
    distribution(row, 'ack_ms', end_to_end)
    distribution(row, 'scheduled_ack_ms', scheduled)
    return row


def runtime(folder):
    rows = json.loads((folder / 'results.json').read_text())
    for row in rows:
        distribution(row, 'loopback_us', [n / 1000 for n in row.pop('latency_ns')])
    return rows


def aggregate(rows, keys):
    groups = {}
    for row in rows:
        groups.setdefault(tuple(row[k] for k in keys), []).append(row)
    result = []
    for group, trials in sorted(groups.items()):
        numeric = {k: statistics.median(r[k] for r in trials) for k, v in trials[0].items()
                   if k not in [*keys, 'repeat'] and isinstance(v, (int, float))}
        result.append(dict(zip(keys, group), repeats=len(trials), **numeric))
    return result


def main(args):
    raw = dict(conversion=conversion(args.input / 'conversion'),
               conversion_1280=conversion(args.input / 'conversion-1280'), runtime=runtime(args.input / 'runtime'),
               encoder=[encoder_trial(p) for p in sorted((args.input / 'encoder').glob('*/result.json'))])
    summary = {name: aggregate(rows, ['workers', 'damage'] if name.startswith('conversion') else ['workers'])
               for name, rows in raw.items()}
    C.save(args.output, dict(method='nearest-rank per trial, median of three trials per setting', summary=summary, trials=raw))


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--input', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    main(parser.parse_args())
