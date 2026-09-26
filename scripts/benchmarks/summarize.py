#!/usr/bin/env python3
"""Summarize T382 raw observations without pooling window percentiles."""
import argparse
import gzip
import json
from pathlib import Path
import re
import statistics
from observation_integrity import observation_integrity


def load_lines(folder, name):
    path = folder / name
    if path.exists():
        text = path.read_text()
    else:
        with gzip.open(str(path) + '.gz', 'rt') as stream:
            text = stream.read()
    return [json.loads(line) for line in text.splitlines() if line]


def distribution(values):
    if not values:
        return None
    return dict(n=len(values), minimum=min(values), median=statistics.median(values),
                maximum=max(values), mean=statistics.mean(values))


def cpu_delta(before, after, seconds, hz):
    if before.get('start_ticks') != after.get('start_ticks') or before.get('pid') != after.get('pid'):
        return None
    if 'ticks' not in before or 'ticks' not in after or seconds <= 0:
        return None
    ticks = after['ticks'] - before['ticks']
    return ticks / hz / seconds * 100 if ticks >= 0 else None


def host_cpu(before, after, hz):
    result = {}
    prior = {p['pid']: p for p in before['host']}
    seconds = after['monotonic'] - before['monotonic']
    for proc in after['host']:
        value = cpu_delta(prior.get(proc['pid'], {}), proc, seconds, hz)
        if value is not None:
            result[proc['role']] = result.get(proc['role'], 0) + value
    pipeline = [result.get(role) for role in ['blent', 'evdi_helper', 'ffmpeg']]
    if all(value is not None for value in pipeline):
        result['pipeline_total'] = sum(pipeline)
    return result


def process_summary(samples, meta):
    cpu, rss = {}, {}
    for before, after in zip(samples, samples[1:]):
        values = host_cpu(before, after, meta['host_ticks_per_second'])
        values['android_app'] = cpu_delta(before['android'], after['android'],
                                          after['monotonic'] - before['monotonic'],
                                          meta['android_ticks_per_second'])
        for role, value in values.items():
            if value is not None:
                cpu.setdefault(role, []).append(value)
    for sample in samples:
        processes = sample['host'] + [dict(sample['android'], role='android_app')]
        for process in processes:
            if 'rss_bytes' in process:
                rss.setdefault(process['role'], []).append(process['rss_bytes'] / 1048576)
    return dict(cpu_percent_one_core={k: distribution(v) for k, v in cpu.items()},
                rss_mib={k: distribution(v) for k, v in rss.items()},
                observer_seconds=distribution([r['collection_seconds'] for r in samples]))


def battery_summary(samples):
    rows = [row for row in samples if row.get('battery', {}).get('Charge counter')]
    if len(rows) < 2:
        return None
    first, last = rows[0], rows[-1]
    delta = int(last['battery']['Charge counter']) - int(first['battery']['Charge counter'])
    duration = last['monotonic'] - first['monotonic']
    return dict(samples=len(rows), first_utc=first['utc'], last_utc=last['utc'],
                seconds=duration, charge_delta_uah=delta, net_battery_ma=delta * 3.6 / duration,
                first_level=int(first['battery']['level']), last_level=int(last['battery']['level']),
                temperature_c=distribution([int(row['battery']['temperature']) / 10 for row in rows]),
                usb_reported_values=sorted({(row['battery']['USB powered'],
                                            row['battery']['Max charging current'],
                                            row['battery']['Max charging voltage']) for row in rows}))


PATTERNS = {
    'packet_ready_to_ack_window': r'Latency (?:encode→display|packet-ready→render-ACK \(host clock\)): p50 ([\d.]+)ms\s+p95 ([\d.]+)ms\s+max ([\d.]+)ms\s+\((\d+) samples',
    'tablet_arrival_to_callback_window': r'(?:of which tablet decode\+render|Latency tablet arrival→render-callback \(tablet clock\):) p50 ([\d.]+)ms\s+p95 ([\d.]+)ms',
    'capture_to_fifo_window': r'capture→fifo p50 ([\d.]+)ms p95 ([\d.]+)ms \((\d+) frames',
    'encoder_window': r'Encoder: (\d+) access units in ([\d.]+)s, [\d.]+ MB/s \(([\d.]+) kbps\)',
    'capture_cycle_window': r'cycle: request→ready avg ([\d.]+)ms \(\d+ waited, \d+ immediate\), grab avg ([\d.]+)ms',
    'grabs_per_second': r'\] ([\d.]+) grabs/s',
}
FIELDS = {
    'packet_ready_to_ack_window': ['p50_ms', 'p95_ms', 'max_ms', 'samples'],
    'tablet_arrival_to_callback_window': ['p50_ms', 'p95_ms'],
    'capture_to_fifo_window': ['p50_ms', 'p95_ms', 'samples'],
    'encoder_window': ['access_units', 'seconds', 'kbps'],
    'capture_cycle_window': ['request_ready_mean_ms', 'grab_mean_ms'],
    'grabs_per_second': ['fps'],
}


def window_values(logs):
    values = {}
    for row in logs:
        for name, pattern in PATTERNS.items():
            match = re.search(pattern, row['message'])
            if match:
                result = dict(zip(FIELDS[name], map(float, match.groups())))
                if name == 'encoder_window':
                    result['fps'] = result['access_units'] / result['seconds']
                values.setdefault(name, []).append(result)
    return {name: {field: distribution([row[field] for row in rows]) for field in rows[0]}
            for name, rows in values.items()}


def phase_summary(phase, end, samples, logs, meta):
    selected = [row for row in samples if phase['utc'] <= row['utc'] < end]
    # Drop the first 10 seconds of five-second log windows after each boundary.
    windows = [row for row in logs if phase['utc'] + 10 <= row['utc'] < end]
    return dict(trial=phase['trial'], kind=phase['kind'], start_utc=phase['utc'], end_utc=end,
                sample_count=len(selected), processes=process_summary(selected, meta),
                battery=battery_summary(selected), log_windows=window_values(windows))


def visibility_integrity(folder, meta, phases):
    reasons = [row['reason'] for row in phases if row.get('event') == 'invalid']
    marker = folder / 'invalid.json'
    if marker.exists():
        reasons.append(json.loads(marker.read_text())['reason'])
    guarded = meta.get('visibility_guard_version') == 1
    complete = bool(phases and phases[-1]['event'] == 'complete')
    if guarded:
        if not complete:
            reasons.append('workload did not complete')
        if not all(row.get('visibility_verified') is True for row in phases):
            reasons.append('missing phase visibility verification')
    return complete and not reasons, guarded, list(dict.fromkeys(reasons))


def read_evidence(folder, name, reasons):
    try:
        return load_lines(folder, name)
    except (OSError, ValueError) as error:
        reasons.append(f'missing or invalid {name}: {error}')
        return []


def summarize(folder):
    meta = json.loads((folder / 'metadata.json').read_text())
    missing = []
    samples = read_evidence(folder, 'samples.jsonl', missing)
    phases = read_evidence(folder, 'phases.jsonl', missing)
    logs = read_evidence(folder, 'host-windows.jsonl', missing)
    complete, guarded, reasons = visibility_integrity(folder, meta, phases)
    reasons.extend(missing)
    reasons.extend(observation_integrity(folder, meta, phases, samples, logs, load_lines))
    complete = complete and not reasons
    summaries = []
    for start, end in zip(phases, phases[1:]):
        if start.get('measured'):
            summaries.append(phase_summary(start, end['utc'], samples, logs, meta))
    result = dict(complete=complete, source_commit=meta['source_commit'], phases=summaries,
                whole_run_battery=battery_summary(samples),
                semantics='Latency distributions describe logged five-second window statistics, not pooled frame percentiles. CPU 100% is one core. Negative battery mA is net discharge while USB powered.')
    result.update(visibility='invalid' if reasons else ('verified' if guarded else 'unverified'),
                  observation='invalid' if reasons else ('verified' if meta.get('observation_guard_version') == 1 else 'unverified'),
                  invalid_reasons=reasons)
    if reasons:
        result['invalid_data'] = dict(phases=result['phases'], whole_run_battery=result['whole_run_battery'])
        result.update(phases=[], whole_run_battery=None)
    return result


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('folder', type=Path)
    args = parser.parse_args()
    print(json.dumps(summarize(args.folder), indent=2))
