#!/usr/bin/env python3
"""T419: keep swap, EGL presentation and codec callback boundaries separate."""
import argparse
import json
from pathlib import Path
import re
import statistics


def distribution(values):
    values = sorted(values)
    if not values:
        return None
    return {name: values[int(fraction * (len(values) - 1))]
            for name, fraction in [('p50', .5), ('p95', .95), ('p99', .99)]}


def cpu(before, after):
    seconds = (after['elapsed_ns'] - before['elapsed_ns']) / 1e9
    milliseconds = after['process_cpu_ms'] - before['process_cpu_ms']
    if seconds <= 0 or milliseconds < 0:
        raise ValueError('invalid CPU interval')
    return dict(seconds=seconds, cpu_ms=milliseconds, cpu_percent_one_core=milliseconds / seconds / 10)


def allocation(before, after):
    keys = ['art.gc.bytes-allocated', 'art.gc.bytes-freed', 'art.gc.gc-count', 'art.gc.gc-time']
    return {key: int(after['runtime'][key]) - int(before['runtime'][key]) for key in keys
            if key in before['runtime'] and key in after['runtime']}


def rectangles(result):
    trace = result['trace']
    if len(trace) != result['count']:
        raise ValueError('missing rectangle trace rows')
    updates = [row for row in trace if row[5] > 0]
    present = [row for row in updates if row[7] > 0]
    if any(row[7] < row[1] for row in present):
        raise ValueError('presentation precedes local admission')
    pairs = {'read_decode_ms': (2, 1), 'upload_ms': (3, 2), 'swap_ms': (4, 3),
             'admission_to_swap_ms': (4, 1), 'schedule_lateness_ms': (1, 0)}
    metrics = {name: distribution([(row[end] - row[start]) / 1e6 for row in updates])
               for name, (end, start) in pairs.items()}
    metrics.update(presented=len(present), updates=len(updates), inputs=len(trace),
                   absent_presentation=len(updates) - len(present),
                   admission_to_egl_present_ms=distribution([(row[7] - row[1]) / 1e6 for row in present]),
                   schedule_to_egl_present_ms=distribution([(row[7] - row[0]) / 1e6 for row in present]),
                   compressed_storage_bytes=result['compressed_storage_bytes'],
                   input_mode=result['input_mode'])
    return metrics


def video(result):
    stats = result['stats']
    if stats['duplicates'] or stats['invalidations']:
        raise ValueError('invalid video callback sequence')
    return dict(inputs=result['sent'], callbacks=stats['rendered'],
                missing_callbacks=result['sent'] - stats['rendered'],
                admission_to_callback_ms=distribution([value / 1000 for value in stats['arrival_to_callback_us']]))


def memory(folder):
    rows = []
    for path in sorted(folder.glob('memory-*.txt')):
        raw = path.read_text()
        match = re.search(r'TOTAL PSS:\s+(\d+)\s+TOTAL RSS:\s+(\d+)\s+TOTAL SWAP PSS:\s+(\d+)', raw)
        if match is None:
            raise ValueError('missing memory observation: ' + str(path))
        rows.append(dict(source=path.name, total_pss_mib=int(match[1]) / 1024,
                         total_rss_mib=int(match[2]) / 1024, total_swap_pss_mib=int(match[3]) / 1024))
    return rows


def service_cpu(folder, hz):
    samples = [json.loads(line) for line in (folder / 'resources.jsonl').read_text().splitlines()]
    # Trim initial warmup and final close observations. Boundary remains an
    # external sampling window, not exactly the app's own timed interval.
    if len(samples) < 7:
        return {}
    first, last = samples[2], samples[-2]
    return {name: sampled_cpu(first[name], last[name], hz) for name in first}


def sampled_cpu(first, last, hz):
    if (first['pid'], first['start_ticks']) != (last['pid'], last['start_ticks']):
        raise ValueError('resource process identity changed')
    seconds = (last['at_ns'] - first['at_ns']) / 1e9
    ticks = last['ticks'] - first['ticks']
    if seconds <= 0 or ticks < 0:
        raise ValueError('invalid sampled CPU interval')
    return dict(seconds=seconds, cpu_percent_one_core=100 * ticks / hz / seconds)


def surface_frames(folder):
    path = folder / 'surface.jsonl'
    if not path.exists():
        return None
    records = [json.loads(line) for line in path.read_text().splitlines()]
    return {tuple(row) for record in records for row in record['frames'] if 0 < row[1] < 2**63 - 1}


def surface_metrics(folder, result):
    frames = surface_frames(folder)
    if frames is None:
        return None
    begin = result['before']['elapsed_ns'] + 1_000_000_000
    end = result['after']['elapsed_ns'] - 2_000_000_000
    # Interior window avoids layer creation/destruction truncating the ring.
    rows = [row for row in result['trace'] if begin <= row[1] < end]
    if result.get('mode') == 'rect':
        visible = {row[1] for row in frames}
        joined = [(row[1], row[7]) for row in rows if row[5] > 0 and row[7] in visible]
        expected = sum(row[5] > 0 for row in rows)
    else:
        joined, expected = video_presentations(rows, frames)
    if any(present < admitted for admitted, present in joined):
        raise ValueError('SurfaceFlinger presentation precedes admission')
    return dict(window='one second after timed start through two seconds before timed end',
                expected=expected, joined=len(joined), missing=expected - len(joined),
                unique_present_times=len({row[1] for row in joined}),
                admission_to_surface_present_ms=distribution([(b - a) / 1e6 for a, b in joined]))


def video_presentations(rows, frames):
    # Decoder replay uses sequence IDs as MediaCodec presentationTimeUs.
    # This tablet exposes those IDs * 1000 in the first ring column.
    by_sequence = {}
    for token, presented, _ in frames:
        if token <= 0 or token % 1000:
            continue
        sequence = token // 1000
        if sequence in by_sequence and by_sequence[sequence] != presented:
            raise ValueError('ambiguous sequence/presentation identity')
        by_sequence[sequence] = presented
    return [(row[1], by_sequence[row[0]]) for row in rows if row[0] in by_sequence], len(rows)


def trial(folder, meta):
    result = json.loads((folder / 'result.json').read_text())
    if not result.get('completed') or result.get('verified'):
        raise ValueError('incomplete or verification-only trial cannot become a timing result')
    measurements = rectangles(result) if result.get('mode') == 'rect' else video(result)
    return dict(path=folder.name, scene=result['scene'], codec=result['case'],
                cpu=cpu(result['before'], result['after']), allocations=allocation(result['before'], result['after']),
                metrics=measurements, memory=memory(folder), services=service_cpu(folder, meta['clock_ticks']),
                surface=surface_metrics(folder, result))


def cohorts(trials):
    rows = []
    keys = sorted({(row['scene'], row['codec']) for row in trials})
    for scene, codec in keys:
        cases = [row for row in trials if (row['scene'], row['codec']) == (scene, codec)]
        app_memory = [m['total_pss_mib'] for row in cases for m in row['memory']
                      if m['source'].endswith('-com.uscreen.rectbench.txt')]
        rows.append(dict(scene=scene, codec=codec, trials=len(cases),
                         cpu_percent_one_core=statistics.median(row['cpu']['cpu_percent_one_core'] for row in cases),
                         sampled_app_pss_mib=distribution(app_memory),
                         sample_count=len(app_memory)))
    return rows


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('folder', type=Path)
    args = parser.parse_args()
    meta = json.loads((args.folder / 'metadata.json').read_text())
    if meta['verification']:
        raise ValueError('verification run is not performance evidence')
    rows = [trial(path.parent, meta) for path in sorted(args.folder.glob('*/result.json'))]
    result = dict(trials=rows, cohorts=cohorts(rows), completed_trials=len(rows))
    (args.folder / 'summary.json').write_text(json.dumps(result, indent=2) + '\n')
    for row in result['cohorts']:
        print(row)


if __name__ == '__main__':
    main()
