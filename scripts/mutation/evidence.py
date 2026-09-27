"""Strict cargo-mutants 27.1.0 result import; incomplete runs never pass."""
from collections import Counter
import json
import re

CLASSES = {'CaughtMutant': 'caught', 'MissedMutant': 'survived',
           'Unviable': 'unviable', 'Timeout': 'timeout'}


def identity(mutant):
    return json.dumps([mutant['file'], mutant['span'], mutant['replacement']], sort_keys=True)


def baseline_passed(outcomes, directory):
    baselines = [row for row in outcomes if row['scenario'] == 'Baseline']
    if len(baselines) != 1:
        return False
    row = baselines[0]
    phases = {part['phase']: part['process_status'] for part in row['phase_results']}
    log = (directory / row['log_path']).read_text(errors='replace')
    return (row['summary'] == 'Success' and phases == {'Build': 'Success', 'Test': 'Success'}
            and any(int(n) for n in re.findall(r'test result: ok\. (\d+) passed', log)))


def classify(row):
    value = CLASSES.get(row['summary'], 'tool_error')
    phases = {part['phase']: part['process_status'] for part in row['phase_results']}
    if value in {'caught', 'survived'} and phases.get('Build') != 'Success':
        return 'tool_error'
    if value == 'caught' and not failed_test(phases.get('Test')):
        return 'tool_error'
    if value == 'survived' and phases.get('Test') != 'Success':
        return 'tool_error'
    return value


def failed_test(status):
    return (isinstance(status, dict) and isinstance(status.get('Failure'), int)
            and status['Failure'] != 0)


def summarize(directory, process):
    candidates = json.loads((directory / 'mutants.json').read_text())
    outcomes = json.loads((directory / 'outcomes.json').read_text())['outcomes']
    expected = [identity(row) for row in candidates]
    rows = [row for row in outcomes if row['scenario'] != 'Baseline']
    actual = [identity(row['scenario']['Mutant']) for row in rows]
    unique = len(set(expected)) == len(expected) and len(set(actual)) == len(actual)
    counts = Counter(classify(row) for row in rows)
    baseline = baseline_passed(outcomes, directory)
    complete = bool(expected) and unique and set(actual) == set(expected)
    valid_process = process['status'] == 'completed' and process.get('code') in {0, 2, 3, 4}
    passed = baseline and complete and valid_process and counts['caught'] == len(expected)
    return dict(baseline_passed=baseline, complete=complete, unique=unique,
                candidates=len(expected), outcomes=dict(counts), passes=passed,
                whole_project_mutation_score=None)
