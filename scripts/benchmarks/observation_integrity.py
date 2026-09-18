"""Offline evidence requirements; historical files are read without modification."""
import gzip
import json
import re


def covered(rows, start, end, gap):
    times = sorted(row['utc'] for row in rows if start <= row.get('utc', -1) < end)
    if len(times) < 2:
        return False
    points = [start, *times, end]
    return all(b - a <= gap for a, b in zip(points, points[1:]))


def identity_problem(rows, expected_pid):
    if not rows:
        return 'Android process evidence missing'
    identities = {(row.get('pid'), row.get('start_ticks')) for row in rows}
    if len(identities) != 1 or any(None in identity for identity in identities):
        return 'Android process changed or identity evidence missing'
    if expected_pid is not None and next(iter(identities))[0] != expected_pid:
        return 'Android process differs from logged PID'
    return None


def android_log_times(folder):
    path = folder / 'android.log'
    if path.exists():
        text = path.read_text()
    else:
        with gzip.open(str(path) + '.gz', 'rt') as source:
            text = source.read()
    return [float(match.group(1)) for line in text.splitlines()
            if (match := re.match(r'^\s*(\d+\.\d+)\s', line))]


def phase_evidence(phases, samples, logs):
    reasons = []
    for start, end in zip(phases, phases[1:]):
        if not start.get('measured'):
            continue
        begin, finish = start['utc'], end['utc']
        if not covered(samples, begin, finish, 30):
            reasons.append('missing or stale samples in measured phase')
        if not any(begin <= row.get('utc', -1) < finish for row in logs):
            reasons.append('missing host logs in measured phase')
    return reasons


def phase_health(phases, health, receipts):
    reasons = []
    for start, end in zip(phases, phases[1:]):
        if not start.get('measured'):
            continue
        if not covered(health, start['utc'], end['utc'], 4):
            reasons.append('tablet session observations missing or stale')
        if not any(start['utc'] <= stamp < end['utc'] for stamp in receipts):
            reasons.append('missing Android log receipts in measured phase')
    return reasons


def guarded_evidence(folder, meta, phases, load_lines):
    report = json.loads((folder / 'observer.json').read_text())
    reasons = list(report.get('errors', []))
    if report.get('complete') is not True:
        reasons.append('observers did not finish cleanly')
    health = load_lines(folder, 'android-session.jsonl')
    problem = identity_problem(health, meta.get('android_pid'))
    if problem:
        reasons.append(problem)
    if any(row.get('foreground') is not True for row in health):
        reasons.append('tablet foreground verification failed')
    reasons.extend(phase_health(phases, health, report.get('log_receipts', {}).get('android.log', [])))
    return reasons


def observation_integrity(folder, meta, phases, samples, logs, load_lines):
    reasons = []
    problem = identity_problem([row.get('android', {}) for row in samples], meta.get('android_pid'))
    if problem:
        reasons.append(problem)
    try:
        if not android_log_times(folder):
            reasons.append('missing Android logs')
        reasons.extend(phase_evidence(phases, samples, logs))
        if meta.get('observation_guard_version') == 1:
            reasons.extend(guarded_evidence(folder, meta, phases, load_lines))
    except (OSError, ValueError, KeyError, TypeError) as error:
        reasons.append(f'missing or invalid observation evidence: {error}')
    return reasons
