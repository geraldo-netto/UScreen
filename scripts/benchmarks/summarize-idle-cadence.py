#!/usr/bin/env python3
"""T492: correlate sparse USB replay timing with actual encoded join points."""
import argparse
import json
from pathlib import Path
import subprocess
import profile_usb_pipeline as pipeline

SUMMARY = pipeline.module('summarize-profile-selection')


def validate_packets(phase, probed):
    packets = phase['packets']
    if len(probed) != len(packets) or len(packets) != len(phase['frames']):
        raise ValueError('encoded packet/input counts differ')
    position = 0
    for packet, probe in zip(packets, probed):
        if int(probe['pos']) != position or int(probe['size']) != packet['bytes']:
            raise ValueError('encoded byte positions do not match observed packets')
        position += packet['bytes']


def keyframes(phase, probed):
    validate_packets(phase, probed)
    packets = phase['packets']
    keys = [dict(sequence=packet['sequence'], pts=packet['pts'], ready_ns=packet['ready_ns'])
            for packet, probe in zip(packets, probed) if 'K' in probe['flags']]
    if not keys or keys[0]['sequence'] != packets[0]['sequence']:
        raise ValueError('encoder did not start with an independent picture')
    gaps = [(b['ready_ns'] - a['ready_ns']) / 1e6 for a, b in zip(keys, keys[1:])]
    if any(gap <= 0 for gap in gaps):
        raise ValueError('invalid keyframe observation clock')
    return dict(keys=keys, inter_keyframe_ms=gaps, max_inter_keyframe_ms=max(gaps, default=None))


def probe(folder):
    command = ['ffprobe', '-v', 'error', '-f', 'h264', '-show_packets',
               '-show_entries', 'packet=pos,size,flags', '-of', 'json', str(folder / 'encoded.h264')]
    result = json.loads(subprocess.check_output(command, text=True, timeout=10))
    (folder / 'keyframe-probe.json').write_text(json.dumps(dict(command=command, result=result), indent=2) + '\n')
    return result['packets']


def observations(folder):
    timing = SUMMARY.usb(folder)
    keyed = {(row['trial'], row['phase']): row for row in timing}
    for path in sorted(folder.glob('trial-*/result.json')):
        result = json.loads(path.read_text())
        for number, phase in enumerate(result['phases']):
            row = keyed[(path.parent.name, number)]
            row.update(keyframes(phase, probe(path.parent / f'phase-{number}')))
            row['encoded'] = len(phase['packets'])
            sequences = {packet['sequence'] for packet in phase['packets']}
            row['acknowledged'] = sum(ack['sequence'] in sequences for ack in result['acknowledgements'])
    return timing


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('folder', type=Path)
    args = parser.parse_args()
    rows = observations(args.folder)
    result = dict(phases=rows, completed_phases=len(rows),
                  timing=SUMMARY.aggregate(rows, ['encoder', 'rate'], 'raw_write_to_ack_ms'),
                  boundary='Host raw-write admission to callback ACK, plus encoded keyframe availability; excludes capture and physical presentation.')
    (args.folder / 'idle-summary.json').write_text(json.dumps(result, indent=2) + '\n')
    for row in rows:
        print(row['trial'], row['phase'], row['encoder'], row['rate'], row['raw_write_to_ack_ms'], row['max_inter_keyframe_ms'])


if __name__ == '__main__':
    main()
