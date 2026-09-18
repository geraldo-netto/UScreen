#!/usr/bin/env python3
"""T479: keep decoder-only and combined USB clock boundaries separate."""
import argparse
import collections
import json
from pathlib import Path
import statistics
from profile_usb_wire import receipt


def percentiles(values):
    values = sorted(values)
    if not values:
        raise ValueError('missing timing observations')
    return {key: values[round((len(values) - 1) * q)]
            for key, q in [('p50', .5), ('p95', .95), ('p99', .99)]}


def decoder(folder):
    rows = []
    for path in sorted(folder.glob('*/result.json')):
        result = json.loads(path.read_text())
        choice = result['selection']['decoder_selection']
        if not result.get('completed') or result['selection_receipt'] != receipt(choice):
            raise ValueError(f'incomplete or mismatched decoder trial: {path}')
        values = [(row[2] - row[1]) / 1e6 for row in result['trace'] if row[1] > 0 and row[2] >= row[1]]
        rows.append(dict(scene=result['scene'], rate=result['send_fps'], trial=result['trial'],
                         sent=result['sent'], rendered=result['stats']['rendered'], setup_ms=result['setup_us'] / 1000,
                         feed_to_release_ms=percentiles(values), selection=choice))
    return rows


def usb_phase(phase, acks, offset, rate):
    durations, packet_times = [], []
    first_ack = None
    for index, (frame, packet) in enumerate(zip(phase['frames'], phase['packets'])):
        sequence = offset + index + 1
        if packet['sequence'] != sequence:
            raise ValueError('packet/input identity mismatch')
        ack = acks.get(sequence)
        if ack is None:
            continue
        first_ack = first_ack or ack['acknowledged_ns']
        append_interval(durations, packet_times, frame, packet, ack, index >= rate)
    if len(durations) < (len(phase['frames']) - rate) * .9:
        raise ValueError('insufficient post-warmup ACK coverage')
    return dict(raw_write_to_ack_ms=percentiles(durations), packet_ready_to_ack_ms=percentiles(packet_times),
                samples=len(durations), first_write_to_ack_ms=(first_ack - phase['frames'][0]['admitted_ns']) / 1e6)


def append_interval(durations, packet_times, frame, packet, ack, measured):
    raw = (ack['acknowledged_ns'] - frame['admitted_ns']) / 1e6
    packet_time = (ack['acknowledged_ns'] - packet['ready_ns']) / 1e6
    if not 0 <= packet_time <= raw:
        raise ValueError('inconsistent host clock intervals')
    if measured:
        durations.append(raw)
        packet_times.append(packet_time)


def usb(folder):
    rows = []
    for path in sorted(folder.glob('trial-*/result.json')):
        result = json.loads(path.read_text())
        request = json.loads(path.with_name('request.json').read_text())
        acks = {row['sequence']: row for row in result['acknowledgements']}
        if len(acks) != len(result['acknowledgements']):
            raise ValueError('duplicate ACK')
        if not result['android']['completed']:
            raise ValueError('interrupted USB trial')
        offset = 0
        for phase, observations in enumerate(result['phases']):
            row = usb_phase(observations, acks, offset, request['rate'])
            row.update(trial=path.parent.name, phase=phase, scene=request['scene'], encoder=request['encoder'],
                       rate=request['rate'], setup_ms=result['setups'][phase]['setup_us'] / 1000)
            rows.append(row)
            offset += len(observations['frames'])
    return rows


def aggregate(rows, keys, metric):
    groups = collections.defaultdict(list)
    for row in rows:
        groups[tuple(row[key] for key in keys)].append(row[metric])
    return [dict(zip(keys, key), runs=len(values), median_run_percentiles={
        percentile: statistics.median(row[percentile] for row in values)
        for percentile in ['p50', 'p95', 'p99']}) for key, values in sorted(groups.items())]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--decoder', type=Path, required=True)
    parser.add_argument('--usb', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    decoder_rows, usb_rows = decoder(args.decoder), usb(args.usb)
    result = dict(decoder_trials=decoder_rows, usb_phases=usb_rows,
                  decoder_summary=aggregate(decoder_rows, ['scene', 'rate'], 'feed_to_release_ms'),
                  usb_summary=aggregate(usb_rows, ['scene', 'rate', 'encoder'], 'raw_write_to_ack_ms'))
    args.output.write_text(json.dumps(result, indent=2) + '\n')


if __name__ == '__main__':
    main()
