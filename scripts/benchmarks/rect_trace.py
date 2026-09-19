"""T419: validate frame identity before diagnosing absent presentations."""
from collections import defaultdict
import csv
import io
import statistics
import subprocess


def query(processor, trace, sql):
    command = [processor, 'query', str(trace), sql]
    result = subprocess.run(command, check=True, capture_output=True, text=True, timeout=30)
    return list(csv.DictReader(io.StringIO(result.stdout)))


def clock_offset(rows):
    offsets = [int(row['ts']) - int(row['clock_value']) for row in rows]
    if not offsets or max(offsets) - min(offsets) > 2_000:
        raise ValueError('missing or unstable BOOTTIME/MONOTONIC conversion')
    return int(statistics.median(offsets))


def events_by_frame(rows):
    events = defaultdict(lambda: defaultdict(set))
    for row in rows:
        events[int(row['frame_number'])][row['name']].add(int(row['ts']))
    return events


def identity(events, surface, offset):
    candidates = []
    presentations = [(number, value - offset) for number, event in events.items()
                     for value in event.get('PresentFenceSignaled', [])]
    for sequence, presented in surface.items():
        matches = [number for number, at in presentations if abs(at - presented) <= 2_000]
        if len(matches) == 1:
            candidates.append(matches[0] - sequence)
    if len(candidates) < 10 or len(set(candidates)) != 1:
        raise ValueError('sequence/frame-number identity is absent or inconsistent')
    return dict(frame_offset=candidates[0], validated_pairs=len(candidates))


def classify(sequence, mapping, events, surface):
    if sequence in surface:
        return 'presented_in_surface_history'
    event = events.get(sequence + mapping['frame_offset'], {})
    if not event.get('Queue'):
        return 'unknown_missing_queue'
    if not event.get('Latch'):
        return 'queued_not_latched'
    if event.get('PresentFenceSignaled'):
        return 'present_fence_without_history'
    return 'latched_without_present_fence'


def diagnose(trace, processor, result, records):
    layers = {row['layer'] for row in records}
    if len(layers) != 1:
        raise ValueError('ambiguous replay layer')
    layer = next(iter(layers))
    quoted = "'" + layer.replace("'", "''") + "'"
    errors = query(processor, trace, "SELECT name,value FROM stats WHERE severity IN ('error','data_loss') AND value>0")
    if errors:
        raise ValueError('trace errors prevent a complete-coverage claim: ' + str(errors))
    rows = query(processor, trace, "SELECT frame_number,name,ts FROM frame_slice WHERE layer_name=" + quoted +
                 " AND name IN ('Queue','Latch','PresentFenceSignaled') ORDER BY ts")
    clocks = query(processor, trace, "SELECT ts,clock_value FROM clock_snapshot WHERE clock_name='MONOTONIC'")
    offset = clock_offset(clocks)
    events = events_by_frame(rows)
    surface = surface_sequences(records)
    mapping = identity(events, surface, offset)
    return coverage(result, events, surface, offset, mapping, layer)


def surface_sequences(records):
    result = {}
    for row in {tuple(row) for record in records for row in record['frames']}:
        token, present, _ = row
        if token <= 0 or token % 1000 or not 0 < present < 2**63 - 1:
            continue
        sequence = token // 1000
        if sequence in result and result[sequence] != present:
            raise ValueError('ambiguous SurfaceFlinger sequence')
        result[sequence] = present
    return result


def coverage(result, events, surface, offset, mapping, layer):
    begin = result['before']['elapsed_ns'] + 1_000_000_000
    end = result['after']['elapsed_ns'] - 2_000_000_000
    timed = [row for row in result['trace'] if begin <= row[1] < end]
    if not timed:
        raise ValueError('empty interior measurement window')
    queue = [at - offset for event in events.values() for at in event.get('Queue', [])]
    if not queue or min(queue) > begin or max(queue) < end:
        raise ValueError('trace does not span the interior window')
    classifications = defaultdict(list)
    for row in timed:
        classifications[classify(row[0], mapping, events, surface)].append(row[0])
    return dict(layer=layer, boottime_minus_monotonic_ns=offset, mapping=mapping,
                eligible=len(timed), classification=dict(classifications),
                queue_frames=len([event for event in events.values() if event.get('Queue')]))
