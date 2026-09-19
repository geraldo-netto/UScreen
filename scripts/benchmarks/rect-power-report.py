#!/usr/bin/env python3
"""T419 signed battery flow; USB input power is not measured by these counters."""
import argparse
from collections import defaultdict
import json
import math
from pathlib import Path
import statistics
from rect_trace import clock_offset, query


def integrate(times, values):
    if len(times) != len(values) or len(times) < 2:
        raise ValueError('at least two paired battery samples are required')
    if any(b <= a for a, b in zip(times, times[1:])):
        raise ValueError('battery sample clocks are not strictly increasing')
    return sum((a + b) / 2 * (end - begin) / 1e9
               for begin, end, a, b in zip(times, times[1:], values, values[1:]))


def battery_summary(samples, begin, end):
    selected = [row for row in samples if begin <= row['ts'] <= end]
    if len(selected) < 3:
        raise ValueError('insufficient battery samples in the stable window')
    times = [row['ts'] for row in selected]
    if times[0] - begin > 6_000_000_000 or end - times[-1] > 6_000_000_000:
        raise ValueError('battery samples do not cover the stable window')
    if max(b - a for a, b in zip(times, times[1:])) > 6_000_000_000:
        raise ValueError('battery sampling gap exceeds the five-second cadence tolerance')
    seconds = (times[-1] - times[0]) / 1e9
    current = [row['batt.current_ua'] for row in selected]
    charge = [row['batt.charge_uah'] for row in selected]
    voltage = [row['batt.voltage_uv'] for row in selected]
    total = integrate(times, current)
    return dict(samples=len(times), seconds=seconds, mean_current_ma=total / seconds / 1000,
                integrated_net_charge_mah=total / 3_600_000,
                gauge_net_charge_mah=(charge[-1] - charge[0]) / 1000,
                nonzero_gauge_steps_uah=sorted({abs(b - a) for a, b in zip(charge, charge[1:]) if b != a}),
                mean_voltage_v=integrate(times, voltage) / seconds / 1e6,
                current_range_ua=[min(current), max(current)],
                capacity_range_pct=[min(row['batt.capacity_pct'] for row in selected),
                                    max(row['batt.capacity_pct'] for row in selected)])


def counters(processor, trace):
    errors = query(processor, trace, "SELECT name,value FROM stats WHERE severity IN ('error','data_loss') AND value>0")
    if errors:
        raise ValueError('trace contains errors or data loss: ' + str(errors))
    offset = clock_offset(query(processor, trace, "SELECT ts,clock_value FROM clock_snapshot WHERE clock_name='MONOTONIC'"))
    rows = query(processor, trace, 'SELECT c.ts,t.name,c.value FROM counter c JOIN counter_track t ON c.track_id=t.id ORDER BY c.ts')
    combined = defaultdict(dict)
    for row in rows:
        timestamp = int(row['ts']) - offset
        combined[timestamp][row['name']] = float(row['value'])
    required = {'batt.current_ua', 'batt.charge_uah', 'batt.voltage_uv', 'batt.capacity_pct'}
    result = []
    for timestamp, row in sorted(combined.items()):
        validate_counters(row, required)
        result.append(dict(ts=timestamp, **row))
    return result


def validate_counters(row, required):
    if not required <= row.keys():
        raise ValueError('missing battery property; unsupported is not zero')
    if not all(math.isfinite(row[name]) for name in required):
        raise ValueError('non-finite battery property')
    if not 0 <= row['batt.capacity_pct'] <= 100:
        raise ValueError('invalid battery capacity')
    if row['batt.voltage_uv'] <= 0 or row['batt.charge_uah'] < 0:
        raise ValueError('unavailable voltage or charge counter')


def trial(folder, processor):
    results = list((folder / 'replay').glob('*/result.json'))
    if len(results) != 1:
        raise ValueError('expected one replay in each battery phase')
    result = json.loads(results[0].read_text())
    if not result.get('completed') or result.get('verified'):
        raise ValueError('incomplete/verification replay is not battery evidence')
    begin, end = result['before']['elapsed_ns'], result['after']['elapsed_ns']
    samples = counters(processor, folder / 'power.pftrace')
    # Exclude the first 30 measured seconds as a settling window for every path.
    battery = battery_summary(samples, begin + 30_000_000_000, end)
    seconds = (end - begin) / 1e9
    return dict(path=folder.name, battery=battery, samples=samples,
                scene=result['scene'], codec=result['case'],
                input_rate=result.get('rate', result.get('send_fps')),
                timed_seconds=seconds,
                cpu_percent_one_core=(result['after']['process_cpu_ms'] - result['before']['process_cpu_ms']) / seconds / 10,
                reported_updates=result.get('updates', result.get('stats', {}).get('rendered')))


def groups(trials):
    grouped = defaultdict(list)
    for row in trials:
        grouped[(row['scene'], row['codec'], row['input_rate'])].append(row)
    return [dict(scene=key[0], codec=key[1], rate=key[2], trials=len(rows),
                 mean_current_ma=statistics.mean(r['battery']['mean_current_ma'] for r in rows),
                 per_trial_mean_current_ma=[r['battery']['mean_current_ma'] for r in rows],
                 gauge_net_charge_mah=[r['battery']['gauge_net_charge_mah'] for r in rows],
                 integrated_net_charge_mah=[r['battery']['integrated_net_charge_mah'] for r in rows])
            for key, rows in sorted(grouped.items())]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('folder', type=Path)
    parser.add_argument('--processor', required=True)
    args = parser.parse_args()
    trials = [trial(path.parent, args.processor) for path in sorted(args.folder.glob('*/power.pftrace'))]
    result = dict(completed_phases=len(trials), trials=trials, groups=groups(trials),
                  boundary='Net battery flow with USB plugged in; positive current charges the battery. No USB input power measurement.')
    (args.folder / 'summary.json').write_text(json.dumps(result, indent=2) + '\n')
    for row in result['groups']:
        print(row)


if __name__ == '__main__':
    main()
